# Roadmap: beyond Cargo

Where rust-rules stands against Cargo today, what is missing, and the path to
being the better choice — not just a hermetic alternative. Written against
v0.2.0 (2026-08).

## Where we already beat Cargo

These are structural wins Cargo cannot match without becoming a different
tool:

| Capability | rust-rules | Cargo |
|---|---|---|
| Hermeticity | Everything hash-verified: toolchain, crates, tool. Sandboxed builds, no network, no ambient env | Ambient `RUSTFLAGS`/`PATH`/`.cargo/config`, network by default, host cc silently |
| Reproducibility | Content-addressed inputs; CI produced identical results on a cold machine first try | Fingerprints are mtime-fragile; "works on my machine" is a genre |
| Test caching | Unchanged tests cost 0s — plz caches results | `cargo test` reruns everything, every time |
| Build caching | Per-crate, shareable via plz remote cache across machines/CI | Per-workspace `target/` dir; sccache bolts on partially |
| Monorepo | One graph across Rust, Go, C, JS, proto. A `grpc_library` generates Rust *and* Go stubs from the same proto | One language, one workspace; polyglot means glue scripts |
| Static binaries | Default (`crt-static`), like Go | Opt-in flag folklore |
| Test reporting | Per-test results in the build system's UI/CI | Text output |
| Dependency review | Adding a dep is a reviewable BUILD diff with pinned hash; resolution is deterministic in the graph | Lockfile churn; feature drift is silent |

## What Cargo still has on us

Honest list, ordered by how much it matters to a developer choosing daily.

### 1. The inner loop

Development here is editor-agnostic and increasingly agent-driven; what
matters is that `plz build`/`plz test` are fast, targeted, and give clear
diagnostics — which they already are and do. The remaining gaps:

- **Incremental compilation**: rustc's incremental cache makes small edits
  to a big crate fast under Cargo. We rebuild whole crates (plz caches
  *across* crates perfectly, but not within one). Mitigations: crate
  splitting is idiomatic in monorepos anyway, and pipelined compilation
  (done 2026-08, `PipelinedCompilation`) builds dependency chains at
  frontend depth.
- ~~fmt / clippy / doc~~ — done 2026-08: `rust_clippy`, `rust_fmt_test`,
  `rust_doc`, all from the dist tarball's own binaries.

### 2. Ecosystem long tail

- ~~`links` / `DEP_<LINKS>_<KEY>` propagation~~ — done 2026-08
  (test/links proves the pair end to end). bindgen too: `rust_bindgen`,
  with the bindgen binary built from crates in-graph.
