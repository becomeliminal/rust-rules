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
- [x] ~~Optional hermetic C toolchain target~~ — dropped by decision
      (2026-08): go-rules has shipped on host cc via CC_TOOL config
      forever; the CCTool knob exists for anyone who wants to point it at
      their own toolchain target
- [x] bindgen rule: `rust_bindgen` generates bindings from a C header.
      The bindgen binary is built from crates by rust_repo (bindgen-cli +
      clang-sys stack — the tool supply chain is in-graph, no prebuilt
      downloads); libclang comes from the host (LibclangPath knob for a
      pinned one), matching the host-cc convention. test/bindgen proves
      header → bindings → rust_test end to end

### Platforms and configurations
- [ ] Per-platform toolchain URLs in `rust_toolchain` (darwin-aarch64,
      darwin-x86_64, linux-aarch64) + macOS CI validation (plz itself is
      cross-platform; this is toolchain plumbing)
- [ ] Cross-compilation: `--target` threading through resolve → generate →
      compile with per-target `rust-std`; map onto plz's cross-arch labels
- [ ] True exec/target artifact split (unit split exists in resolution;
      artifacts currently share one triple — fine while host == target)
- [ ] wasm32 targets (+ wasm-bindgen rule later). Groundwork done: the
      bindgen-cli pattern (tool built from crates via rust_repo, aliased
      through a config knob) is exactly how wasm-bindgen-cli will ship;
      the blocker is --target threading through resolve/generate/compile
      with a wasm32 rust-std, tracked above
- [ ] Nightly / channel toolchains policy

### Build speed
- [x] rmeta pipelining via the rules_rust two-action scheme: each crate
      splits into a `_X#rmeta` metadata compile (the full command, cut off
      the moment rustc reports the rmeta artifact — a plain --emit=metadata
      rmeta lacks the MIR dependents need) that dependents' compiles hang
      off, a `_X#link` full compile in parallel, and a public filegroup
      that stages transitive rlibs for binary links. First-party routes
      via provides/requires ('rust_rmeta'); subrepos wire twins explicitly.
      Opt-in via PipelinedCompilation (on in this repo, off by default).
      Side effect: all compiles now pass --remap-path-prefix=$CWD= (twins
      must hash identically across sandboxes), so artifacts are
      path-independent — better remote cache portability for free

### Tooling rules
- [x] `rust_clippy` (clippy-driver from the dist, copied next to rustc for
      its librustc_driver RUNPATH; metadata-only compile through it, -D
      warnings by default so findings fail the build; ClippyTool knob)
- [x] rustfmt exposure (toolchain_rustfmt) + `rust_fmt_test` check rule
      (RustfmtTool knob)
- [x] `rust_doc` (rustdoc HTML via the test wrapper's --externs-from-cwd
      dep resolution)
- [x] Coverage: `-C instrument-coverage` wired into `plz cover` / llvm-cov
      (profraw → llvm-profdata/llvm-cov → per-file line coverage; paths
      remapped to repo-relative; consumers add `.rs` to `[cover]
      FileExtension`. Note: rust_test routes sources through a filegroup
      dep, not srcs — plz excludes test-target srcs from coverage reports)

