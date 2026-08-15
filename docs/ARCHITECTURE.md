# Architecture

Hermetic Rust build rules for Please — the `go_repo` experience, for Rust.
This document covers the internals; see the top-level README for usage.

Third-party crates are declared once, by name and version. Everything else —
transitive dependency routing, feature unification, BUILD file generation,
compilation — is computed by `please_rust`, a single tool that reimplements
cargo's build contract offline. **Cargo is never invoked**: not at build time,
not at resolution time, not to add a dependency.

```python
# third_party/rust/BUILD — maintained by `please_rust sync`
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
| `rust_toolchain` | rustc/cargo/stdlib fetched from static.rust-lang.org, sha256-pinned | fetch rule only |
| `rust_repo` | crate tarball fetched from static.crates.io via `remote_file`, sha256-verified | fetch rule only |
| `please_rust generate` | parses the crate's Cargo.toml, emits BUILD rules into a Please subrepo | none |
| `please_rust resolve` | semver version routing + cargo resolver-v2 feature unification across the declared graph → checked-in `rust.lock` | none |
| `please_rust compile` / `build-script` | drives rustc with cargo's full env contract (`CARGO_PKG_*`, `OUT_DIR`, feature cfgs, proc-macro externs, build script directives) | none |
| `please_rust sync` | maintains the `rust_repo` list: naming, hashes, lock regeneration; imports a cargo `Cargo.lock` wholesale | none |
| `please_rust lock --add crate@req` | resolves new deps against the crates.io sparse index (cached, `--offline` supported), hashes from index checksums | dev-time only |

The `rust_repo` declarations play the role `go.mod` plays for go-rules: the
committed, deterministic resolution artifact. `rust.lock` (checked in next to
the BUILD file) carries the computed feature sets and dependency routing, so
builds are reproducible byte-for-byte with no resolver in the loop.

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

# After hand-editing rust_repo declarations, re-resolve:
plz run //tools/please_rust -- sync
```

All three rewrite `third_party/rust/BUILD` and regenerate `rust.lock`.
Feature requests live on the `rust_repo` entries you name; everything
transitive is derived.

## First-party rules

`rust_library`, `rust_binary`, `rust_test`, and `rust_benchmark` (criterion)
compile through the same `please_rust compile` driver and interoperate with
`rust_repo` crates in both directions. See `examples/`.

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

## Bootstrap

`//tools/please_rust:please_rust` is self-hosted: built by these rules from
the `rust_repo` graph. The `:bootstrap` genrule builds it once with cargo to
break the egg (exactly as go-rules bootstraps please_go); nothing else ever
runs cargo.

## Status / known gaps

Audited against cargo's documented build-script and feature contracts and
against go-rules' architecture. What remains open, deliberately:

- **Hermetic C toolchain**: build scripts that invoke a C compiler (`cc`
  crate, `-sys` crates) use the host cc found on PATH; a declared toolchain
  (CC/AR/CFLAGS into the build-script env — the C sources are already staged)
  is future work. blake3 is built with its `pure` feature for this reason.
  go-rules has the same frontier with cgo.
- **`links` / `DEP_<LINKS>_<KEY>` propagation**: metadata directives are
  parsed and `CARGO_MANIFEST_LINKS` is set, but a linked crate's metadata is
  not yet exported to its dependents' build scripts. This only matters for
  native-library ecosystems (libz-sys and friends), so it lands together
  with the C toolchain.
- **`lock` resolution is greedy** max-satisfying with pin preference and
  clear conflict errors; a backtracking (PubGrub) solver can replace its
  `select()` seam without touching anything else.
- **Single-platform resolution**: one target triple per resolve
  (`x86_64-unknown-linux-gnu` by default; `--target` on resolve/sync/lock).
  The host/target unit split is in place, so cross-compilation needs only a
  second triple threaded through, not a redesign.
- **Not enforced**: `rust-version` (MSRV) checks; `CARGO_CFG_TARGET_FEATURE`
  is approximated for x86_64/aarch64; dev-dependencies are ignored (nothing
  builds third-party crates' own tests).