- **The native build tail**: `cc`-crate builds work (host cc via the cc
  plugin's config). pkg-config, cmake, bindgen (libclang), vendored
  autotools builds are unproven; `ring`/`openssl-sys`/`rocksdb` will fight.
  Needs: DEP_* wiring, an optional hermetic C toolchain target, per-crate
  escape hatches, and a **top-100-crates corpus in CI** to burn the tail
  down measurably instead of anecdotally.
- **Git forges beyond GitHub** (gitlab/self-hosted): needs a generic git
  fetch rule; archive-URL hosts work today.
- **Private/alternative registries**: index URL + download URL templating.
- **`sync --upgrade`**: bump-all-to-latest-compatible flow (`cargo update`
  parity). `lock --add` exists; upgrade doesn't.

### 3. Resolver fidelity

- **PubGrub backtracking**: `lock` is greedy max-satisfying with clear
  conflict errors. Correct until a real conflict needs backtracking; the
  `select()` seam is built for the swap.
- **MSRV-aware resolution** (Cargo ≥1.84 behavior): we ignore
  `rust-version`.
- **Multi-platform locks**: one target triple per resolve. The host/target
  unit split is done, so this is threading, not redesign.

### 4. Build fidelity

- **Profiles**: we have dbg (`-g`) / opt (`-O`). Cargo has opt-level 0–3/s/z,
  lto thin/fat, codegen-units, panic=abort, debug-assertions,
  overflow-checks, strip, per-dep overrides. Plan: profile knobs on the
  plugin config mapped into plz's dbg/opt configurations, with per-target
  overrides.
- **Cross-compilation / more targets**: wasm, musl, aarch64, embedded
  (per-target `rust-std`, `build-std`). plz has native cross-arch labels to
  hang this on.
- **Nightly / channel toolchains**: the toolchain rule takes any version;
  channels with moving hashes need a policy.

### 5. Migration cost

- **First-party workspace importer** (`sync --import-workspace`): walk a
  cargo workspace's manifests and source tree, emit
  `rust_library`/`rust_binary`/`rust_test` BUILD files. This is puku for
  Rust and the single biggest adoption lever — it turns "switching is a
  slog" into an afternoon.

### 6. Operational debt (from the labs pilot)

- **Subrepo name collision**: plugin-internal third_party subrepos register
  globally-unqualified names; fix by namespacing with `subrepo_name()` and
  matching label prefixes. Until then: consumers use the released binary,
  never plugin-internal targets (the go-rules convention anyway).
- **`lock` feature UX**: enabling a new optional feature (serde `derive`)
  required a manual matching-version `lock --add serde_derive@1.0.229`.
  `lock` should fetch newly-activated optionals itself, and accept
  `--add serde@1 --features derive`.
- **Release automation**: CI should build and attach the static
  `please_rust` binary on tag (v0.2.0's was attached by hand).
- **macOS/Windows hosts**: parametrized but unvalidated.
- **Remote execution audit**: absolute-path canonicalization and cwd walks
  need checking against plz remote build workers.

## The plan

Phases ordered by leverage. S = hours, M = a day or two, L = up to a week.

### Phase 1 — Daily-drivable (win the developer)

The phase that makes someone *choose* this over Cargo for their next
service.

1. ~~Workspace importer~~ — done 2026-08: `sync --import-workspace`
  emits member BUILD files and scaffolds the repo config; a bare
  40-crate cargo workspace imports, builds and tests in one command.
2. ~~fmt / clippy / doc rules~~ — done 2026-08.
3. ~~Profile knobs~~ — done 2026-08: opt-level, lto, codegen-units,
  panic, strip, debug-assertions.
4. **Pilot debt** — ~~`lock` feature UX~~ and ~~release-on-tag CI~~ done
  2026-08; subrepo namespacing fix remains.

**Exit criterion:** a cargo project ports with one command, clippy and fmt
gate CI, and builds/tests get faster.

### Phase 2 — Ecosystem depth (stop losing on crates)

6. ~~`links`/`DEP_*` propagation~~ — done 2026-08.
7. **Top-100-crates corpus in CI** — a generated test package that locks and
  builds the most-downloaded crates; the pass-rate is the public metric.
  (M, then continuous)
8. **Optional hermetic C toolchain target** + pkg-config/bindgen strategy,
  driven by what the corpus fails on. (L)
9. **Generic git + private registries.** (M)
10. **PubGrub at the `select()` seam; MSRV-aware resolution.** (M)

**Exit criterion:** corpus pass-rate ≥90 of top 100, with the failures
individually explained.

### Phase 3 — Platform breadth (parked)

Parked by decision (2026-08): the team is single-platform (linux-amd64),
so cross-platform work has no consumer to validate it. Revisit when a real
target appears; the host/target unit split already did the hard part.


11. **Multi-target resolution + cross-compilation** (musl, aarch64, wasm)
  on plz's cross-arch machinery — the unit split already did the hard
  part. (L)
12. **macOS host support**, validated in CI. (M)
13. **Nightly/channel toolchains.** (S)

### Phase 4 — Proof (make the "faster than Cargo" claim with numbers)

14. ~~Benchmark harness~~ — done 2026-08: `scripts/benchmark.sh`, numbers
  in [BENCHMARKS.md](BENCHMARKS.md).
15. **Remote cache/execution validation** at labs scale. (M)
16. **Stress**: a 1k+ crate graph through sync/resolve/build. (S)

## What we deliberately do not chase

- **Editor/IDE integration** (`rust-project.json` for rust-analyzer): parked.
  Development on this codebase is emacs- and agent-driven; the build system's
  job is fast targeted commands and clear diagnostics, not editor plumbing.
  The dist tarball ships `rust-analyzer-preview`, so if a consumer ever needs
  it, `please_rust ide` generating `rust-project.json` is a well-understood
  add (it is how rules_rust does it) — on request, not on the roadmap.
- **Building third-party crates' own test suites** — Cargo doesn't run your
  dependencies' tests either in normal use.
- **`cargo publish`** — publishing to crates.io stays with Cargo; this is a
  build system, not a registry client.
- **Bit-for-bit rustc incremental parity** — plz's cross-crate caching plus
  crate splitting is the monorepo answer; chasing rustc's intra-crate
  incremental state trades away hermeticity.

## The thesis, restated

Cargo is a excellent single-language package manager with a build system
attached. In a monorepo, its model inverts into a liability: per-workspace
caches, no cross-language graph, ambient configuration, network coupling,
and tests that always rerun. Everything in Phases 1–2 is engineering, not
research — after them, the honest comparison is "Cargo's inner loop
convenience vs. hermetic builds, shared caches, cached tests, and one graph
for every language you ship." For a team running services, that trade
already reads one way; the phases above close the convenience gap so the
trade isn't even close.
