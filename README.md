# Rust Rules
This repo provides Rust build rules for the [Please](https://please.build) build system.

## Basic usage
First add the plugin to your project. In `plugins/BUILD`:
```python
plugin_repo(
    name = "rust",
    owner = "becomeliminal",
    revision = "<Some git tag, commit, or other reference>",
)
```

Set up the Rust toolchain for your project in `third_party/rust`:
```python
subinclude("///rust//build_defs:rust")

rust_toolchain(
    name = "toolchain",
    hashes = ["<hash>"],
    version = "X.XX.X",
    visibility = ["PUBLIC"],
)
```

The toolchain is downloaded for the machine doing the building, so a repo
worked on from both linux and macOS lists a hash per platform and plz
accepts whichever matches.

Then add the plugin config to `.plzconfig`:
```ini
[Plugin "rust"]
Target = //plugins:rust
```

You can then compile and test Rust libraries like so:
```python
subinclude("///rust//build_defs:rust")

rust_library(
    name = "lib",
    root = "src/lib.rs",
    modules = [
        "src/module_a.rs",
        "src/module_a/sub_module_a.rs",
        "src/module_b.rs",
        "src/module_b/sub_module_b.rs",
    ],
)

rust_test(
    name = "lib",
    root = "src/lib.rs",
    modules = [
        "src/module_a.rs",
        "src/module_a/sub_module_a.rs",
        "src/module_b.rs",
        "src/module_b/sub_module_b.rs",
    ],
)
```
Tests report individual results to Please (not just pass/fail). Integration
tests are `rust_test` rules rooted at a file under `tests/`, depending on the
library; documentation tests run with `rust_doc_test`:
```python
rust_test(
    name = "integration_test",
    root = "tests/integration_test.rs",
    deps = [":lib"],
)

rust_doc_test(
    name = "doc_test",
    crate_name = "lib",
    root = "src/lib.rs",
    deps = [":lib"],
)
```

You can define third-party crates using `rust_repo`. Only your direct
dependencies need declaring — versions, features and transitive dependencies
are resolved from each crate's `Cargo.toml`, the same way Cargo would:
```python
subinclude("///rust//build_defs:rust")

rust_repo(
    name = "serde",
    crate = "serde",
    version = "1.0.228",
    features = ["derive"],
)

rust_repo(
    name = "rand",
    crate = "rand",
    version = "0.8.5",
)
```

To add a dependency (and everything it needs) straight from crates.io:
```ini
plz run //tools/please_rust -- lock --add serde@1
```
Enable features, and anything they turn on is declared for you:
```ini
plz run //tools/please_rust -- lock --add serde@1 --features derive
```

Version selection is a PubGrub solve over the crates.io index: it backtracks
rather than failing when a late requirement rules out an earlier choice, and
it respects `rust-version`, so an older `rust_toolchain` gets the newest
releases that actually support it. Already-declared versions are preferred,
so adding one crate does not churn the rest of the graph. `--ignore-msrv`
turns MSRV filtering off; `--greedy` selects the older non-backtracking
resolver.

Declarations are shared by everyone working in the repo, so `lock` solves
for every platform in `--targets` (linux x86_64 and both darwin arches by
default) and declares the union. A linux developer adding `chrono` declares
`core-foundation-sys` too, because `iana-time-zone` needs it on macOS, and
their colleague on a Mac can build the branch. Resolution itself is still
per-platform and happens in the build graph, so each machine builds only the
crates it actually needs.


Or import an existing Cargo project wholesale from its lockfile:
```ini
plz run //tools/please_rust -- sync --import path/to/Cargo.lock
```

To port a whole cargo workspace, point the importer at it:
```ini
plz run //tools/please_rust -- sync --import-workspace path/to/workspace
```
This writes a BUILD file next to every member (`rust_library`,
`rust_binary`, `rust_test` for unit and integration tests, with path
dependencies mapped to member labels), scaffolds `third_party/rust/BUILD`,
`.plzconfig` and `plugins/BUILD` on a fresh repo, and imports the
workspace's `Cargo.lock` for the third-party graph. Existing BUILD files
are never overwritten; build scripts, optional features and renames are
reported for manual follow-up.

Both maintain the `rust_repo` declarations in `third_party/rust/BUILD` for
you, including sha256 hashes so every download is verified. After editing
declarations by hand, run `plz run //tools/please_rust -- sync` to re-resolve.

To use a fork or an unpublished revision, fetch the crate from a git forge
at a pinned revision instead of crates.io:
```python
rust_repo(
    name = "anyhow",
    crate = "anyhow",
    version = "1.0.86",
    git_repo = "dtolnay/anyhow",
    git_revision = "1.0.86",
)
```
(`sync --import` translates `git+https://github.com/...` lockfile sources
automatically.)

`rust_library` builds an `rlib` by default; `crate_type` also supports
`proc-macro`, `dylib`, `cdylib` and `staticlib` for compiler plugins and
C-ABI artifacts.

To compile a binary, you can use `rust_binary`. Binaries statically link the
C runtime by default (like Go), producing self-contained executables; opt out
per rule with `static = False` or globally with the `DefaultStatic` config:
```python
subinclude("///rust//build_defs:rust")

rust_binary(
    name = "bin",
    main = "src/main.rs",
    deps = [
        ":lib",
        "//third_party/rust:<rust_repo_name>",
    ],
)
```

To benchmark your code with [Criterion](https://crates.io/crates/criterion), you can use `rust_benchmark`:
```python
subinclude("///rust//build_defs:rust")

rust_benchmark(
    name = "your_benchmark",
    main = "src/main.rs",
    deps = [
        "//your/lib/to/benchmark",
    ],
)
```

You can use criterion directly in your `src/main.rs`:
```rust
use criterion::{criterion_group, criterion_main, Criterion, measurement::WallTime};
use fibonacci::{fibonacci};

fn benchmark_fibonacci(c: &mut Criterion<WallTime>) {
    c.bench_function("fibonacci 20", |b| b.iter(|| fibonacci(20)));
}

criterion_group!(
    name = benches;
    config = Criterion::default().with_measurement(WallTime);
    targets = benchmark_fibonacci
);
criterion_main!(benches);
```

And run the benchmark with Please:
```ini
plz run //path/to/your_benchmark -- --bench
```

FFI bindings come from `rust_bindgen` — the bindgen binary is built from
crates by `rust_repo` (declare `bindgen-cli` via `lock --add`), and libclang
comes from the host like the C compiler does (`LibclangPath` pins one):
```python
rust_bindgen(
    name = "ffi_bindings",       # generates ffi_bindings.rs
    header = "include/mylib.h",
)

rust_library(
    name = "mylib_sys",
    root = "src/lib.rs",
    modules = [":ffi_bindings"], # or use it as the root directly
)
```

Protobuf and gRPC codegen live in
[rust-proto-rules](https://github.com/becomeliminal/rust-proto-rules), a
separate plugin that pins these rules by tag and plugs into the proto
plugin's language definitions.

Clippy, rustfmt and rustdoc ship in the toolchain, with a rule each:
```python
rust_clippy(
    name = "lint",           # plz build //pkg:lint — any clippy finding fails
    root = "src/lib.rs",
    modules = ["src/util.rs"],
    deps = [":lib_deps"],
)

rust_fmt_test(
    name = "fmt_test",       # plz test //pkg:fmt_test — fails on unformatted code
    srcs = glob(["src/*.rs"]),
)

rust_doc(
    name = "docs",           # plz build //pkg:docs — rustdoc HTML output
    root = "src/lib.rs",
    deps = [":lib_deps"],
)
```

## Configuration
Plugins are configured through the Plugin section like so:
```ini
[Plugin "rust"]
SomeConfig = some-value
```
The available configuration options are:

`rust_toolchain` is one target, and each tool it provides is an entry point
of it:

```ini
[Plugin "rust"]
Rustc   = //third_party/rust:toolchain|rustc
StdLib  = //third_party/rust:toolchain|stdlib
Sysroot = //third_party/rust:toolchain|sysroot
```

`|rustdoc`, `|cargo`, `|clippy`, `|rustfmt`, `|llvm-tools`, `|llvm-cov` and
`|llvm-profdata` are available the same way, and are derived from `Rustc`
unless you set them, so most repos configure nothing beyond the three above.

Keeping the toolchain in one output is deliberate rather than tidy: rustc,
rustdoc and the llvm tools each load a library from beside their own binary,
and exposing them as separate targets let a build stage a binary without it.
That works locally, where the whole toolchain is present, and fails on a
remote worker, which stages only what an action names.

### Cross-compilation
`plz build --arch darwin_arm64 //...` compiles for another platform.
`rust_toolchain` installs the standard library for whatever `--arch` names,
and `architectures` installs more for cross-compiling part of a repo by
hand:

```python
rust_toolchain(
    name = "toolchain",
    version = "1.97.1",
    architectures = ["darwin_arm64"],
)
```

Build scripts, proc macros and installed binaries are compiled for the host
whatever the target is, since they run during the build; cargo splits its
unit graph the same way. Libraries need nothing further. Linking an
executable for another platform also needs a linker that targets it, which
comes from `CCTool`.

### DefaultStatic
Binaries statically link the C runtime by default, producing self-contained
executables like Go's. Set to false to default to dynamic linking; either
way `static = True/False` on a `rust_binary` overrides per rule.
```ini
[Plugin "rust"]
DefaultStatic = false
```

### CCTool
Optional build label or path of a C compiler, used by crate build scripts
and as rustc's linker. Empty uses the host cc via PATH, matching the cc
plugin's default.
```ini
[Plugin "rust"]
CCTool = //third_party/cc:toolchain
```

### PleaseRustTool
The `please_rust` binary. The default is a hash-pinned download of a
released one, per platform, the way go-rules ships `please_go`: nothing to
build, no toolchain, no cargo, and it works under remote execution. A
platform with no published binary yet falls back to building it from
source, which is a cargo build needing network access — correct, but slow
enough that you will notice.

Point it at your own build or your own pin if you would rather not depend
on the release:

```python
# third_party/rust/BUILD
remote_file(
    name = "please_rust_tool",
    url = "https://github.com/becomeliminal/rust-rules/releases/download/<tag>/please_rust-linux_amd64",
    hashes = ["<sha256 from the release>"],
    binary = True,
    visibility = ["PUBLIC"],
)
```
```ini
[Plugin "rust"]
PleaseRustTool = //third_party/rust:please_rust_tool
```

Note that a parse-only command such as `plz query` still has to build the
crate subrepos it touches, and building those needs this tool — so the
choice affects far more than a full build.

### Profiles
Cargo's profile settings, mapped onto Please's build configs. The tuning
knobs apply to optimised builds (`plz build -c opt`); `DebugAssertions`
applies to both.
```ini
[Plugin "rust"]
OptLevel = 3          ; 0-3, s, z
LTO = thin            ; thin, fat, off
CodegenUnits = 1
Panic = abort         ; unwind, abort
Strip = symbols       ; none, debuginfo, symbols
DebugAssertions = false
```

### PipelinedCompilation
Splits each library crate into a metadata-only compile that dependents'
compiles hang off and a full compile that runs in parallel (the scheme
cargo and rules_rust use). Dependency chains build at frontend depth
instead of full-compile depth; the cost is that the compiler frontend runs
twice per crate. Off by default.
```ini
[Plugin "rust"]
PipelinedCompilation = true
```

### Coverage
`plz cover` works out of the box: tests are compiled with
`-C instrument-coverage` and the profiles are converted per-file line
coverage via the toolchain's bundled llvm tools (the `LlvmTools` option
overrides where those come from). The one thing a consuming repo must add is
`.rs` to Please's coverage extension list, which doesn't include it by
default:
```ini
[cover]
FileExtension = .rs
```

## General notes
Measured against cargo building the identical project with the identical
rustc, plz is ~1.7x faster on cold builds and caches test results cargo
always re-runs; cargo keeps a decisive edge on single-crate edit loops.
The full numbers, methodology and honest caveats are in
[docs/BENCHMARKS.md](docs/BENCHMARKS.md), reproducible via
`scripts/benchmark.sh`.

Hit a confusing failure? [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)
lists the real messages this plugin produces, what causes each, and what to
do about it.

Rust Rules replicates Cargo's build contract without ever invoking Cargo:
crate tarballs are fetched as verified downloads, `Cargo.toml` files are
parsed to infer dependencies, features, editions and build scripts, and
resolution happens deterministically inside the build graph. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how this works internally.

## Contributing
Contributions are welcome! Please open or submit a pull request with your changes. Ensure that your code follows the existing style and includes tests where applicable.

### Extra Features for Contribution
Here are some extra features that would be valuable additions to this project:

- **Target (OS and Architecture) Compatibility**: built and tested in CI on
  x86_64-unknown-linux-gnu and aarch64-apple-darwin, and cross-compiled from
  the former to the latter. Intel Macs, ARM linux and Windows have nobody
  running them; ARM linux is covered by resolution but not by a build.

C toolchains stay host-provided by convention (the `CCTool` config accepts
a build label for anyone who wants their own), and private registries are
on-demand: crates.io plus git forks and `download=` overrides cover the
common cases.
