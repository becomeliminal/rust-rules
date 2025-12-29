#!/bin/bash
# =============================================================================
# Verify Negative Test (Expected Failure)
# =============================================================================
# Tests that 'straddle_carrier verify' correctly FAILS when deps are missing.
#
# Setup:
#   - Cargo.toml declares: serde, anyhow
#   - BUILD file only has: serde (missing anyhow!)
#
# Expected behavior:
#   - verify should exit with non-zero status
#   - verify should report "Missing dependency: anyhow"
#
# This is a "negative test" - the test PASSES when verify FAILS.
# =============================================================================

set -e

# Find test files in the sandbox
BIN="./straddle_carrier/straddle_carrier_bin"
CARGO_TOML="./straddle_carrier/test_data/verify_negative/Cargo.toml"
BUILD_FILE="./straddle_carrier/test_data/verify_negative/deps.build"
CARGO_BIN="./third_party/rust/rust-1.92.0-x86_64-unknown-linux-gnu/cargo/bin/cargo"
RUSTC_BIN="./third_party/rust/rust-1.92.0-x86_64-unknown-linux-gnu/rustc/bin/rustc"

# Set up PATH and LD_LIBRARY_PATH for cargo/rustc
CARGO_DIR=$(dirname "$CARGO_BIN")
RUSTC_DIR=$(dirname "$RUSTC_BIN")
export PATH="$CARGO_DIR:$RUSTC_DIR:$PATH"

REPO_ROOT=$(pwd | sed 's|/plz-out/tmp/.*||')
RUSTC_LIB_DIR="$REPO_ROOT/plz-out/bin/$(dirname $(dirname $RUSTC_BIN))/lib"
export LD_LIBRARY_PATH="$RUSTC_LIB_DIR:$LD_LIBRARY_PATH"

# Create dummy src for cargo metadata
CARGO_TOML_DIR=$(dirname "$CARGO_TOML")
mkdir -p "$CARGO_TOML_DIR/src"
echo "// dummy" > "$CARGO_TOML_DIR/src/lib.rs"

echo "Testing that verify fails when dependencies are missing..."
echo "  Cargo.toml: $CARGO_TOML"
echo "  BUILD file: $BUILD_FILE"
echo ""

# Run verify and expect it to fail
if "$BIN" verify --cargo-toml "$CARGO_TOML" --build-file "$BUILD_FILE" --mode compatible 2>&1; then
    echo "ERROR: verify should have failed but succeeded!"
    exit 1
fi

echo ""
echo "PASS: verify correctly detected missing dependencies"
