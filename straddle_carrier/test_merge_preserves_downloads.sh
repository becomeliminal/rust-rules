#!/bin/bash
# Test that merge preserves rust_crate_download rules

set -e

BIN="./straddle_carrier/straddle_carrier_bin"
OLD="./straddle_carrier/test_data/merge_preserves_downloads/old.build"
NEW="./straddle_carrier/test_data/merge_preserves_downloads/new.build"
OUTPUT="/tmp/merge_preserves_downloads_$$.build"

echo "Testing that merge preserves rust_crate_download rules..."
echo "  Old: $OLD"
echo "  New: $NEW"
echo ""

"$BIN" merge --old-source "$OLD" --new-source "$NEW" --mode override --output "$OUTPUT" --no-backup

echo "=== Merged output ==="
cat "$OUTPUT"
echo ""

# Check that rust_crate_download was preserved
DOWNLOAD_COUNT=$(grep -c 'rust_crate_download' "$OUTPUT" || echo "0")
if [ "$DOWNLOAD_COUNT" -eq 0 ]; then
    echo "ERROR: rust_crate_download rules were lost during merge!"
    echo "Expected at least 1 rust_crate_download rule, got $DOWNLOAD_COUNT"
    rm -f "$OUTPUT"
    exit 1
fi

echo "Found $DOWNLOAD_COUNT rust_crate_download rule(s)"

# Check that download = parameter was preserved
if ! grep -q 'download = ":foo_dl"' "$OUTPUT"; then
    echo "ERROR: download = parameter was lost during merge!"
    rm -f "$OUTPUT"
    exit 1
fi

echo "download parameter preserved correctly"

rm -f "$OUTPUT"
echo ""
echo "PASS: merge preserves rust_crate_download rules"

