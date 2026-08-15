# Rust Rules

Hermetic Rust build rules for the [Please](https://please.build) build system —
the `go_repo` experience, for Rust.

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

- Build scripts that invoke a C compiler (`cc` crate, `-sys` crates) need a
  host cc; a declared hermetic C toolchain is future work (blake3 is built
  with its `pure` feature for this reason). go-rules has the same frontier
  with cgo.
- `rust_proto` and the protoc/prost/tonic codegen toolchain are pending a
  port to `rust_repo` (the tonic runtime itself builds). The proto preload is
  disabled in `.plzconfig` until then.
- `lock` uses greedy max-satisfying resolution with clear conflict errors;
  a backtracking (PubGrub) solver can replace its `select()` seam.
- Resolution currently targets one platform (`x86_64-unknown-linux-gnu` by
  default; `--target` on resolve/sync/lock).
