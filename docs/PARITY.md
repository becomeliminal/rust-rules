# Parity log

Tracks feature parity against the two reference implementations. rules_rust
(Bazel) parity is the active target; Cargo parity is the running log behind
it. Check items off as they land; add new gaps as they're found.

Where we are already ahead of both (not tracked, just noted): cargo-free
resolution in the build graph (rules_rust shells out to `cargo metadata`;
its repin flow is the pain we designed away), instant dep addition via
`lock --add`, one polyglot graph with the proto plugin, and a tool three
orders of magnitude smaller than crate_universe.

## Track 1: rules_rust parity (active)

### C / native interop

plz has no Bazel-style provider system, so interop is explicit metadata
plumbing: carry native-link info (lib paths, -l flags) through the graph the
way build-script rustc-link-lib directives already flow, and emit it at
binary-link time.
- [x] `links` / `DEP_<LINKS>_<KEY>` propagation to dependents' build scripts
      (lock carries links keys; buildscript files are self-describing;
      test/links proves the pair end to end)
- [x] Rust → C dependency edges: cc_deps on rust_library/binary/test links
      c/cc plugin archives (recorded in the rlib, bundled, flowing to final
      links; test/cc_interop). Note: use c_library for C — cc_library
      compiles as C++ and mangles symbols
- [x] C → Rust dependency edges: c_binary/cc targets link staticlib outputs
      directly (test/cc_interop:uses_rust); cbindgen header generation still
      open — tracked under the bindgen item
- [ ] Optional hermetic C toolchain target for CCTool (host cc remains the
      default, per the cc plugin convention)
- [ ] bindgen rule (needs libclang strategy)

### Platforms and configurations
- [ ] Per-platform toolchain URLs in `rust_toolchain` (darwin-aarch64,
      darwin-x86_64, linux-aarch64) + macOS CI validation (plz itself is
      cross-platform; this is toolchain plumbing)
- [ ] Cross-compilation: `--target` threading through resolve → generate →
      compile with per-target `rust-std`; map onto plz's cross-arch labels
- [ ] True exec/target artifact split (unit split exists in resolution;
      artifacts currently share one triple — fine while host == target)
- [ ] wasm32 targets (+ wasm-bindgen rule later)
- [ ] Nightly / channel toolchains policy

### Build speed
- [ ] rmeta pipelining via the rules_rust two-action scheme: a metadata-only
      compile per crate that dependents' compiles hang off, with full rlib
      codegen in parallel and only binary links waiting on rlibs. Cost:
      frontend runs twice; win: chains build at frontend-depth speed

### Tooling rules
- [ ] `rust_clippy` (clippy-preview is already in the dist tarball) + CI gate
- [ ] rustfmt exposure + format check rule
- [ ] `rust_doc` (rustdoc HTML output)
- [ ] Coverage: `-C instrument-coverage` wired into `plz cover` / llvm-cov

### Hardening
- [ ] Remote execution audit (absolute-path canonicalization, cwd walks)
- [ ] Scale test: 1k+ crate graph through sync/resolve/build
- [ ] Subrepo name namespacing (`subrepo_name()` prefix — pilot finding)

## Track 2: Cargo parity (log)

### Resolver
- [ ] Differential testing: cargo resolution as a dev-time oracle — pick
      real-world repos, resolve with cargo and with us, diff. Lives in a
      separate corpus repo (rust-rules-corpus), not here; run several repos
      and expand over time
- [ ] PubGrub backtracking at `lock`'s `select()` seam
- [ ] MSRV-aware resolution (`rust-version`, Cargo ≥1.84 behavior)
- [ ] Multi-platform lock entries (per-triple resolution outputs)

### Dependency management
- [ ] `sync --upgrade` (`cargo update` parity: bump all to latest compatible)
- [ ] `lock --add crate@req --features a,b` + auto-fetch of newly-activated
      optional deps (pilot finding: serde derive needed a manual matching add)
- [ ] Generic git fetcher (gitlab/self-hosted; github archive URLs work)
- [ ] Private / alternative registries (index + download URL templating)
- [ ] Registry auth tokens

### Build fidelity
- [ ] Profiles: opt-level 0–3/s/z, lto thin/fat, codegen-units,
      panic=abort, debug-assertions, overflow-checks, strip; per-dep
      overrides
- [ ] `cargo bench` profile semantics for rust_benchmark
- [ ] Build-script env completeness audit against cargo docs (rerun as cargo
      versions land; last audit 2026-08 caught error=, rustc-flags,
      CARGO_CRATE_NAME et al.)

### Ecosystem burn-down
- [ ] Top-100-crates corpus in the separate corpus repo, run in its CI;
      pass-rate is the public metric
- [ ] Per-crate escape hatches: patches (arg exists, unexercised), source
      overrides (done via download=), env injection for build scripts

### Explicit non-goals (decided, revisit only on demand)
- rust-analyzer / `rust-project.json` generation — parked; emacs- and
  agent-driven development here
- Third-party crates' own test suites
- `cargo publish`
- rustc incremental compilation, in any mode (decided 2026-08: crate
  splitting + plz caching is the model; cross-machine cached builds likely
  beat cargo's warm incremental in practice anyway — the benchmark harness
  will settle it)
