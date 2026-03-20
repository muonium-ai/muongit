#!/usr/bin/env python3
"""Generate a human-readable Markdown benchmark comparison report."""

import argparse
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone


def get_machine_info():
    """Gather machine info for the report header."""
    info = {}
    info["platform"] = platform.system()
    info["arch"] = platform.machine()
    info["python"] = platform.python_version()

    # CPU info
    if platform.system() == "Darwin":
        try:
            brand = subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"],
                text=True,
            ).strip()
            info["cpu"] = brand
        except Exception:
            info["cpu"] = platform.processor() or "unknown"
        try:
            mem = subprocess.check_output(
                ["sysctl", "-n", "hw.memsize"], text=True
            ).strip()
            info["memory_gb"] = f"{int(mem) / (1024**3):.0f}"
        except Exception:
            info["memory_gb"] = "?"
    else:
        info["cpu"] = platform.processor() or "unknown"
        info["memory_gb"] = "?"

    # Git commit
    try:
        info["git_commit"] = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"], text=True
        ).strip()
    except Exception:
        info["git_commit"] = "unknown"

    return info


def load_results(results_dir):
    """Load JSONL results for all languages."""
    results = {}
    langs = ["libgit2", "rust", "swift", "kotlin"]

    for lang in langs:
        path = os.path.join(results_dir, f"{lang}.jsonl")
        if not os.path.exists(path):
            continue
        for line in open(path):
            line = line.strip()
            if not line:
                continue
            try:
                d = json.loads(line)
            except Exception:
                continue
            op = d["op"]
            if op not in results:
                results[op] = {}
            results[op][lang] = {
                "median_ms": d.get("median_ms", 0),
                "mean_ms": d.get("mean_ms", 0),
                "min_ms": d.get("min_ms", 0),
                "iterations": d.get("iterations", 0),
                "ops_per_sec": d.get("ops_per_sec", 0),
            }

    return results


def fmt_ms(v):
    """Format milliseconds for display."""
    if v is None:
        return "-"
    if v < 0.001:
        return "<0.001"
    if v >= 1000:
        return f"{v:,.0f}"
    return f"{v:.3f}"


def fmt_ratio(muon_ms, libgit2_ms):
    """Format ratio vs libgit2."""
    if libgit2_ms is None or muon_ms is None:
        return ""
    if libgit2_ms < 0.0001:
        return ""
    ratio = muon_ms / libgit2_ms
    if ratio < 0.01:
        return "<0.01x"
    if ratio < 0.1:
        return f"{ratio:.2f}x"
    if ratio < 1.0:
        return f"{ratio:.1f}x"
    if ratio < 10:
        return f"{ratio:.1f}x"
    return f"{ratio:.0f}x"


# Operations grouped by category
OP_GROUPS = [
    (
        "SHA Hashing",
        [
            "sha1_10kb",
            "sha256_10kb",
            "sha1_1mb",
            "sha256_1mb",
            "sha1_10mb",
            "sha256_10mb",
        ],
    ),
    (
        "OID Operations",
        ["oid_cmp_256x16k", "oid_create_1k", "oid_create_10k", "oid_create_100k"],
    ),
    ("Blob Hashing", ["blob_hash_1k", "blob_hash_10k"]),
    ("Tree Serialization", ["tree_serialize_1k", "tree_serialize_10k"]),
    ("Commit Serialization", ["commit_serialize_10k"]),
    ("Index Read/Write", ["index_rw_1k", "index_rw_10k"]),
    ("Tree Diff", ["diff_1k", "diff_10k"]),
]

LANGS = ["libgit2", "rust", "swift", "kotlin"]
LANG_LABELS = {"libgit2": "libgit2 (C)", "rust": "Rust", "swift": "Swift", "kotlin": "Kotlin"}


