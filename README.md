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

Or import an existing Cargo project wholesale from its lockfile:
```ini
plz run //tools/please_rust -- sync --import path/to/Cargo.lock
```

Both maintain the `rust_repo` declarations in `third_party/rust/BUILD` for
you, including sha256 hashes so every download is verified. After editing
declarations by hand, run `plz run //tools/please_rust -- sync` to re-resolve.

To compile a binary, you can use `rust_binary`:
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

## Configuration
Plugins are configured through the Plugin section like so:
```ini
[Plugin "rust"]
SomeConfig = some-value
```
The available configuration options are:

### Rustc
The path to the `rustc` compiler to use. Defaults to the toolchain's rustc.
```ini
[Plugin "rust"]
Rustc = //third_party/rust:toolchain_rustc
```

### StdLib
The path to the `stdlib` to be linked by the compiler. Defaults to the toolchain's stdlib.
```ini
[Plugin "rust"]
StdLib = //third_party/rust:toolchain_stdlib
```

## General notes
Rust Rules replicates Cargo's build contract without ever invoking Cargo:
crate tarballs are fetched as verified downloads, `Cargo.toml` files are
parsed to infer dependencies, features, editions and build scripts, and
resolution happens deterministically inside the build graph. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how this works internally.

## Contributing
Contributions are welcome! Please open or submit a pull request with your changes. Ensure that your code follows the existing style and includes tests where applicable.

### Extra Features for Contribution
Here are some extra features that would be valuable additions to this project:

- **Crate Types**: `lib`, `rlib`, `proc-macro` and `bin` crates are supported. Adding support for `staticlib`, `dylib` and `cdylib` would be useful.

- **C toolchain**: Build scripts that invoke a C compiler (the `cc` crate, `-sys` crates) currently rely on the host compiler. A declared, hermetic C toolchain would close this gap.

- **Target (OS and Architecture) Compatibility**: This project has primarily been tested on unknown-linux-gnu x86_64 architecture. It would be nice to test and support other targets to ensure cross-platform compatibility.
