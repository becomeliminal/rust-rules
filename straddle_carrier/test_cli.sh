#!/bin/bash
set -e

# In Please tests, data deps are available relative to test dir
BIN="./straddle_carrier/straddle_carrier_bin"

echo "Testing: no arguments shows help and exits with code 2..."
set +e
OUTPUT=$("$BIN" 2>&1)
EXIT_CODE=$?
set -e

if [[ $EXIT_CODE -ne 2 ]]; then
    echo "FAIL: Expected exit code 2, got $EXIT_CODE"
    exit 1
fi

if ! echo "$OUTPUT" | grep -q "Usage:"; then
    echo "FAIL: Expected help output to contain 'Usage:'"
    echo "Got: $OUTPUT"
    exit 1
fi

echo "Testing: --help shows help and exits with code 0..."
OUTPUT=$("$BIN" --help 2>&1)
EXIT_CODE=$?

if [[ $EXIT_CODE -ne 0 ]]; then
    echo "FAIL: Expected exit code 0 for --help, got $EXIT_CODE"
    exit 1
fi

if ! echo "$OUTPUT" | grep -q "Usage:"; then
    echo "FAIL: Expected --help output to contain 'Usage:'"
    echo "Got: $OUTPUT"
    exit 1
fi

if ! echo "$OUTPUT" | grep -q "cargo metadata"; then
    echo "FAIL: Expected --help to contain long description with 'cargo metadata'"
    echo "Got: $OUTPUT"
    exit 1
fi

echo "Testing: -h shows short help and exits with code 0..."
OUTPUT=$("$BIN" -h 2>&1)
EXIT_CODE=$?

if [[ $EXIT_CODE -ne 0 ]]; then
    echo "FAIL: Expected exit code 0 for -h, got $EXIT_CODE"
    exit 1
fi

if ! echo "$OUTPUT" | grep -q "Usage:"; then
    echo "FAIL: Expected -h output to contain 'Usage:'"
    echo "Got: $OUTPUT"
    exit 1
fi

echo "Testing: --version shows version and exits with code 0..."
OUTPUT=$("$BIN" --version 2>&1)
EXIT_CODE=$?

if [[ $EXIT_CODE -ne 0 ]]; then
    echo "FAIL: Expected exit code 0 for --version, got $EXIT_CODE"
    exit 1
fi

if ! echo "$OUTPUT" | grep -q "straddle_carrier 0.1.0"; then
    echo "FAIL: Expected --version output to contain 'straddle_carrier 0.1.0'"
    echo "Got: $OUTPUT"
    exit 1
fi

echo "Testing: -V shows version and exits with code 0..."
OUTPUT=$("$BIN" -V 2>&1)
EXIT_CODE=$?

if [[ $EXIT_CODE -ne 0 ]]; then
    echo "FAIL: Expected exit code 0 for -V, got $EXIT_CODE"
    exit 1
fi

if ! echo "$OUTPUT" | grep -q "straddle_carrier 0.1.0"; then
    echo "FAIL: Expected -V output to contain 'straddle_carrier 0.1.0'"
    echo "Got: $OUTPUT"
    exit 1
fi

echo "All CLI tests passed!"