def generate_report(results, machine_info, timestamp):
    """Generate Markdown report string."""
    lines = []

    lines.append("# MuonGit Benchmark Report")
    lines.append("")
    lines.append(f"**Date:** {timestamp.strftime('%Y-%m-%d %H:%M:%S %Z')}")
    lines.append(f"**Commit:** `{machine_info.get('git_commit', '?')}`")
    lines.append(
        f"**Machine:** {machine_info.get('cpu', '?')} "
        f"({machine_info.get('arch', '?')}, "
        f"{machine_info.get('memory_gb', '?')} GB RAM)"
    )
    lines.append(
        f"**Platform:** {machine_info.get('platform', '?')} "
        f"(Python {machine_info.get('python', '?')})"
    )
    lines.append("")
    lines.append("---")
    lines.append("")

    # Summary table
    lines.append("## Summary")
    lines.append("")
    lines.append("All times in **milliseconds** (median). Lower is better.")
    lines.append("")

    header = f"| {'Operation':<25s} |"
    sep = f"| {'-'*25} |"
    for lang in LANGS:
        header += f" {LANG_LABELS[lang]:>12s} |"
        sep += f" {'-'*12}:|"
    header += f" {'vs libgit2':>10s} |"
    sep += f" {'-'*10}:|"

    lines.append(header)
    lines.append(sep)

    for group_name, ops in OP_GROUPS:
        for op in ops:
            r = results.get(op, {})
            row = f"| {op:<25s} |"
            medians = {}
            for lang in LANGS:
                if lang in r:
                    v = r[lang]["median_ms"]
                    medians[lang] = v
                    row += f" {fmt_ms(v):>12s} |"
                else:
                    row += f" {'-':>12s} |"

            # Best muongit ratio vs libgit2
            ratio = ""
            if "libgit2" in medians:
                muon_vals = [
                    (medians[l], l)
                    for l in ["rust", "swift", "kotlin"]
                    if l in medians and medians[l] > 0
                ]
                if muon_vals:
                    best_val, best_lang = min(muon_vals, key=lambda x: x[0])
                    ratio = fmt_ratio(best_val, medians["libgit2"])

            row += f" {ratio:>10s} |"
            lines.append(row)

    lines.append("")

    # Performance gaps section
    lines.append("## Performance Gaps vs libgit2")
    lines.append("")

    gaps = []
    for _, ops in OP_GROUPS:
        for op in ops:
            r = results.get(op, {})
            if "libgit2" not in r:
                continue
            lg2 = r["libgit2"]["median_ms"]
            if lg2 < 0.0001:
                continue
            for lang in ["rust", "swift", "kotlin"]:
                if lang not in r:
                    continue
                v = r[lang]["median_ms"]
                if v < 0.001:
                    continue
                ratio = v / lg2
                if ratio > 5:
                    gaps.append((ratio, op, lang, v, lg2))

    gaps.sort(reverse=True)
    if gaps:
        lines.append(
            "Operations where muongit is **>5x slower** than libgit2 (sorted by gap):"
        )
        lines.append("")
        lines.append(f"| {'Operation':<25s} | {'Lang':<8s} | {'muongit (ms)':>12s} | {'libgit2 (ms)':>12s} | {'Ratio':>8s} |")
        lines.append(f"| {'-'*25} | {'-'*8} | {'-'*12}:| {'-'*12}:| {'-'*8}:|")
        for ratio, op, lang, v, lg2 in gaps:
            lines.append(
                f"| {op:<25s} | {lang:<8s} | {fmt_ms(v):>12s} | {fmt_ms(lg2):>12s} | {ratio:>7.0f}x |"
            )
        lines.append("")
    else:
        lines.append("No operations >5x slower than libgit2.")
        lines.append("")

    # Wins section
    lines.append("## Where muongit matches or beats libgit2")
    lines.append("")

    wins = []
    for _, ops in OP_GROUPS:
        for op in ops:
            r = results.get(op, {})
            if "libgit2" not in r:
                continue
            lg2 = r["libgit2"]["median_ms"]
            if lg2 < 0.0001:
                continue
            for lang in ["rust", "swift", "kotlin"]:
                if lang not in r:
                    continue
                v = r[lang]["median_ms"]
                ratio = v / lg2 if lg2 > 0 else 999
                if ratio <= 2.0:
                    wins.append((ratio, op, lang, v, lg2))

    wins.sort()
    if wins:
        lines.append("Operations where muongit is **within 2x** of libgit2:")
        lines.append("")
        lines.append(f"| {'Operation':<25s} | {'Lang':<8s} | {'muongit (ms)':>12s} | {'libgit2 (ms)':>12s} | {'Ratio':>8s} |")
        lines.append(f"| {'-'*25} | {'-'*8} | {'-'*12}:| {'-'*12}:| {'-'*8}:|")
        for ratio, op, lang, v, lg2 in wins:
            lines.append(
                f"| {op:<25s} | {lang:<8s} | {fmt_ms(v):>12s} | {fmt_ms(lg2):>12s} | {ratio:>7.1f}x |"
            )
        lines.append("")

    lines.append("---")
    lines.append("")
    lines.append(
        f"*Generated by `benchmarks/generate-report.py` on "
        f"{timestamp.strftime('%Y-%m-%d %H:%M:%S')}*"
    )
    lines.append("")

    return "\n".join(lines)


def parse_args():
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(
        description="Generate a benchmark report from one run directory."
    )
    parser.add_argument(
        "--results-dir",
        required=True,
        help="Directory containing per-language JSONL benchmark results.",
    )
    parser.add_argument(
        "--output",
        required=True,
        help="Output path for the generated Markdown report.",
    )
    parser.add_argument(
        "--timestamp",
        help="UTC timestamp for the run in YYYYMMDD_HHMMSS format.",
    )
    return parser.parse_args()


def parse_timestamp(raw_timestamp):
    """Parse a benchmark run timestamp."""
    if raw_timestamp is None:
        return datetime.now(timezone.utc)
    return datetime.strptime(raw_timestamp, "%Y%m%d_%H%M%S").replace(
        tzinfo=timezone.utc
    )


def main():
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    args = parse_args()
    results_dir = os.path.abspath(args.results_dir)
    output_path = os.path.abspath(args.output)

    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    timestamp = parse_timestamp(args.timestamp)

    results = load_results(results_dir)
    if not results:
        print("Error: No benchmark results found in", results_dir, file=sys.stderr)
        sys.exit(1)

    machine_info = get_machine_info()
    report = generate_report(results, machine_info, timestamp)

    with open(output_path, "w") as f:
        f.write(report)

    rel_output_path = os.path.relpath(output_path, repo_root)
    print(f"Report saved to: {output_path}")
    print(f"Relative path:   {rel_output_path}")

    # Print to stdout too
    print()
    print(report)


if __name__ == "__main__":
    main()
