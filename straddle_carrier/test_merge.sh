#!/bin/bash
set -e

# In Please tests, data deps are available in the current directory
BIN="./straddle_carrier/straddle_carrier_bin"
# Test data files are in straddle_carrier/test_data/ 
TEST_DATA="./straddle_carrier/test_data"
TEMP_DIR=$(mktemp -d)

cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

echo "=== Testing merge modes ==="

# Function to normalize BUILD file output for comparison
# Removes comments and extra whitespace
normalize() {
    grep -v '^#' "$1" | grep -v '^$' | sed 's/[[:space:]]*$//'
}

# Function to run a merge test
run_merge_test() {
    local test_name="$1"
    local mode="$2"
    local test_dir="$TEST_DATA/$test_name"
    local output_file="$TEMP_DIR/output.build"

    echo ""
    echo "Testing: $test_name (mode: $mode)..."

    # Run merge
    "$BIN" merge \
        --old-source "$test_dir/old.build" \
        --new-source "$test_dir/new.build" \
        --mode "$mode" \
        --no-backup \
        --output "$output_file" 2>&1

    # Compare output with expected (ignoring comments and whitespace)
    normalize "$output_file" > "$TEMP_DIR/actual_normalized"
    normalize "$test_dir/expected.build" > "$TEMP_DIR/expected_normalized"

    if diff -q "$TEMP_DIR/actual_normalized" "$TEMP_DIR/expected_normalized" > /dev/null 2>&1; then
        echo "  PASS: Output matches expected"
    else
        echo "  FAIL: Output differs from expected"
        echo ""
        echo "  Expected (normalized):"
        cat "$TEMP_DIR/expected_normalized" | sed 's/^/    /'
        echo ""
        echo "  Actual (normalized):"
        cat "$TEMP_DIR/actual_normalized" | sed 's/^/    /'
        echo ""
        echo "  Diff:"
        diff "$TEMP_DIR/expected_normalized" "$TEMP_DIR/actual_normalized" | sed 's/^/    /' || true
        exit 1
    fi
}

# Test override mode
run_merge_test "merge_override" "override"

# Test update-or-expand-only mode
run_merge_test "merge_update_or_expand_only" "update-or-expand-only"

# Test parallel mode
run_merge_test "merge_parallel" "parallel"

echo ""
echo "=== Testing backup functionality ==="

# Test that backup is created by default
echo ""
echo "Testing: backup file creation..."
cp "$TEST_DATA/merge_override/old.build" "$TEMP_DIR/backup_test.build"
"$BIN" merge \
    --old-source "$TEMP_DIR/backup_test.build" \
    --new-source "$TEST_DATA/merge_override/new.build" \
    --mode override 2>&1

if [ -f "$TEMP_DIR/backup_test.build.backup" ]; then
    echo "  PASS: Backup file created"
else
    echo "  FAIL: Backup file not created"
    exit 1
fi

# Verify backup content matches original
if diff -q "$TEST_DATA/merge_override/old.build" "$TEMP_DIR/backup_test.build.backup" > /dev/null 2>&1; then
    echo "  PASS: Backup content matches original"
else
    echo "  FAIL: Backup content differs from original"
    exit 1
fi

# Test --no-backup flag
echo ""
echo "Testing: --no-backup flag..."
cp "$TEST_DATA/merge_override/old.build" "$TEMP_DIR/no_backup_test.build"
rm -f "$TEMP_DIR/no_backup_test.build.backup"
"$BIN" merge \
    --old-source "$TEMP_DIR/no_backup_test.build" \
    --new-source "$TEST_DATA/merge_override/new.build" \
    --mode override \
    --no-backup 2>&1

if [ ! -f "$TEMP_DIR/no_backup_test.build.backup" ]; then
    echo "  PASS: No backup file created with --no-backup"
else
    echo "  FAIL: Backup file was created despite --no-backup"
    exit 1
fi

echo ""
echo "=== All merge tests passed! ==="

