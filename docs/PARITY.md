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
- [x] macOS: `plz build //...` and `plz test //...` are green natively on
      macos-latest in CI, and the release publishes a darwin_arm64 binary.
      Five bugs stood between here and there, each found by a build on a
      Mac rather than by reading: a toolchain hash that could only match
      linux, a tool target declared for no platform a Mac has, BUILD
      colliding with crates that ship a build/ directory on a
      case-insensitive filesystem, proc macros named .so where macOS
      produces .dylib, and build scripts told they were targeting linux.
      darwin_amd64 is not built: its macos-13 runners were dropped in
      December 2025 and the label now never starts. macos-15-intel would
      work until August 2027, when GitHub-hosted x86_64 macOS ends
- [x] Cross-compilation of libraries: `plz build --arch darwin_arm64`.
      `rust_toolchain` installs the `rust-std` for whatever `--arch` names
      (plus anything in `architectures`), and the triple threads through
      resolve → generate → compile. Verified from linux: first-party and
      third-party rlibs come out as Mach-O arm64 objects, and CI builds four
      third-party crates chosen for the mechanism - two reached only as a
      build dependency, two only through a proc macro.

      Third-party cross-compilation did not actually work until 2026-08-19,
      despite being recorded here as done. A build script runs on the host,
      so its own dependencies must be host-built, and nothing said which
      crates those were; rustc reported `E0461: couldn't find crate
      version_check with expected target triple x86_64-unknown-linux-gnu` on
      the first one it reached. Both bugs were invisible natively, because
      the host triple is the target triple and any artifact serves either
      unit. That is the argument for the CI step rather than the claim
- [ ] Cross-compiling anything that links or compiles C. `plz build --arch
      darwin_arm64 //...` still fails on four targets here and all four are
      this: three are binaries, which need a macOS SDK and a cross linker to
      link, and blake3's build script compiles NEON C for arm64 with the
      host `cc`. Both come from `CCTool`, like every other C toolchain here,
      and neither is the Rust-level split above. Libraries are unaffected
- [x] True exec/target artifact split: build scripts, proc macros and
      installed binaries compile for the host, libraries for the target,
      the same split cargo makes in its unit graph
- [x] wasm32 targets. `TargetTriple = wasm32-unknown-unknown` compiles for
      it; CI asserts the emitted object is a WebAssembly module rather than
      trusting the flag. Please's os_arch pair cannot name wasm32 - it is
      neither an OS nor an architecture Please knows - so `--arch` will never
      reach it and the triple has to be named verbatim. `rust_toolchain`
      fetches the standard library for a TargetTriple the same way it does
      for an `--arch` platform: only when that is what you are building for,
      because wasm32's std is 93M against the host's 162M and the sysroot is
      staged by every compile
- [ ] wasm-bindgen rule. Same shape as the shipped `rust_bindgen` - a tool
      built from crates via rust_repo, aliased through a config knob - so it
      is a rule around a binary rather than new mechanism
- [ ] musl and embedded targets. `TargetTriple` names them and
      `rust_toolchain` will fetch their standard library, so what is untested
      is whether anything else assumes a hosted platform. `no_std` crates in
      particular have never been built here
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
      unverified. Pointing a remote executor at them, in that order, closes
      it; asked of the labs pilot on 2026-08-17. The entry-point toolchain
      makes the rustdoc and llvm cases structurally the same as the compile
      path that was verified remotely, which is an argument, not a run
- [x] Upstream plz WaitForPackage race (found 2026-08): a lost-wakeup
      TOCTOU on `packageWaits` in src/core/state.go hangs builds when many
      packages concurrently subinclude the plugin's build_defs — all plz
      versions incl. master. Reproduced (1/12 cold builds on a 40-package
      consumer), root-caused, three-line AddOrGet fix written and validated
      (0/16 with a patched plz). Our own repo dodges it via
      preloadbuilddefs. Fix + regression test submitted upstream as
      thought-machine/please#3576 and **merged**; until it lands in a
      released plz, large plugin consumers can rarely hit a wedged parse
- [ ] Remote execution audit (absolute-path canonicalization, cwd walks)
- [x] Scale test: the corpus graph is 1,102 crates through
      sync/resolve/build
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
- [x] Differential testing: `scripts/differential.sh` in the corpus puts the
      same request to cargo, which is what tells a bug in these rules apart
      from a crate that cannot be built as asked. It settled sqlx-postgres
      (cargo fails the same way), derive_more and moka (features must be
      chosen), and four -sys crates blocked on missing system packages
