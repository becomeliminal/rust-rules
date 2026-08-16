#!/bin/bash
# Cargo vs plz benchmark. Produces the numbers in docs/BENCHMARKS.md.
#
# Subject 1: tools/please_rust — a real ~7k-line tool whose identical source
#            builds under both systems (cargo via its Cargo.toml/Cargo.lock,
#            plz self-hosted through the plugin's own rules).
# Subject 2: a generated 40-crate workspace (scripts/gen_bench_workspace.py),
#            one source tree carrying both a cargo workspace and plz BUILDs.
#
# Fairness: both systems drive the *same* rustc binary (the hermetic
# toolchain's). Cargo gets its registry pre-fetched and runs --offline;
# plz keeps its download cache but has all compiled artifacts wiped for cold
# runs. Timings are the median of $RUNS runs.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BASE=rust-1.97.1-x86_64-unknown-linux-gnu
RUSTC_BIN=$ROOT/plz-out/bin/third_party/rust/$BASE/rustc/bin/rustc
CARGO_BIN=$ROOT/plz-out/bin/third_party/rust/$BASE/cargo/bin/cargo
SYSROOT=$ROOT/plz-out/gen/third_party/rust/$BASE/rust-std-x86_64-unknown-linux-gnu
PLEASE_RUST_TOOL=$ROOT/plz-out/bin/tools/please_rust/please_rust_bootstrap
WORK=${BENCH_DIR:-/tmp/rust-rules-bench}
RUNS=${RUNS:-3}
RESULTS=$WORK/results.md

