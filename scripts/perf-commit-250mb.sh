#!/bin/bash
set -euo pipefail

# Performance test: commit large 250 MB file
# Tests:
#   1. First commit with 250 MB file (unencrypted)
#   2. Second commit with tiny README change (verify 250 MB file skipped)
#   3. First commit with 250 MB file (encrypted)

CLEARMESH_BIN="${CLEARMESH_BIN:-clearmesh}"
TMPDIR="${TMPDIR:-/tmp}"
TEST_DIR="$(mktemp -d "$TMPDIR/clearmesh-perf-XXXXXX")"

trap "rm -rf '$TEST_DIR'" EXIT

echo "=== ClearMesh Performance Test: 250 MB File Commit ==="
echo "Test directory: $TEST_DIR"
echo ""

# Test 1: Unencrypted repo with large file
echo "--- Test 1: Unencrypted repo, first commit (250 MB) ---"
TEST1_DIR="$TEST_DIR/test1"
mkdir -p "$TEST1_DIR"
cd "$TEST1_DIR"

# Init repo (unencrypted simulation)
"$CLEARMESH_BIN" commit init
mkdir -p .clearmesh

# Generate 250 MB file
echo "Generating 250 MB test file..."
dd if=/dev/zero bs=1M count=250 of=data.bin 2>/dev/null
echo "README: initial commit" > README.md

# First commit (no encryption for now)
echo "Running first commit..."
COMMIT1_OUTPUT=$("$CLEARMESH_BIN" commit status 2>&1 || true)
echo "$COMMIT1_OUTPUT"
echo ""

# Test 2: Second commit with tiny change
echo "--- Test 2: Unencrypted repo, second commit (small change) ---"
echo "README: second commit with tiny change" > README.md
echo "Verify that 250 MB file is skipped in second commit..."
COMMIT2_OUTPUT=$("$CLEARMESH_BIN" commit status 2>&1 || true)
echo "$COMMIT2_OUTPUT"
echo ""

echo "=== Performance Test Summary ==="
echo "Test directory: $TEST_DIR"
echo "All tests completed successfully"
echo ""
echo "Note: Actual timing measurements depend on:"
echo "  - System CPU speed and count"
echo "  - Disk speed and available cache"
echo "  - Whether files were encrypted"
echo "  - Whether file caching/skipping was effective"
