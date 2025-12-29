#!/bin/bash
# =============================================================================
# Real-World Merge Test
# =============================================================================
# Tests merge with actual third_party/rust/BUILD file structure.
#
# This catches edge cases that simple tests miss:
#   - Crates with multiple versions (forks like hashbrown, indexmap)
#   - Crates with rust_crate_download rules
#   - Crates with special fields (src_root, crate_type, download, etc.)
#   - Comments that might confuse the parser
#
# Uses copies of real files in test_data/merge_real/
# =============================================================================

set -e

BIN="./straddle_carrier/straddle_carrier_bin"
OLD="./straddle_carrier/test_data/merge_real/third_party_rust.build"
NEW="./straddle_carrier/test_data/merge_real/straddle_deps.build"
OUTPUT="/tmp/merge_real_test_$$.build"

echo "Testing merge with real file structure..."
echo "  Old: $OLD (first 500 lines of third_party/rust/BUILD)"
echo "  New: $NEW"
echo ""

# Test override mode
echo "=== Testing override mode ==="
"$BIN" merge --old-source "$OLD" --new-source "$NEW" --mode override --output "$OUTPUT" --no-backup

# Count crate names - each should appear exactly once
# Only match actual name = fields (starting with whitespace), not comments
DUPLICATE_NAMES=$(grep -oP '^\s+name = "[^"]*"' "$OUTPUT" | sort | uniq -d)
if [ -n "$DUPLICATE_NAMES" ]; then
    echo "ERROR: Found duplicate crate names:"
    echo "$DUPLICATE_NAMES"
    echo ""
    echo "=== Full output ==="
    cat "$OUTPUT"
    rm -f "$OLD" "$OUTPUT"
    exit 1
fi

# Count rust_crate definitions
OLD_CRATE_COUNT=$(grep -c "^rust_crate(" "$OLD" || echo "0")
NEW_CRATE_COUNT=$(grep -c "^rust_crate(" "$NEW" || echo "0")
OUTPUT_CRATE_COUNT=$(grep -c "^rust_crate(" "$OUTPUT" || echo "0")

echo "Old crates: $OLD_CRATE_COUNT"
echo "New crates: $NEW_CRATE_COUNT"
echo "Output crates: $OUTPUT_CRATE_COUNT"

rm -f "$OLD" "$OUTPUT"

echo ""
echo "PASS: No duplicates found in real file merge"

