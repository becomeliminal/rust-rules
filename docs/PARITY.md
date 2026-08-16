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
- [ ] `links` / `DEP_<LINKS>_<KEY>` propagation to dependents' build scripts
      (parsed today, not wired)
- [ ] Rust → C dependency edges: `rust_library` deps on the cc plugin's
      `cc_library`, linked correctly
- [ ] C → Rust dependency edges: cc targets consuming `staticlib`/`cdylib`
      outputs with generated headers (cbindgen)
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
- [ ] rmeta pipelining: dependents' metadata compiles start against `.rmeta`
      before dependency codegen finishes (cargo and rules_rust both do this;
      needs a split metadata-only action — investigate cost model in plz)
- [ ] Opt-in non-hermetic incremental dev mode (the rules_rust
      `experimental_incremental_base` equivalent: persistent rustc
      incremental dir, local dev only, never CI)

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
- [ ] Differential testing: cargo resolution as a dev-time oracle over a
      manifest corpus; every divergence is a bug or a documented decision
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
- [ ] Top-100-crates corpus package in CI; pass-rate is the public metric
- [ ] Per-crate escape hatches: patches (arg exists, unexercised), source
      overrides (done via download=), env injection for build scripts

### Explicit non-goals (decided, revisit only on demand)
- rust-analyzer / `rust-project.json` generation — parked; emacs- and
  agent-driven development here
- Third-party crates' own test suites
- `cargo publish`
- Bit-for-bit rustc incremental parity in hermetic mode