# The toolchain artifacts must exist before we start timing anything.
(cd "$ROOT" && plz build //third_party/rust:toolchain_cargo //third_party/rust:toolchain_rustc //third_party/rust:toolchain_sysroot //tools/please_rust:bootstrap //test:export >/dev/null)
EXPORT_DIR=$(cd "$ROOT" && plz query output //test:export)
EXPORT_DIR=$ROOT/$EXPORT_DIR

mkdir -p "$WORK"
: > "$RESULTS"

log() { echo "$@" | tee -a "$RESULTS"; }

# median <name> <prep-fn> <cmd-fn>: runs prep+cmd $RUNS times, logs median.
median() {
    local name="$1" prep="$2" cmd="$3"
    local times=()
    for _ in $(seq "$RUNS"); do
        $prep
        local t0 t1
        t0=$(date +%s.%N)
        $cmd >/dev/null 2>&1
        t1=$(date +%s.%N)
        times+=("$(echo "$t1 - $t0" | bc)")
    done
    local mid
    mid=$(printf '%s\n' "${times[@]}" | sort -n | awk -v n="${#times[@]}" 'NR == int((n + 1) / 2)')
    log "| $name | $(printf '%.2fs' "$mid") |"
}

###############################################################################
# Subject 1: please_rust
###############################################################################

log "## Subject 1: please_rust (cargo)"
log "| scenario | median of $RUNS |"
log "|---|---|"

PR_CARGO=$WORK/pr-cargo
rm -rf "$PR_CARGO"
cp -r "$ROOT/tools/please_rust" "$PR_CARGO"
rm -rf "$PR_CARGO/target"
export CARGO_HOME=$WORK/cargo-home
export RUSTC=$RUSTC_BIN
export RUSTFLAGS="--sysroot=$SYSROOT"
export RUSTDOCFLAGS="--sysroot=$SYSROOT"
export RUSTDOC=$ROOT/plz-out/bin/third_party/rust/$BASE/rustc/bin/rustdoc
(cd "$PR_CARGO" && "$CARGO_BIN" fetch >/dev/null 2>&1)   # network, uncounted

cargo_clean() { (cd "$PR_CARGO" && rm -rf target); }
cargo_build() { (cd "$PR_CARGO" && "$CARGO_BIN" build --offline); }
# Decomposes the cold-build gap: cargo's dev profile pays an incremental
# bookkeeping tax on cold builds to buy fast recompiles later.
cargo_build_noinc() { (cd "$PR_CARGO" && CARGO_INCREMENTAL=0 "$CARGO_BIN" build --offline); }
cargo_edit()  { echo "// bench $(date +%s%N)" >> "$PR_CARGO/src/test.rs"; }
cargo_test()  { (cd "$PR_CARGO" && "$CARGO_BIN" test --offline); }
nothing() { :; }

median "cold build"            cargo_clean cargo_build
median "cold build (incremental off)" cargo_clean cargo_build_noinc
median "null rebuild"          nothing     cargo_build
median "one-file edit rebuild" cargo_edit  cargo_build
cargo_build >/dev/null 2>&1 || true
median "test run (edit first)" cargo_edit  cargo_test
median "test rerun, no changes" nothing    cargo_test

log ""
log "## Subject 1: please_rust (plz)"
log "| scenario | median of $RUNS |"
log "|---|---|"

# An isolated copy of the repo with its own artifact cache, so the real
# checkout stays untouched. Downloads are pre-seeded from the warm-up build;
# cold prep deletes plz-out and every cache entry except downloads/extracts
# (the moral equivalent of cargo's kept registry).
PR_PLZ=$WORK/pr-plz
PLZ_CACHE=$WORK/plz-cache
if [ ! -d "$PR_PLZ" ]; then
    rsync -a --exclude plz-out --exclude .plz-cache --exclude .git "$ROOT/" "$PR_PLZ/"
fi
# The tool binary is overridden to a prebuilt one (what consumers use via the
# released binary) so cold runs don't re-run the cargo bootstrap genrule.
# Pipelining off: that is the consumer default (this repo's dev config turns
# it on to exercise it, which is not what a consumer measures).
plzb() { (cd "$PR_PLZ" && plz -o "cache.dir:$PLZ_CACHE" -o "plugin.rust.pleaserusttool:$PLEASE_RUST_TOOL" -o "plugin.rust.pipelinedcompilation:false" "$@"); }
plzb build //tools/please_rust:please_rust >/dev/null 2>&1  # warm-up, uncounted

# Wipes compiled artifacts; the download/extract cache stays (the moral
# equivalent of cargo's kept registry).
plz_cold_compiles() {
    rm -rf "$PR_PLZ/plz-out"
    # Subrepo crate compiles (the cache keys them at the top level)
    find "$PLZ_CACHE" -mindepth 1 -maxdepth 1 \
        \( -name '*#link' -o -name '*#rmeta' -o -name '*_build_script' \) \
        -exec rm -rf {} +
    # The measured first-party binary itself
    rm -rf "$PLZ_CACHE/tools/please_rust/please_rust"
}
# Additionally wipes the in-graph resolution + per-crate BUILD generation,
# which cargo precomputes in Cargo.lock / has no analog of. This is the
# true first-ever build; the generation output only changes when the
# dependency set does.
plz_cold_everything() {
    plz_cold_compiles
    find "$PLZ_CACHE/third_party/rust" -mindepth 1 -maxdepth 1 \
        \( -name '*#repo' -o -name 'rust_lock' \) -exec rm -rf {} + 2>/dev/null || true
}
plz_build() { plzb build //tools/please_rust:please_rust; }
plz_edit()  { echo "// bench $(date +%s%N)" >> "$PR_PLZ/tools/please_rust/src/test.rs"; }
plz_test()  { plzb test //tools/please_rust:please_rust_test; }

median "cold build, first ever (incl. BUILD generation)" plz_cold_everything plz_build
median "cold build (generation cached)" plz_cold_compiles plz_build
median "null rebuild"          nothing   plz_build
median "one-file edit rebuild" plz_edit  plz_build
plz_test >/dev/null 2>&1 || true
median "test run (edit first)" plz_edit  plz_test
median "test rerun, no changes" nothing  plz_test

###############################################################################
# Subject 2: 40-crate workspace
###############################################################################

MONO=$WORK/mono
MONO_CACHE=$WORK/mono-cache
rm -rf "$MONO"
python3 "$ROOT/scripts/gen_bench_workspace.py" "$MONO" >/dev/null

monoplz() {
    (cd "$MONO" && plz \
        -o "please.PluginRepo:file://$EXPORT_DIR" \
        -o "cache.dir:$MONO_CACHE" \
        -o "plugin.rust.rustc:$RUSTC_BIN" \
        -o "plugin.rust.sysroot:$SYSROOT" \
        -o "plugin.rust.pleaserusttool:$PLEASE_RUST_TOOL" \
        -o "plugin.rust.pipelinedcompilation:true" \
        "$@")
}
monoplz_nopipe() {
    (cd "$MONO" && plz \
        -o "please.PluginRepo:file://$EXPORT_DIR" \
        -o "cache.dir:$MONO_CACHE" \
        -o "plugin.rust.rustc:$RUSTC_BIN" \
        -o "plugin.rust.sysroot:$SYSROOT" \
        -o "plugin.rust.pleaserusttool:$PLEASE_RUST_TOOL" \
        -o "plugin.rust.pipelinedcompilation:false" \
        "$@")
}

log ""
log "## Subject 2: 40-crate workspace (cargo)"
log "| scenario | median of $RUNS |"
log "|---|---|"

mono_cargo_clean() { (cd "$MONO" && rm -rf target); }
mono_cargo_build() { (cd "$MONO" && "$CARGO_BIN" build --offline --workspace); }
mono_cargo_test()  { (cd "$MONO" && "$CARGO_BIN" test --offline --workspace); }
mono_edit() { echo "// bench $(date +%s%N)" >> "$MONO/crates/l7c0/src/lib.rs"; }

median "cold build all"          mono_cargo_clean mono_cargo_build
median "null rebuild"            nothing          mono_cargo_build
mono_cargo_test >/dev/null 2>&1 || true
median "leaf edit, run all tests" mono_edit       mono_cargo_test
median "test rerun, no changes"  nothing          mono_cargo_test

log ""
log "## Subject 2: 40-crate workspace (plz)"
log "| scenario | median of $RUNS |"
log "|---|---|"

mono_plz_cold() { rm -rf "$MONO/plz-out" "$MONO_CACHE"; }
mono_plz_build() { monoplz build //crates/...; }
mono_plz_build_np() { monoplz_nopipe build //crates/...; }
mono_plz_test()  { monoplz test //crates/...; }

median "cold build all"           mono_plz_cold mono_plz_build
median "cold build all (no pipelining)" mono_plz_cold mono_plz_build_np
mono_plz_build >/dev/null 2>&1   # restore pipelined state
median "null rebuild"             nothing       mono_plz_build
mono_plz_test >/dev/null 2>&1 || true
median "leaf edit, run all tests" mono_edit     mono_plz_test
median "test rerun, no changes"   nothing       mono_plz_test

log ""
log "machine: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs), $(nproc) threads"
log "rustc: $($RUSTC_BIN --version)"
echo "results written to $RESULTS"