- [ ] Differential testing at rule level: cargo resolution as an oracle — pick
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
- [ ] Multi-platform lock entries (per-triple resolution outputs). The
      declaration set already covers every platform in `--targets`; what is
      missing is carrying more than one triple's resolved entries in the
      lock, so a build for another platform reads rather than re-solves

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
- [x] Crate corpus: 240 deliberately awkward crates, 1,102 in the graph,
      all building. Chosen by what predicts trouble (links, build deps,
      platform-gated deps, feature and optional-dep counts, live majors)
      rather than by download count, and each drop verified against cargo
      before dropping. 19 bugs found, none of which had appeared in this
      repo's own ~190 crates. Lives in the private becomeliminal/rust-corpus.
      Still wanted: running it in CI, and a public pass-rate
- [ ] Per-crate escape hatches: patches (arg exists, unexercised), source
      overrides (done via download=), env injection for build scripts

### Explicit non-goals (decided, revisit only on demand)
- [x] rust-analyzer / `rust-project.json` — shipped. The account of what it
      does, the numbers it was measured at and what it does not cover are in
      the rust-analyzer entry in the later Track 2 log.

      The standard library is declared through `sysroot_project`, a nested
      project rust-analyzer resolves relative to `sysroot_src`. The two
      obvious alternatives both fail, and neither says why:
      * listing core and std among the ordinary crates leaves them crates
        that merely happen to be called core and std - lang items attach only
        to the crate rust-analyzer believes *is* the sysroot, so `Sized` is
        unsatisfied for `char` and `Iterator` has no impls, while every
        import and macro around them resolves
      * letting it discover them runs `cargo metadata` over the stdlib
        sources with a nightly-only `-Z` flag against whatever cargo is on
        PATH, which is ambient tooling and fails on a stable cargo

      Describing it is necessary but not sufficient: std is not
      self-contained, and the crates it is built from have to be described
      with it - see the hashbrown finding in the later entry.

      Measured with the version-matched analyzer. An analyzer older than the
      toolchain reports success by failing to load the sysroot at all, which
      is how an earlier attempt was recorded as clean when it was not.
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
      unverified. Pointing a remote executor at them, in that order, closes
      it; asked of the labs pilot on 2026-08-17. The entry-point toolchain
      makes the rustdoc and llvm cases structurally the same as the compile
      path that was verified remotely, which is an argument, not a run
- [x] Upstream plz WaitForPackage race (found 2026-08): a lost-wakeup
      TOCTOU on `packageWaits` in src/core/state.go hangs builds when many
      packages concurrently subinclude the plugin's build_defs — all plz
      versions incl. master. Reproduced (1/12 cold builds on a 40-package
      consumer), root-caused, three-line AddOrGet fix written and validated
      (0/16 with a patched plz). Our own repo dodges it via
      preloadbuilddefs. Fix + regression test submitted upstream as
      thought-machine/please#3576 and **merged**; until it lands in a
      released plz, large plugin consumers can rarely hit a wedged parse
- [ ] Remote execution audit (absolute-path canonicalization, cwd walks)
- [x] Scale test: the corpus graph is 1,102 crates through
      sync/resolve/build
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
- [x] Differential testing: `scripts/differential.sh` in the corpus puts the
      same request to cargo, which is what tells a bug in these rules apart
      from a crate that cannot be built as asked. It settled sqlx-postgres
      (cargo fails the same way), derive_more and moka (features must be
      chosen), and four -sys crates blocked on missing system packages
- [ ] Differential testing at rule level: cargo resolution as an oracle — pick
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
- [ ] Multi-platform lock entries (per-triple resolution outputs). The
      declaration set already covers every platform in `--targets`; what is
      missing is carrying more than one triple's resolved entries in the
      lock, so a build for another platform reads rather than re-solves

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
- [x] Crate corpus: 240 deliberately awkward crates, 1,102 in the graph,
      all building. Chosen by what predicts trouble (links, build deps,
      platform-gated deps, feature and optional-dep counts, live majors)
      rather than by download count, and each drop verified against cargo
      before dropping. 19 bugs found, none of which had appeared in this
      repo's own ~190 crates. Lives in the private becomeliminal/rust-corpus.
      Still wanted: running it in CI, and a public pass-rate
- [ ] Per-crate escape hatches: patches (arg exists, unexercised), source
      overrides (done via download=), env injection for build scripts

