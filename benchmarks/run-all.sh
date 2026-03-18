#!/usr/bin/env bash
# MuonGit Cross-Language Benchmark Runner
# Runs benchmarks for Rust, Swift, and Kotlin, then produces a comparison report.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_DIR="$REPO_ROOT/benchmarks/results"
mkdir -p "$RESULTS_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "=== MuonGit Benchmark Suite ==="
echo ""

# Rust
echo "[1/3] Running Rust benchmarks..."
cd "$REPO_ROOT/rust"
cargo build --release --bin muongit-bench 2>/dev/null
cargo run --release --bin muongit-bench 2>/dev/null > "$RESULTS_DIR/rust.jsonl"
echo "  -> $RESULTS_DIR/rust.jsonl"

# Swift
echo "[2/3] Running Swift benchmarks..."
cd "$REPO_ROOT/swift"
swift build -c release --product muongit-bench 2>/dev/null
swift run -c release muongit-bench 2>/dev/null > "$RESULTS_DIR/swift.jsonl"
echo "  -> $RESULTS_DIR/swift.jsonl"

# Kotlin
echo "[3/3] Running Kotlin benchmarks..."
cd "$REPO_ROOT/kotlin"
./gradlew -q bench 2>/dev/null > "$RESULTS_DIR/kotlin.jsonl"
echo "  -> $RESULTS_DIR/kotlin.jsonl"

# Merge all results
cat "$RESULTS_DIR/rust.jsonl" "$RESULTS_DIR/swift.jsonl" "$RESULTS_DIR/kotlin.jsonl" \
    > "$RESULTS_DIR/all_${TIMESTAMP}.jsonl"

# Generate comparison report
echo ""
echo "=== Benchmark Comparison Report ==="
echo ""
printf "%-25s %12s %12s %12s\n" "Operation" "Rust (ms)" "Swift (ms)" "Kotlin (ms)"
printf "%-25s %12s %12s %12s\n" "-------------------------" "------------" "------------" "------------"

# Parse JSON lines and build comparison table
python3 -c "
import json, sys

results = {}
for path, lang in [('$RESULTS_DIR/rust.jsonl', 'rust'), ('$RESULTS_DIR/swift.jsonl', 'swift'), ('$RESULTS_DIR/kotlin.jsonl', 'kotlin')]:
    try:
        for line in open(path):
            line = line.strip()
            if not line:
                continue
            d = json.loads(line)
            op = d['op']
            if op not in results:
                results[op] = {}
            results[op][lang] = d['median_ms']
    except Exception:
        pass

ops = ['sha1_10kb', 'sha256_10kb', 'oid_cmp_256x16k',
       'sha1_1mb', 'sha256_1mb', 'sha1_10mb', 'sha256_10mb',
       'oid_create_10k', 'oid_create_100k', 'oid_create_1k',
       'blob_hash_10k', 'blob_hash_1k',
       'tree_serialize_1k', 'tree_serialize_10k',
       'commit_serialize_10k',
       'index_rw_1k', 'index_rw_10k',
       'diff_1k', 'diff_10k']

for op in ops:
    r = results.get(op, {})
    rust = '%.3f' % r['rust'] if 'rust' in r else '-'
    swift = '%.3f' % r['swift'] if 'swift' in r else '-'
    kotlin = '%.3f' % r['kotlin'] if 'kotlin' in r else '-'
    print(f'{op:<25s} {rust:>12s} {swift:>12s} {kotlin:>12s}')
" 2>/dev/null || echo "(python3 required for comparison table)"

echo ""
echo "Results saved to: $RESULTS_DIR/all_${TIMESTAMP}.jsonl"
echo "Done."
