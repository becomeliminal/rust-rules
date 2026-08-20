# Architecture

Hermetic Rust build rules for Please: the `go_repo` experience, for Rust.
This document covers the internals; see the top-level README for usage.

Third-party crates are declared once, by name and version. Everything else,
transitive dependency routing, feature unification, BUILD file generation,
compilation, is computed by `please_rust`, a single tool that reimplements
cargo's build contract offline. **Cargo is never invoked**: not at build time,
not at resolution time, not to add a dependency.

```python
# third_party/rust/BUILD, maintained by `please_rust sync`
rust_repo(
    name = "serde",
    crate = "serde",
    version = "1.0.228",
    features = ["derive", "default"],
    hashes = ["9a8e94ea7f378bd32cbbd37198a4a91436180c5bb14e6ce8b44b779e8c1785b3"],
)
```

```python
# your code
subinclude("//build_defs:rust")

rust_binary(
    name = "app",
    main = "src/main.rs",
    deps = ["//third_party/rust:serde", "//third_party/rust:tokio"],
)
```

## How it works

The architecture mirrors [go-rules](https://github.com/please-build/go-rules),
which exists for the same reason: the language's native tool is not hermetic,
so its *contract* is reimplemented under the build system's control.

| Layer | What happens | Network |
|---|---|---|
| `rust_toolchain` | rustc/cargo/stdlib fetched from static.rust-lang.org, sha256-pinned, then split into the pieces a build actually uses (`_rustc`, `_cargo`, `_sysroot`, `_llvm_tools`, `_sysroot_src`) so a compile stages the compiler and not the whole distribution | fetch rule only |
| `rust_repo` | crate tarball fetched from static.crates.io via `remote_file`, sha256-verified | fetch rule only |
| `please_rust generate` | parses the crate's Cargo.toml, emits BUILD rules into a Please subrepo | none |
| `please_rust resolve` | semver version routing + cargo resolver-v2 feature unification across the declared graph, computed by the `rust_resolve` rule inside the build graph | none |
| `please_rust compile` / `build-script` | drives rustc with cargo's full env contract (`CARGO_PKG_*`, `OUT_DIR`, feature cfgs, proc-macro externs, build script directives) | none |
| `please_rust sync` | maintains the `rust_repo` list: naming, hashes, pruning; imports a cargo `Cargo.lock` wholesale, or a whole workspace with `--import-workspace` | none |
| `please_rust ide` | describes the whole crate graph for rust-analyzer, from the lock and from per-crate fragments the first-party rules emit | none |
| `please_rust lock --add crate@req` | PubGrub solve over the crates.io sparse index (cached, `--offline` supported), MSRV-filtered against the declared toolchain; hashes from index checksums | dev-time only |

The `rust_repo` declarations play the role `go.mod` plays for go-rules: the
committed, deterministic resolution artifact. The computed feature sets and
dependency routing are derived from them by the `rust_resolve` rule inside
the build graph (nothing else is checked in), so builds are reproducible
with no resolver in the loop and no lockfile to drift.

Multiple versions of a crate coexist: the newest declared version owns the
plain name (`indexmap`), older duplicates are suffixed (`indexmap-1.9.3`),
and every compile gets cargo-style `-C metadata`/`-C extra-filename`
disambiguation. Renamed deps (`package = "..."`), build scripts (with real
`OUT_DIR` handling), proc-macros, and platform-specific `[target.'cfg(...)']`
dependencies (evaluated with `cfg-expr`) are all supported.

## Adding a dependency

```sh
# From the crates.io index (no cargo required anywhere):
plz run //tools/please_rust -- lock --add axum@0.7

# Or import an existing cargo project's entire lockfile:
plz run //tools/please_rust -- sync --import path/to/Cargo.lock

# Or port a whole cargo workspace, BUILD files and all:
plz run //tools/please_rust -- sync --import-workspace path/to/workspace

# After hand-editing rust_repo declarations, re-resolve:
plz run //tools/please_rust -- sync
```

All three rewrite the declarations in place; resolution itself is derived
in the build graph, so there is no lock file to commit or drift.
Feature requests live on the `rust_repo` entries you name; everything
transitive is derived.

## First-party rules

`rust_library` (all crate types: rlib, proc-macro, dylib, cdylib,
staticlib), `rust_binary` (statically linked by default), `rust_test` (unit,
integration and `rust_doc_test` doctests, with per-test reporting and
`plz cover` support), `rust_benchmark` (criterion), `rust_bindgen`,
`rust_clippy`, `rust_fmt_test` and `rust_doc` all drive the same
`please_rust compile` machinery and interoperate with `rust_repo` crates in
both directions. C interop runs both ways via `cc_deps` and `staticlib`.
See `examples/` and `test/`.

```python
rust_library(
    name = "lib",
    root = "src/lib.rs",
    modules = ["src/module_a.rs"],
    deps = ["//third_party/rust:serde"],
)

rust_test(
    name = "test",
    root = "src/lib.rs",
)
```

## Editor integration

rust-analyzer learns a crate graph either by running cargo or by being handed
a `rust-project.json`. There is no cargo to run, so the second applies.
Generating that file and remembering to regenerate it is not how this works.

go-rules ships a `GOPACKAGESDRIVER` binary that gopls calls instead of
`go list`. rust-analyzer has the same shape in
`rust-analyzer.workspace.discoverConfig`: a command it runs when a project is
opened, and runs again when a watched file changes. `rust_project` answers it.

```
editor opens, or a BUILD file changes
        │
        ▼
plz run //:rust-project -- --discover {arg}
        │
        ├─ query the build graph for every crate, this repo and its subrepos
        ├─ build what the project is about to point at, from what it points at
        └─ emit JSONL: progress lines, then a `finished` object carrying the graph
```

Three things are worth knowing about the shape:

**Discovery is a query, so it cannot be a build rule.** A rule cannot ask the
graph what is in it while that graph is being parsed. That is why this is a
run target rather than something `plz build` produces, and why the same target
also writes a file when run by hand, which is what CI asserts against.

**Third-party crates never go through the query.** They come from the lock,
which already records where each crate's sources landed, so a crate in a
subrepo needs no discovery at all. Only first-party crates are found by
querying, which is why subrepo support is about the query half only.

**Whatever the project names, gets built.** The toolchain, the standard
library's sources, the proc-macro dylibs: naming a path is not the same as it
existing, and the difference is silent every time. rust-analyzer degrades and
reports something unrelated. The tool emits the paths it is about to write and
the driver builds anything absent, rather than keeping a list of artifact
kinds that has to stay right.

## Bootstrap

`//tools/please_rust:please_rust` is self-hosted: built by these rules from
the `rust_repo` graph. The `:bootstrap` genrule builds it once with cargo to
break the egg (exactly as go-rules bootstraps please_go); nothing else ever
runs cargo.

## Notes for plugin authors

Two things about Please's plugin model caused every consumer-facing bug this
plugin has shipped, and neither is obvious from the outside.

**A plugin's `.plzconfig` is shipped to its consumers.** `plugin_repo`
downloads the repo, config and all, so anything under `[Parse]` becomes a
requirement on every repo that parses a package of the plugin: preload a
subinclude and consumers must have that plugin declared too. go-rules ships
no `[Parse]` section at all, which is the discipline to copy. This repo's
own BUILD files subinclude explicitly instead.

**Subrepo names are global and unqualified.** Please derives them from the
declaring package path plus the name, with no prefix identifying which repo
declared them, so a plugin's `third_party/rust/serde` and a consumer's are
one name and collide the moment both are parsed. That is why this repo keeps
its own crates in `third_party/crates`: `rust_repo` derives both
`third_party_path` and the lock label from the package the declarations live
in, so a plugin only has to put them somewhere its consumers will not. The
same applies to any plugin declaring crates on a consumer's behalf.

**Plugin names are a second global namespace, separate from subrepo names.**
A subrepo that declares a plugin its consumer also declares collides, and
Please stops rather than choosing. It only surfaces when something parses that
subrepo's own `plugins/` package, which a repo-wide query does. Anything sweeping a subrepo should be able to skip a package that will
not parse rather than lose the subrepo.

**A nested `plz` does not inherit command-line overrides.** A tool that shells
out to `plz` runs a fresh invocation, so `plz -o plugin.rust.pleaserusttool:x`
applies to the outer one only and the inner build silently uses whatever the
config names. `PLZ_OVERRIDES` in the environment does carry through. Anything
that must hold across a nested invocation belongs in `.plzconfig`.

## What went wrong, and what it taught us

Findings that shaped the design. Kept because each one cost real time and none
of them is obvious from reading the code.

### A large graph breaks things a small one cannot

A 1,102-crate corpus found twenty bugs, and not one had appeared in this
repo's own ~190 crates. They group by the mechanism that hid them:

- **Two versions of one crate.** Externconfig filenames collided, so syn 2 and
  syn 3 both wrote `syn.externconfig` and one overwrote the other. Every input
  is staged flat, so this needs two live majors to appear at all.
- **Package name is not crate name.** A dep guessed the crate from its label,
  so rustls-webpki, md-5 and sha-1 resolved to nothing. rustc falls back to
  searching `-L` and only objects when a second version is reachable, which is
  why it stayed quiet.
- **Duality is contagious.** Sharing one artifact between the host and target
  unit is sound only if everything it links is shared too. `quote` matched in
  both units so it was shared, `proc-macro2` did not, and the shared `quote`
  embedded the wrong one. Needs a proc macro whose own dependencies differ per
  unit.
- **Feature resolution.** Optional deps did not enable their own feature; an
  explicit feature named after an optional dep was skipped; `[lib] name` was
  ignored.
- **What a compile stages.** Build scripts could not see files beside them,
  compiles guessed at what a crate reads, and shaderc-sys vendors ~5,000 files
  of glslang and SPIRV-Tools next to its build script, which is past the exec
  argument limit.

The lesson generalises past Rust: a test suite covers the mechanisms it can
reach, and scale is a mechanism.

### A remote worker stages only what an action names

Every remote-execution failure found by the labs pilot was one shape, an
action assuming the environment around it rather than naming what it needs:
subrepo configs carrying cross-repo labels, the tool default resolving into
the consuming repo, a second toolchain download, Please crashing on a subrepo
with no remote tree, rustc's driver library unstaged for every compile, and
copies into a nested output directory that does not exist yet.

Two further constraints came out of the same work and shape the toolchain
layout. A binary and the libraries beside it never separate, because rustc
resolves `librustc_driver` through `../lib` relative to itself, so
`toolchain_rustc` holds `bin` and `lib` together. And a non-empty directory
cannot be an entry point: Please validates entry points by membership in the
flattened action outputs, and a directory's own path is never a key there, so
a sysroot has to be a whole output rather than a path inside one. Locally both
mistakes are invisible, because the whole tree is already on disk.

### The standard library is not self-contained

Describing the sysroot as core, alloc and std alone leaves `HashMap::new()`
uninferable in every crate, because `std::collections::HashMap` wraps
hashbrown's. The stdlib's own dependency graph is read out of `rust-src`, from
the manifests and the vendored sources beside them, including optional deps
that a feature turns on, which is the only route by which hashbrown reaches
core.

Declaring it at all has one correct form, `sysroot_project`, and both obvious
alternatives fail silently. Listing core and std among the ordinary crates
leaves them crates that merely happen to be called core and std: lang items
attach only to the crate rust-analyzer believes *is* the sysroot, so `Sized`
is unsatisfied for `char` while every import around it resolves. Letting
rust-analyzer discover them runs `cargo metadata` over the stdlib sources with
a nightly-only flag against whatever cargo is on PATH.

### Please's namespaces are global

Subrepo names come from package path plus name with nothing identifying the
declaring repo, so a plugin's `third_party/rust/serde` and a consumer's are
one name. This plugin keeps its crates in `third_party/crates` for that
reason. Plugin names are a second global namespace, separate from the first,
so a subrepo declaring a plugin its consumer also declares aborts the parse.

Two upstream fixes came out of this work: thought-machine/please#3576, a
lost-wakeup TOCTOU on `packageWaits` that hangs builds when many packages
concurrently subinclude a plugin's build_defs, reproduced at 1 in 12 cold
builds and merged; and #3577, Please crashing on a subrepo with no remote
tree.

### Measure with the matching version

rust-analyzer 0.3.2264 reported zero diagnostics against a 1.97.1 toolchain
because it could not load the sysroot at all, and said so about its
proc-macro server. That was recorded as clean once. The version-matched
analyzer ships in the dist tarball. The current baseline, against which a
regression would show, is 107,105 expressions inferred with 60 unknown, 0
panics and 0 type mismatches over this repo.

## Status / known gaps

Audited against cargo's documented build-script and feature contracts and
against go-rules' architecture. What remains open, deliberately:

- **C compiler comes from the cc plugin's configuration**: build scripts
  (`cc` crate) and rustc's linker use the toolchain the cc plugin is
  configured with, the host cc by default, following cc-rules' and
  go-rules' convention. The rust plugin's optional `CCTool` knob (the
  go-rules `CC_TOOL` pattern) accepts a build label, so a downloaded
  hermetic toolchain target can be swapped in without changing any rules;
  such a toolchain is not shipped here. C sources are staged and compile
  (blake3 builds its real asm implementations).
- **Version selection is a PubGrub solve.** Each (crate, compatibility
  bucket) is a package, so incompatible majors coexist as cargo allows, and
  a requirement spanning buckets becomes a proxy package whose versions are
  the candidate buckets, making that choice backtrackable too. Declared
  versions are preferences, not requirements, so adding one crate does not
  churn the rest. Releases needing a newer rustc than the declared toolchain
  are filtered out.
- **Resolution is per-platform, declarations are not**: a resolve is for one
  triple, derived from what is being built for (`--target` overrides it), so
  a mac developer resolves the darwin graph rather than a checked-in linux
  one. Declarations are shared by everyone in the repo, so `lock` and
  `sync --prune` cover every triple in `--targets` and write the union: a
  linux run still declares the crates a darwin build reaches.
- **Cross-compilation**: `--arch` picks the target platform, `rust_toolchain`
  puts that platform's `rust-std` in the one sysroot, and compiles name it
  with `--target`. Build scripts, proc macros and installed binaries stay on
  the host, matching cargo's split of its unit graph.
- **Not enforced**: `rust-version` (MSRV) checks; `CARGO_CFG_TARGET_FEATURE`
  is approximated for x86_64/aarch64; dev-dependencies are ignored (nothing
  builds third-party crates' own tests).
