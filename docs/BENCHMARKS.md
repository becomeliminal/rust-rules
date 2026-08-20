# Benchmarks: cargo vs plz

The same project, built by both systems, on the same machine, driving the
**same rustc binary**. The subject is `tools/please_rust` itself: a real
~7k-line tool whose identical source builds under cargo (its checked-in
`Cargo.toml`/`Cargo.lock`, ~45 registry crates including build scripts and
proc macros) and under plz (self-hosted through this plugin's own rules).

Every number is reproducible with `scripts/benchmark.sh`. Medians of 3 runs.

## Results

Measured 2026-08-20 at v0.7.1, medians of three, on an idle machine.

### Subject 1: please_rust, 12,440 lines, ~45 registry crates

| scenario | cargo | plz |
|---|---|---|
| cold build (all deps + tool) | 9.27s | **6.65s** |
| cold build, `CARGO_INCREMENTAL=0` | 8.68s | n/a |
| cold build, generation cached | n/a | 6.81s |
| null rebuild (nothing changed) | **0.03s** | 0.19s |
| one-file edit, rebuild | **0.52s** | 5.36s |
| one-file edit, run test suite | **0.79s** | 6.54s |
| test rerun, nothing changed | 0.22s | **0.19s** |

### Subject 2: a generated 40-crate workspace

| scenario | cargo | plz |
|---|---|---|
| cold build all | **0.50s** | 1.95s |
| cold build all, no pipelining | n/a | 1.92s |
| null rebuild | **0.02s** | 0.16s |
| leaf edit, run all tests | 1.22s | **0.31s** |

Machine: AMD Ryzen AI 9 HX 370, 24 threads, linux-amd64.
Toolchain: rustc 1.97.1 for both sides.

### What changed since the previous measurement

The previous numbers were taken 2026-08-16 at 8c8110c and recorded plz winning
the cold build by 1.7x. It is **1.39x** now. The figure moved because the
toolchain was split so a compile stages the compiler rather than the whole
distribution, and because rmeta pipelining landed. Both columns moved: cargo's
cold build also came down, from 15.10s to 9.27s, which is why the ratio
narrowed rather than widened.

Two results reversed. plz now wins the test rerun rather than losing it, and
on the 40-crate workspace plz wins "edit a leaf, run every test" by 3.9x while
losing the cold build, which is the clearest illustration of where each system
spends its advantage.

## Reading the numbers honestly

**plz wins the cold build 1.39x**, and its number *includes* work cargo does
not do at build time: dependency resolution and per-crate BUILD generation run
inside the build graph, where cargo precomputes resolution into Cargo.lock.
Wiping that generated state so it re-runs barely moves the result, 6.65s
against 6.81s cached, which is within the noise of three runs: generation
overlaps with compilation.

The gap is not rustc doing less work, it is scheduling. Turning cargo's
incremental bookkeeping off makes its cold build slightly faster, 8.68s
against 9.27s, so incremental is a small cost at cold-build time rather than
the explanation. Part of the remaining difference is profile defaults, since
cargo's dev profile uses 256 codegen units per crate against rustc's default
16, and part is cargo's fingerprinting and build-script orchestration between
compiles.

**cargo wins the single-crate edit loop ~10x.** rustc's incremental cache
recompiles only what changed inside a crate; plz recompiles the whole
crate. This is a deliberate trade. Intra-crate incremental state is
machine-local and unreproducible, and caching it would break hermeticity.
The mitigation is structural: split crates. In a monorepo of small crates,
an edit recompiles one small crate plus dependent frontends (pipelined
compilation builds chains at frontend depth), and plz's per-crate cache
does the rest.

**No-op operations are a near-tie here** (~0.3s of plz process overhead vs
cargo's mtime scan), but they are not the same operation. `cargo test`
always re-runs every test; plz returns cached *results* for anything whose
inputs didn't change. On this suite (88 fast unit tests, 0.3s total) that
is worth nothing. On a suite that takes minutes, the plz number stays
~0.3s while cargo's grows with the suite. The same asymmetry applies
across a workspace: after editing one crate, cargo re-runs every test in
the workspace; plz re-runs only the tests downstream of the edit.

## What this doesn't measure

- **Remote caching.** Every plz artifact here is content-addressed, and
  since compiles remap the sandbox path away, byte-identical across
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