### Explicit non-goals (decided, revisit only on demand)
- [x] rust-analyzer / `rust-project.json` — **shipped 2026-08-19**, no
      longer a non-goal. Declare `rust_project` in the repo root BUILD, then
      `plz run //:rust-project`: it queries the build graph for every crate
      in the repo, joins them to the lock, and writes the file at the root.
      There is no crate list to maintain. `targets`/`exclude` narrow it for a
      monorepo whose Rust lives in one subtree.

      Measured with the version-matched analyzer from the dist tarball
      (`rust-analyzer-preview/bin`, 1.97.1) over this repo — 23 first-party
      crates, 219 in total. `analysis-stats` infers 107,105 expressions with
      60 unknown (0%), 0 panics, 0 type mismatches. `diagnostics` reports 27,
      and what they are is known:
      * 26 are `#[derive(Deserialize)]` lines. rust-analyzer cannot infer
        inside that expansion — `#[derive(Serialize)]` and `#[derive(Clone)]`
        on the same struct are clean, which is what says it is the analyzer
        rather than the crate graph. Minimal repro: a two-field struct
        deriving Deserialize, in a project file with nothing else in it
      * 1 is `test/bindgen`, whose `mod point_bindings;` names a file
        rust_bindgen generates at build time. A `mod` declaration cannot
        resolve to a generated file outside the source tree

      Three bugs the measurement found, all fixed:
      * every `#[cfg(test)]` module in the repo was grey dead text — 25 of
        them. First-party crates now carry `cfg = ["test"]`, which is what
        rust-analyzer does under cargo (`cargo.unsetTest` defaults to
        `["core"]`). It also settles a collision: a rust_library and the
        rust_test over the same root are two crates sharing one file, and
        rust-analyzer applies whichever it saw first to both
      * `clap::command!()` errored that `CARGO_PKG_VERSION` was unset. The
        fragment now carries the crate's manifest and the tool reads
        `CARGO_PKG_*` from it, the same as a compile does
      * **`HashMap::new()` did not infer anywhere**, in ordinary first-party
        code — 13 of what were then 41 errors. std is not self-contained:
        `std::collections::HashMap` wraps hashbrown's, and the sysroot was
        described as core/alloc/std alone. The stdlib's own dependency graph
        is now read out of `rust-src` — the manifests, and the vendored
        sources beside them — rather than hardcoded, including optional deps
        that a feature turns on, which is the only way hashbrown reaches core

      Environment requirements:
      * **the editor needs the rust-analyzer extension installed.** Nothing
        warns you if it is not; the file is simply ignored. This cost a
        session to find
      * the file has repo-relative paths, so it belongs at the repo root and
        the repo root is what the editor must open. Fine for a monorepo,
        wrong for anyone opening a single service directory
      * go-to-definition into std needs `rust_toolchain(src_hash = ...)`,
        which fetches `rust-src`. A repo that never builds
        `<toolchain>_sysroot_src` never downloads it
      * `sysroot` wants a rustup-shaped root — `bin/rustc` beside
        `lib/rustlib` — which is `<toolchain>_rustc`, not
        `<toolchain>_sysroot`
      * the CLI does not read the proc-macro server from the project file; it
        takes `--proc-macro-srv`, and needs `LD_LIBRARY_PATH` set to the
        rustc component's `lib` for `librustc_driver`. An editor uses its own

      **Subrepos are covered**, and were the last gap closed. Measured in
      rust-corpus, which pulls rust-rules in as a plugin: 1390 crates, 248
      first-party, 19 from the subrepo, every path on disk. Three things
      made it work. Third-party crates never needed it - they come from the
      lock, which already records where sources landed, so only first-party
      crates go through the query. `plz query outputs` on a plugin's target
      gives its checkout, and every path a fragment carries hangs off that
      one prefix; `plz query input` does *not* work for this, because plz
      reports only the host repo's files as a target's inputs. And a sweep
      that fails descends rather than giving up: a package referencing a
      plugin this repo lacks, or declaring a plugin it also declares - names
      are one global namespace - would otherwise lose the whole subrepo.
      Subrepo crates are never workspace members.

      Not covered, in rough order of how much it would be missed:
      * generated sources — the bindgen case above
      * one lock per project file. A repo with several `rust_resolve` targets
        can join only one, because each lock has its own `third_party_dir`.
        A dep resolving to no lock is now reported by name rather than
        dropped in silence
- Third-party crates' own test suites
- `cargo publish`
- rustc incremental compilation, in any mode (decided 2026-08: crate
  splitting + plz caching is the model; cross-machine cached builds likely
  beat cargo's warm incremental in practice anyway — the benchmark harness
  will settle it)
