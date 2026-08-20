# Comparison

rust-rules on Please, set against Bazel's rules_rust and against Cargo, feature
by feature. Support is recorded as it stands, not as it is intended.

rust-rules is recorded from this repository at v0.7.1. Cargo is recorded from
its documented behaviour. **rules_rust is recorded from its documented design
and rule set rather than from runs**, which is the one column here that has not
been measured. [#30](https://github.com/becomeliminal/rust-rules/issues/30)
replaces it with a measured pass rate over the same crates.

`yes` supported, `partial` conditional or incomplete, `no` not supported.
Where a row is partial or no for rust-rules, the issue that would close it is
linked in **The open items** below.


## Where this wins

Resolution happens inside the build graph with no Cargo anywhere, so adding a
dependency is one command and there is no lockfile to drift. Rust sits in one
graph beside Go, Python, C and protobuf. Unchanged tests cost nothing, and one
machine's build helps another's.

Measured against Cargo building the identical project with the identical rustc
on 20 August 2026: cold build 6.65s against 9.27s, and on a 40-crate workspace,
editing a leaf and running every test takes 0.31s against 1.22s.

## Where Cargo wins

The inner loop, and it keeps it. A one-file edit rebuilds in 0.52s under Cargo
and 5.36s here, because rustc's incremental cache is state carried between
builds and that is the thing hermeticity gives up. Crate splitting is idiomatic
in a monorepo and `PipelinedCompilation` builds dependency chains at frontend
depth, but the gap is structural rather than a bug.

Cargo is also what every Rust developer knows, what every crate is published
for, and what every piece of documentation assumes. A single-crate project
should use it.

## The open items

Every `partial` and `no` in the tables below has an issue, grouped by kind
rather than by release:

- **Believed to work, never demonstrated:**
  [#14](https://github.com/becomeliminal/rust-rules/issues/14) remote execution paths,
  [#15](https://github.com/becomeliminal/rust-rules/issues/15) remote execution audit,
  [#16](https://github.com/becomeliminal/rust-rules/issues/16) musl and embedded,
  [#17](https://github.com/becomeliminal/rust-rules/issues/17) the patches argument
- **Missing capability:**
  [#18](https://github.com/becomeliminal/rust-rules/issues/18) editor refresh,
  [#21](https://github.com/becomeliminal/rust-rules/issues/21) git forges,
  [#22](https://github.com/becomeliminal/rust-rules/issues/22) sync --upgrade,
  [#23](https://github.com/becomeliminal/rust-rules/issues/23) cross-compiling C,
  [#24](https://github.com/becomeliminal/rust-rules/issues/24) channels,
  [#25](https://github.com/becomeliminal/rust-rules/issues/25) cbindgen,
  [#26](https://github.com/becomeliminal/rust-rules/issues/26) wasm-bindgen,
  [#27](https://github.com/becomeliminal/rust-rules/issues/27) multi-platform locks,
  [#28](https://github.com/becomeliminal/rust-rules/issues/28) bench profile,
  [#29](https://github.com/becomeliminal/rust-rules/issues/29) per-package profiles
- **Editor gaps inside a shipped feature:**
  [#19](https://github.com/becomeliminal/rust-rules/issues/19) generated sources,
  [#20](https://github.com/becomeliminal/rust-rules/issues/20) one lock per project
- **Evidence:**
  [#30](https://github.com/becomeliminal/rust-rules/issues/30) four-way corpus,
  [#31](https://github.com/becomeliminal/rust-rules/issues/31) corpus in CI,
  [#32](https://github.com/becomeliminal/rust-rules/issues/32) differential testing,
  [#33](https://github.com/becomeliminal/rust-rules/issues/33) build-script env audit
- **No consumer yet, so nothing has been built:**
  [#34](https://github.com/becomeliminal/rust-rules/issues/34) private registries,
  [#35](https://github.com/becomeliminal/rust-rules/issues/35) hermetic C toolchain,
  [#36](https://github.com/becomeliminal/rust-rules/issues/36) Windows,
  [#37](https://github.com/becomeliminal/rust-rules/issues/37) third-party test suites,
  [#38](https://github.com/becomeliminal/rust-rules/issues/38) cargo publish,
  [#39](https://github.com/becomeliminal/rust-rules/issues/39) rustc incremental


## Dependency resolution

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Resolve without invoking Cargo**<br>Who computes versions and features | **yes**. PubGrub solve inside the build graph, offline | **no**. crate_universe shells out to cargo | **yes**. It is cargo |
| **Feature unification (resolver v2)**<br>One crate reached by many paths | **yes**. Reimplemented, differential-tested against cargo | **yes**. Delegated to cargo | **yes**. Native |
| **Backtracking solver**<br>Recovers when a late requirement rules out an earlier pick | **yes**. PubGrub, incompatible majors as separate packages | **no**. Whatever cargo does | **yes**. Bespoke solver |
| **MSRV-aware selection**<br>Skip releases needing a newer rustc | **yes**. cargo >=1.84 semantics, --ignore-msrv opts out | **no**. Only if the vendored cargo does it | **yes**. Since 1.84 |
| **Add a dependency in one command**<br>No repin step | **yes**. lock --add crate@req, re-solves and declares | **partial**. cargo add then a repin of crate_universe | **yes**. cargo add |
| **Import an existing Cargo.lock**<br>Adopting a repo that already uses cargo | **yes**. sync --import, and --import-workspace for BUILD files too | **yes**. crate_universe consumes Cargo.toml directly | **yes**. Native |
| **Git and fork dependencies**<br>Pinned revision instead of crates.io | **partial**. github archive URLs; other forges need download= | **yes**. Supported | **yes**. Native |
| **Private or alternative registries**<br>Registry auth | **no**. On demand only; forks and download= cover the cases | **yes**. Via cargo | **yes**. Native |
| **Vendored source overrides**<br>Patch or replace a crate's source | **partial**. download= overrides; patches arg exists, unexercised | **yes**. annotations and patches | **yes**. [patch] and [replace] |

## The Cargo build contract

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Build scripts**<br>build.rs with the full environment | **yes**. CARGO_PKG_*, OUT_DIR, cfgs, directives, links metadata | **yes**. cargo_build_script | **yes**. Native |
| **links and DEP_&lt;LINKS&gt;_&lt;KEY&gt;**<br>Metadata a -sys crate publishes to dependents | **yes**. Carried in the lock, proved end to end in test/links | **yes**. Supported | **yes**. Native |
| **Proc macros**<br>Compiled for the host while the rest is not | **yes**. Host and target units split as cargo splits them | **yes**. Supported | **yes**. Native |
| **Multiple versions of one crate**<br>Incompatible majors alive together | **yes**. Newest owns the plain name, others suffixed, -C metadata per version | **yes**. Supported | **yes**. Native |
| **Renamed dependencies**<br>package = "..." | **yes**. Renames name the declaration they mean | **yes**. Supported | **yes**. Native |
| **Platform-gated dependencies**<br>[target.'cfg(...)'.dependencies] | **yes**. Evaluated with cfg-expr per triple | **yes**. Supported | **yes**. Native |
| **Profile controls**<br>opt-level, lto, codegen-units, panic, strip | **yes**. Plugin config mapped onto build configs | **yes**. Supported | **yes**. Native, plus per-package overrides |
| **Per-package profile overrides**<br>[profile.*.package] | **no**. Open, rarely used | **partial**. Partial | **yes**. Native |
| **Build-script env audit**<br>Kept current with cargo's documented contract | **partial**. Audited 2026-08, rerun as cargo versions land | **yes**. Tracks cargo | **yes**. Definitional |

## First-party rules

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Library, binary, test**<br>The basic three | **yes**. rust_library, rust_binary, rust_test | **yes**. rust_library, rust_binary, rust_test | **yes**. Native |
| **All crate types**<br>rlib, proc-macro, dylib, cdylib, staticlib | **yes**. crate_type on rust_library | **yes**. Supported | **yes**. Native |
| **Integration and doc tests**<br>tests/ and doctests | **yes**. rust_test rooted under tests/, rust_doc_test | **yes**. Supported | **yes**. Native |
| **Per-test reporting**<br>Individual results, not just pass or fail | **yes**. libtest output parsed to JUnit | **yes**. Supported | **no**. Pass or fail per binary |
| **Test result caching**<br>An unchanged test costs nothing | **yes**. Cached by the build system | **yes**. Cached by Bazel | **no**. cargo test reruns every time |
| **Benchmarks**<br>Criterion | **yes**. rust_benchmark | **yes**. rust_binary with a bench harness | **yes**. cargo bench |
| **cargo bench profile semantics**<br>The bench profile specifically | **no**. Open | **partial**. Partial | **yes**. Native |

## Tooling

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Clippy**<br>Lints as a build target | **yes**. rust_clippy, findings fail the build | **yes**. Aspect based | **yes**. cargo clippy |
| **rustfmt check**<br>Formatting as a test | **yes**. rust_fmt_test | **yes**. Supported | **yes**. cargo fmt --check |
| **Documentation**<br>rustdoc HTML | **yes**. rust_doc | **yes**. rust_doc | **yes**. cargo doc |
| **Coverage**<br>Line coverage from instrumented tests | **yes**. -C instrument-coverage into plz cover | **yes**. Supported | **partial**. External tooling, llvm-cov |
| **C header bindings**<br>bindgen | **yes**. rust_bindgen, tool built from declared crates | **yes**. rust_bindgen | **partial**. build.rs calling bindgen |
| **Rust to C headers**<br>cbindgen | **no**. Open | **yes**. Supported | **partial**. build.rs calling cbindgen |
| **wasm-bindgen**<br>JS bindings | **no**. Groundwork only | **yes**. Supported | **partial**. External tool |
| **Publish to crates.io**<br>cargo publish | **no**. Deliberate non-goal | **no**. Not its job | **yes**. Native |

## Native and C interop

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Rust depends on C**<br>Linking C archives into Rust | **yes**. cc_deps through the c/cc plugin | **yes**. cc_library deps | **partial**. build.rs and the cc crate |
| **C depends on Rust**<br>Linking Rust staticlibs into C | **yes**. c_binary links staticlib outputs | **yes**. Supported | **no**. Manual |
| **Vendored C in -sys crates**<br>The crate's own C actually compiling | **yes**. The whole -sys ecosystem in the corpus builds | **yes**. Supported | **yes**. Native |
| **Hermetic C toolchain**<br>The C compiler pinned by the build system | **partial**. Host cc by default, matching the cc plugin whose own CCTool is `cc`. CCTool takes a build label to pin one | **partial**. Host cc by default via local_config_cc. Hermetic needs a registered toolchain such as toolchains_llvm | **no**. The cc crate uses whatever is on PATH |

## Platforms and cross-compilation

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Linux**<br>Built and tested | **yes**. x86_64, green in CI | **yes**. Supported | **yes**. Native |
| **macOS**<br>Built and tested | **yes**. aarch64 native in CI, darwin_arm64 binary published | **yes**. Supported | **yes**. Native |
| **Windows**<br>Built and tested | **no**. Not supported, nobody running it | **yes**. Supported | **yes**. Native |
| **Cross-compiled libraries**<br>Building an rlib for another platform | **yes**. --arch, verified by inspecting the emitted object | **yes**. Bazel platforms | **yes**. --target with the std installed |
| **Cross-compiled binaries**<br>Linking an executable for another platform | **partial**. Needs a cross linker via CCTool, as any C toolchain does | **yes**. Toolchain resolution | **partial**. Needs a cross linker |
| **Explicit target triples**<br>Triples an os/arch pair cannot name | **yes**. TargetTriple, for wasm32, musl, sbf-solana | **yes**. Bazel platform constraints | **yes**. --target |
| **wasm32**<br>Compiling to WebAssembly | **yes**. CI asserts the object is a WebAssembly module | **yes**. Supported | **yes**. Native |
| **musl and embedded**<br>no_std and static libc targets | **partial**. TargetTriple names them, untested here | **yes**. Supported | **yes**. Native |
| **Nightly and channel policy**<br>Choosing a toolchain channel | **no**. Open | **yes**. Supported | **yes**. rustup |
| **Host and target split**<br>Build scripts and proc macros stay on the host | **yes**. The same split cargo makes in its unit graph | **yes**. exec and target configurations | **yes**. Native |

## Build system properties

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **Hermetic by construction**<br>A build cannot reach what it did not declare | **yes**. Sandboxed actions, pinned toolchain, no cargo | **yes**. Bazel sandboxing | **no**. Ambient toolchain and network |
| **Remote execution**<br>Actions run on a worker fleet | **partial**. Verified on a real cluster; three paths undemonstrated | **yes**. Bazel's core strength | **no**. None |
| **Remote and shared caching**<br>One machine's build helps another's | **yes**. Content addressed, paths remapped so artifacts are portable | **yes**. Supported | **no**. sccache, external and partial |
| **Polyglot in one graph**<br>Rust beside Go, Python, protos, C | **yes**. One plz graph, protos through a companion plugin | **yes**. One Bazel graph | **no**. Rust only |
| **Metadata pipelining**<br>Dependents start before codegen finishes | **yes**. Two-action rmeta scheme, opt in | **yes**. Where the scheme originated | **yes**. Native pipelining |
| **Incremental within a crate**<br>rustc's own incremental cache | **no**. Deliberately not, it trades against hermeticity | **no**. The same trade | **yes**. Native and fast |

## Editor integration

| Feature | rust-rules | rules_rust | Cargo |
|---|---|---|---|
| **rust-analyzer support**<br>Code intelligence at all | **yes**. Answers workspace.discoverConfig live | **yes**. Generates rust-project.json on request | **yes**. Native, cargo driven |
| **Refreshes without being asked**<br>Adding a crate updates the editor | **partial**. Watches the root BUILD, nested needs a re-run | **no**. Re-run the generator | **yes**. Native |
| **Standard library go-to-definition**<br>Stepping into std | **yes**. sysroot_project carrying the stdlib's own dependency graph | **yes**. Supported | **yes**. Native |
| **Proc macro expansion in the editor**<br>Derives resolving | **yes**. Dylibs built and named for the analyzer | **yes**. Supported | **yes**. Native |
| **Crates in subrepos**<br>Code intelligence for pulled-in repos | **yes**. Swept and described, never checked on save | **partial**. Workspace dependent | **no**. Not applicable |
