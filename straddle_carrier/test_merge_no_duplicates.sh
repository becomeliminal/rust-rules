#!/bin/bash
# =============================================================================
# Merge Duplicate Detection Test
# =============================================================================
# Verifies that the merge command never produces duplicate crate definitions.
#
# A duplicate would cause Please build failures like:
#   "Duplicate build target in third_party/rust: foo-1.0.0_download"
#
# This test merges overlapping BUILD files and checks that each crate name
# appears exactly once in the output.
# =============================================================================

set -e

BIN="./straddle_carrier/straddle_carrier_bin"
OLD="./straddle_carrier/test_data/merge_duplicates/old.build"
NEW="./straddle_carrier/test_data/merge_duplicates/new.build"
OUTPUT="/tmp/merge_duplicates_test_$$.build"

echo "Testing merge doesn't produce duplicates..."
echo "  Old: $OLD"
echo "  New: $NEW"
echo ""

# Test override mode
echo "=== Testing override mode ==="
"$BIN" merge --old-source "$OLD" --new-source "$NEW" --mode override --output "$OUTPUT" --no-backup

echo "Merged output:"
cat "$OUTPUT"
echo ""

# Count crate names - each should appear exactly once
DUPLICATE_NAMES=$(grep -o 'name = "[^"]*"' "$OUTPUT" | sort | uniq -d)
if [ -n "$DUPLICATE_NAMES" ]; then
    echo "ERROR: Found duplicate crate names in override mode:"
    echo "$DUPLICATE_NAMES"
    rm -f "$OUTPUT"
    exit 1
fi

# Count rust_crate definitions
CRATE_COUNT=$(grep -c "^rust_crate(" "$OUTPUT" || echo "0")
echo "Total crates in output: $CRATE_COUNT"

# Should have: foo (from new), bar (from new), baz (from old), qux (from new) = 4 crates
if [ "$CRATE_COUNT" -ne 4 ]; then
    echo "ERROR: Expected 4 crates, got $CRATE_COUNT"
    rm -f "$OUTPUT"
    exit 1
fi

rm -f "$OUTPUT"

# Test update-or-expand-only mode
echo ""
echo "=== Testing update-or-expand-only mode ==="
"$BIN" merge --old-source "$OLD" --new-source "$NEW" --mode update-or-expand-only --output "$OUTPUT" --no-backup

echo "Merged output:"
cat "$OUTPUT"
echo ""

DUPLICATE_NAMES=$(grep -o 'name = "[^"]*"' "$OUTPUT" | sort | uniq -d)
if [ -n "$DUPLICATE_NAMES" ]; then
    echo "ERROR: Found duplicate crate names in update-or-expand-only mode:"
    echo "$DUPLICATE_NAMES"
    rm -f "$OUTPUT"
    exit 1
fi

CRATE_COUNT=$(grep -c "^rust_crate(" "$OUTPUT" || echo "0")
echo "Total crates in output: $CRATE_COUNT"

# Should have: foo (old, not upgraded due to major bump), bar (merged features), baz (old), qux (new) = 4 crates
if [ "$CRATE_COUNT" -ne 4 ]; then
    echo "ERROR: Expected 4 crates, got $CRATE_COUNT"
    rm -f "$OUTPUT"
    exit 1
fi

rm -f "$OUTPUT"

echo ""
echo "PASS: No duplicates found in merge output"

