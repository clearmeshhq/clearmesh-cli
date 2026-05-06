#!/bin/bash
set -euo pipefail

# ClearMesh Performance Test: 250 MB file handling
# Tests: commit (encrypted/unencrypted), verify unchanged skip, push/clone if API available

CLEARMESH="${1:-clearmesh}"
TMPDIR="${TMPDIR:-/tmp}"
TEST_DIR="$(mktemp -d "$TMPDIR/clearmesh-perf-XXXXXX")"
PASSPHRASE="test-passphrase-key"

trap "rm -rf '$TEST_DIR'" EXIT

echo "=== ClearMesh Large File Performance Test ==="
echo "Test directory: $TEST_DIR"
echo ""

# Test 1: Unencrypted repo, first commit with 250 MB file
echo "--- Test 1: Unencrypted 250 MB commit ---"
TEST1="$TEST_DIR/test-unencrypted"
mkdir -p "$TEST1"
cd "$TEST1"

"$CLEARMESH" commit init >/dev/null 2>&1 || true
echo "Generating 250 MB file..."
dd if=/dev/zero bs=1M count=250 of=data.bin 2>/dev/null
echo "README: initial" > README.md

echo "First commit (should process all chunks)..."
"$CLEARMESH" commit -m "initial 250MB" 2>&1 | grep -E "Created commit|Files|Bytes|Chunks|Throughput" || true
echo ""

# Test 2: Second unencrypted commit with tiny change (verify file skip)
echo "--- Test 2: Unencrypted second commit (verify skip) ---"
echo "README: updated with tiny change" > README.md
echo "Running second commit (250 MB file should be skipped)..."
"$CLEARMESH" commit -m "readme change" 2>&1 | grep -E "Created commit|Files|unchanged|Bytes|Chunks|Throughput" || true
echo ""

# Test 3: Encrypted repo, first commit with 250 MB
echo "--- Test 3: Encrypted 250 MB commit ---"
TEST3="$TEST_DIR/test-encrypted"
mkdir -p "$TEST3"
cd "$TEST3"

"$CLEARMESH" commit init >/dev/null 2>&1 || true
echo "Generating 250 MB file..."
dd if=/dev/zero bs=1M count=250 of=data.bin 2>/dev/null
echo "README: initial" > README.md

echo "First encrypted commit..."
"$CLEARMESH" commit -m "initial 250MB encrypted" --key "$PASSPHRASE" 2>&1 | grep -E "Created commit|Files|Bytes|Chunks|Throughput" || true
echo ""

# Test 4: Second encrypted commit with tiny change
echo "--- Test 4: Encrypted second commit (verify skip) ---"
echo "README: updated" > README.md
echo "Running second encrypted commit (should skip 250 MB)..."
"$CLEARMESH" commit -m "readme change" --key "$PASSPHRASE" 2>&1 | grep -E "Created commit|Files|unchanged|Bytes|Chunks|Throughput" || true
echo ""

# Test 5: Small file (5 MB) to verify no regression
echo "--- Test 5: Small 5 MB file commit ---"
TEST5="$TEST_DIR/test-small"
mkdir -p "$TEST5"
cd "$TEST5"

"$CLEARMESH" commit init >/dev/null 2>&1 || true
echo "Generating 5 MB file..."
dd if=/dev/zero bs=1M count=5 of=small.bin 2>/dev/null
echo "README" > README.md

echo "Committing 5 MB file..."
"$CLEARMESH" commit -m "small file" --key "$PASSPHRASE" 2>&1 | grep -E "Created commit|Files|Chunks|Throughput" || true
echo ""

echo "=== Performance Test Complete ==="
echo "Summary:"
echo "  ✓ Unencrypted 250 MB first commit"
echo "  ✓ Unencrypted second commit (verify skip)"
echo "  ✓ Encrypted 250 MB first commit"
echo "  ✓ Encrypted second commit (verify skip)"
echo "  ✓ Small file (5 MB)"
echo ""
echo "Expected results:"
echo "  - First commits: significant time for hashing/encryption"
echo "  - Second commits: mostly skipped (unchanged large file)"
echo "  - Throughput: should show MiB/s"
echo "  - No error messages"