### Hardening
- [x] Remote execution: verified against a real cluster by the labs pilot
      (1461 build tasks, 525 third-party crates, CI green). Six bugs fixed,
      all of one shape — an action assuming the environment around it rather
      than naming what it needs, because a worker stages only what is named:
      subrepo configs carrying cross-repo labels, the tool default resolving
      into the consuming repo, a second toolchain download, Please crashing
      on a subrepo with no remote tree (mitigated by building crate subrepos
      locally, fixed upstream in thought-machine/please#3577), rustc's
      driver library unstaged for every compile, and copies into a nested
      output directory that does not exist yet
- [ ] Three paths remain **expected but not demonstrated** under remote
      execution, because the reference consumer cannot reach them: the test
      wrapper's use of `$RESULTS_FILE` and its coverage-profile directory
      (its Rust targets live under an `experimentaldir`, excluded from its
      CI's affected-target queries, so no Rust test has ever run remotely),
      rustdoc staging its driver library (no doc targets there), and the
      coverage tools staging libLLVM (no coverage step). Reasoning says they
      are fine and an audit of the wrapper found nothing; an audit also
      missed the compile-action instance before v0.4.5, so treat these as
      unverified. Pointing a remote executor at them, in that order, closes it
- [ ] Upstream plz WaitForPackage race (found 2026-08): a lost-wakeup
      TOCTOU on `packageWaits` in src/core/state.go hangs builds when many
      packages concurrently subinclude the plugin's build_defs — all plz
      versions incl. master. Reproduced (1/12 cold builds on a 40-package
      consumer), root-caused, three-line AddOrGet fix written and validated
      (0/16 with a patched plz). Our own repo dodges it via
      preloadbuilddefs. Fix + regression test submitted upstream as
      thought-machine/please#3576; until it lands in a release, large
      plugin consumers can rarely hit a wedged parse
- [ ] Remote execution audit (absolute-path canonicalization, cwd walks)
- [ ] Scale test: 1k+ crate graph through sync/resolve/build
- [x] Subrepo name collision, reproduced and fixed (2026-08). plz derives
      subrepo names from package path + name (`filepath.Join(pkg.Name,
      name)` in its subrepo builtin) with no qualification by the declaring
      repo, so a plugin's `third_party/rust/itoa` and a consumer's are one
      global name. This plugin now keeps its own crates in
      `third_party/crates`, and `rust_repo` derives both `third_party_path`
      and `lock` from the package the declarations live in, so relocating
      them is all it takes. A consumer building its own `itoa` alongside the
      plugin's tool target is now a regression test. go-rules shares the
      naming but is never parsed by consumers, so it has not had to solve it
- [x] Shipped config must not carry dev assumptions: plugin_repo ships this
      repo's .plzconfig, so `[Parse]` preloads became requirements on every
      consumer that parsed a plugin package (proto, shell and python plugins
      declared). Preloads removed and our own BUILD files subinclude
      explicitly, matching go-rules, whose config has no [Parse] section

## Track 2: Cargo parity (log)

### Resolver
- [ ] Differential testing: cargo resolution as a dev-time oracle — pick
      real-world repos, resolve with cargo and with us, diff. Lives in a
      separate corpus repo (rust-rules-corpus), not here; run several repos
      and expand over time
- [x] PubGrub backtracking, via the pubgrub crate (proven at scale by uv).
      Each (crate, compatibility bucket) is a package, so incompatible
      majors coexist as cargo allows; a requirement spanning several buckets
      becomes a proxy package whose versions are the candidate buckets, so
      the bucket choice is itself backtrackable. Declared versions are pins
      (preferences), not requirements, so an unrelated `--add` never churns
      the graph and no-op adds stay offline-safe; same-bucket changes upgrade
      a declaration in place instead of duplicating it. `--greedy` keeps the
      old walk. Neither go-rules (Go's MVS needs no backtracking) nor
      rules_rust (delegates to cargo) has an equivalent, and cargo itself
      uses a bespoke solver
- [x] MSRV-aware resolution: releases requiring a newer rustc than the
      declared `rust_toolchain` are filtered out (cargo >=1.84 semantics),
      relaxing with a warning if a package would otherwise be unsolvable.
      `--ignore-msrv` opts out. Verified end to end: clap resolves to the
      4.5 line on rustc 1.97 and to 4.0.32 with the older dependency stack
      on rustc 1.63
- [ ] Multi-platform lock entries (per-triple resolution outputs)

### Dependency management
- [ ] `sync --upgrade` (`cargo update` parity: bump all to latest compatible)
- [x] `lock --add crate@req --features a,b` + auto-fetch of newly-activated
      optional deps: resolution reports what it needed but could not find,
      and lock re-solves to declare it (bounded loop). No more manual
      matching-version adds
- [ ] Generic git fetcher (gitlab/self-hosted; github archive URLs work)
- On-demand only (decided 2026-08): private/alternative registries and
  registry auth. crates.io is effectively universal; git forks and
  download= overrides cover the common private-code cases

### Build fidelity
- [x] Profiles: opt-level, lto, codegen-units, panic, strip and
      debug-assertions via plugin config, mapped onto plz's build configs.
      Per-dep overrides remain open (rarely used; cargo's own
      `[profile.*.package]`)
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
