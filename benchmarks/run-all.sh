#!/usr/bin/env bash
# MuonGit Cross-Language Benchmark Runner
# Runs benchmarks for libgit2 (C), Rust, Swift, and Kotlin, then produces a comparison report.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_DIR="$REPO_ROOT/benchmarks/results"
mkdir -p "$RESULTS_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "=== MuonGit Benchmark Suite ==="
echo ""

# libgit2 (C baseline)
echo "[1/4] Running libgit2 benchmarks..."
LIBGIT2_BUILD="$REPO_ROOT/vendor/libgit2/build"
if [ ! -f "$LIBGIT2_BUILD/libgit2.dylib" ] && [ ! -f "$LIBGIT2_BUILD/libgit2.so" ]; then
    echo "  Building libgit2..."
    mkdir -p "$LIBGIT2_BUILD"
    cd "$LIBGIT2_BUILD"
    cmake .. -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTS=OFF -DBUILD_CLI=OFF -DUSE_SSH=OFF -DUSE_NTLMCLIENT=OFF 2>/dev/null
    cmake --build . --config Release -j$(sysctl -n hw.ncpu 2>/dev/null || nproc) 2>/dev/null
fi
BENCH_BIN="$REPO_ROOT/benchmarks/libgit2/muongit-bench-libgit2"
if [ ! -f "$BENCH_BIN" ]; then
    echo "  Compiling libgit2 benchmark..."
    cc -O2 -o "$BENCH_BIN" "$REPO_ROOT/benchmarks/libgit2/bench.c" \
        -I "$REPO_ROOT/vendor/libgit2/include" \
        -L "$LIBGIT2_BUILD" -lgit2
fi
DYLD_LIBRARY_PATH="$LIBGIT2_BUILD" LD_LIBRARY_PATH="$LIBGIT2_BUILD" \
    "$BENCH_BIN" > "$RESULTS_DIR/libgit2.jsonl"
echo "  -> $RESULTS_DIR/libgit2.jsonl"

# Rust
echo "[2/4] Running Rust benchmarks..."
cd "$REPO_ROOT/rust"
cargo build --release --bin muongit-bench 2>/dev/null
cargo run --release --bin muongit-bench 2>/dev/null > "$RESULTS_DIR/rust.jsonl"
echo "  -> $RESULTS_DIR/rust.jsonl"

# Swift
echo "[3/4] Running Swift benchmarks..."
cd "$REPO_ROOT/swift"
swift build -c release --product muongit-bench 2>/dev/null
swift run -c release muongit-bench 2>/dev/null > "$RESULTS_DIR/swift.jsonl"
echo "  -> $RESULTS_DIR/swift.jsonl"

# Kotlin
echo "[4/4] Running Kotlin benchmarks..."
cd "$REPO_ROOT/kotlin"
./gradlew --console=plain bench 2>/dev/null | grep '"op"' > "$RESULTS_DIR/kotlin.jsonl"
echo "  -> $RESULTS_DIR/kotlin.jsonl"

# Merge all results
cat "$RESULTS_DIR/libgit2.jsonl" "$RESULTS_DIR/rust.jsonl" "$RESULTS_DIR/swift.jsonl" "$RESULTS_DIR/kotlin.jsonl" \
    > "$RESULTS_DIR/all_${TIMESTAMP}.jsonl"

# Generate comparison report
echo ""
echo "=== Benchmark Comparison Report ==="
echo ""
printf "%-25s %12s %12s %12s %12s\n" "Operation" "libgit2 (ms)" "Rust (ms)" "Swift (ms)" "Kotlin (ms)"
printf "%-25s %12s %12s %12s %12s\n" "-------------------------" "------------" "------------" "------------" "------------"

# Parse JSON lines and build comparison table
python3 -c "
import json, sys

results = {}
for path, lang in [('$RESULTS_DIR/libgit2.jsonl', 'libgit2'), ('$RESULTS_DIR/rust.jsonl', 'rust'), ('$RESULTS_DIR/swift.jsonl', 'swift'), ('$RESULTS_DIR/kotlin.jsonl', 'kotlin')]:
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
    libgit2 = '%.3f' % r['libgit2'] if 'libgit2' in r else '-'
    rust = '%.3f' % r['rust'] if 'rust' in r else '-'
    swift = '%.3f' % r['swift'] if 'swift' in r else '-'
    kotlin = '%.3f' % r['kotlin'] if 'kotlin' in r else '-'
    print(f'{op:<25s} {libgit2:>12s} {rust:>12s} {swift:>12s} {kotlin:>12s}')
" 2>/dev/null || echo "(python3 required for comparison table)"

echo ""
echo "Results saved to: \$RESULTS_DIR/all_\${TIMESTAMP}.jsonl"

# Generate Markdown report
echo ""
echo "Generating Markdown report..."
python3 "$REPO_ROOT/benchmarks/generate-report.py"
echo "Done."
