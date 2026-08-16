# Benchmarks: cargo vs plz

The same project, built by both systems, on the same machine, driving the
**same rustc binary**. The subject is `tools/please_rust` itself: a real
~7k-line tool whose identical source builds under cargo (its checked-in
`Cargo.toml`/`Cargo.lock`, ~45 registry crates including build scripts and
proc macros) and under plz (self-hosted through this plugin's own rules).

Every number is reproducible with `scripts/benchmark.sh`. Medians of 3 runs.

## Results

| scenario | cargo | plz |
|---|---|---|
| cold build (all deps + tool) | 15.10s | **8.76s** |
| cold build, `CARGO_INCREMENTAL=0` | 16.83s | — |
| null rebuild (nothing changed) | **0.05s** | 0.33s |
| one-file edit, rebuild | **0.54s** | 6.78s |
| one-file edit, run test suite | **0.92s** | 8.24s |
| test rerun, nothing changed | 0.30s | 0.37s |

Machine: AMD Ryzen AI 9 HX 370, 24 threads, linux-amd64.
Toolchain: rustc 1.97.1 for both sides.

## Reading the numbers honestly

**plz wins the cold build ~1.7x** — and its number *includes* work cargo
does not do at build time: dependency resolution and per-crate BUILD
generation run inside the build graph (cargo precomputes resolution into
Cargo.lock). Wiping that generated state too barely moves the needle
(8.76s vs 8.85s with it cached): generation overlaps with compilation.
The gap is not rustc doing less work — it's scheduling. It is also not
cargo's incremental bookkeeping: turning incremental off makes cargo's
cold build no faster (row 2). Part of the difference is profile defaults
(cargo's dev profile uses 256 codegen units per crate against rustc's
default 16), part is cargo's fingerprinting and build-script orchestration
between compiles.

**cargo wins the single-crate edit loop ~12x.** rustc's incremental cache
recompiles only what changed inside a crate; plz recompiles the whole
crate. This is a deliberate trade — intra-crate incremental state is
machine-local and unreproducible, and caching it would break hermeticity.
The mitigation is structural: split crates. In a monorepo of small crates,
an edit recompiles one small crate plus dependent frontends (pipelined
compilation builds chains at frontend depth), and plz's per-crate cache
does the rest.

**No-op operations are a near-tie here** (~0.3s of plz process overhead vs
cargo's mtime scan) — but they are not the same operation. `cargo test`
always re-runs every test; plz returns cached *results* for anything whose
inputs didn't change. On this suite (88 fast unit tests, 0.3s total) that
is worth nothing. On a suite that takes minutes, the plz number stays
~0.3s while cargo's grows with the suite. The same asymmetry applies
across a workspace: after editing one crate, cargo re-runs every test in
the workspace; plz re-runs only the tests downstream of the edit.

## What this doesn't measure

- **Remote caching.** Every plz artifact here is content-addressed and —
  since compiles remap the sandbox path away — byte-identical across
  machines. A shared cache turns any teammate's cold build into a
  download; CI validated this from a cold runner in ~3 minutes including
  toolchain download. Cargo has no equivalent without bolting on sccache.
- **Hermeticity.** The cargo side of this benchmark inherits whatever
  `RUSTFLAGS`, ambient config and network state exist; the plz side is
  sandboxed and hash-verified. That difference doesn't show in seconds.
- **Polyglot graphs.** The benchmark is pure Rust; the plugin's normal
  habitat is a graph shared with Go, C and proto targets.

`scripts/benchmark.sh` also carries a generated-workspace mode
(`scripts/gen_bench_workspace.py`) for monorepo-shaped measurements across
many small crates; at toy sizes its numbers mostly measure process
overhead, so they are not published here.
