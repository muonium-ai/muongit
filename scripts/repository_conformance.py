#!/usr/bin/env python3
"""Cross-implementation repository conformance matrix runner."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
TMP_ROOT = REPO_ROOT / "tmp" / "repository_conformance"
FIXTURE_SCRIPT = REPO_ROOT / "scripts" / "git_remote_fixture.py"
GRADLE_HOME = REPO_ROOT / "tmp" / "gradle-home"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the cross-implementation repository conformance matrix."
    )
    parser.add_argument(
        "--writers",
        default="rust,swift,kotlin",
        help="Comma-separated writer implementations (default: rust,swift,kotlin)",
    )
    parser.add_argument(
        "--readers",
        default="rust,swift,kotlin",
        help="Comma-separated reader implementations (default: rust,swift,kotlin)",
    )
    parser.add_argument(
        "--keep",
        action="store_true",
        help="Keep tmp artifacts instead of deleting any prior run directory first.",
    )
    return parser.parse_args()


def helper_command(implementation: str, *args: str) -> tuple[list[str], dict[str, str]]:
    env = os.environ.copy()

    if implementation == "rust":
        return (
            [
                "cargo",
                "run",
                "--manifest-path",
                str(REPO_ROOT / "rust" / "Cargo.toml"),
                "--quiet",
                "--bin",
                "muongit-conformance",
                "--",
                *args,
            ],
            env,
        )

    if implementation == "swift":
        return (
            [
                "swift",
                "run",
                "--package-path",
                str(REPO_ROOT / "swift"),
                "muongit-conformance",
                *args,
            ],
            env,
        )

    if implementation == "kotlin":
        env["GRADLE_USER_HOME"] = str(GRADLE_HOME)
        env["MUONGIT_CONFORMANCE_ARGS"] = "\n".join(args)
        return (
            [
                str(REPO_ROOT / "kotlin" / "gradlew"),
                "-p",
                str(REPO_ROOT / "kotlin"),
                "--console=plain",
                "runConformance",
            ],
            env,
        )

    raise ValueError(f"unknown implementation: {implementation}")


def run_helper(implementation: str, *args: str) -> str:
    cmd, env = helper_command(implementation, *args)
    completed = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        env=env,
        text=True,
        capture_output=True,
    )
    if completed.returncode != 0:
        sys.stderr.write(f"\n[{implementation}] {' '.join(args)} failed\n")
        if completed.stdout:
            sys.stderr.write(completed.stdout)
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    return completed.stdout


def parse_json_output(raw: str) -> object:
    text = raw.strip()
    start = text.find("{")
    if start < 0:
        raise RuntimeError(f"helper did not emit JSON: {raw}")
    decoder = json.JSONDecoder()
    value, _ = decoder.raw_decode(text[start:])
    return value


def compare_snapshots(
    writer: str,
    reader: str,
    checkpoint: dict[str, str],
    expected: dict[str, object],
    actual: dict[str, object],
) -> None:
    if actual == expected:
        return

    checkpoint_name = checkpoint["name"]
    print(
        f"\nMismatch for checkpoint '{checkpoint_name}': writer={writer}, reader={reader}",
        file=sys.stderr,
    )
    print("Expected:", file=sys.stderr)
    print(json.dumps(expected, indent=2, sort_keys=True), file=sys.stderr)
    print("Actual:", file=sys.stderr)
    print(json.dumps(actual, indent=2, sort_keys=True), file=sys.stderr)
    raise SystemExit(1)


def ensure_root(keep: bool) -> None:
    if TMP_ROOT.exists() and not keep:
        shutil.rmtree(TMP_ROOT)
    TMP_ROOT.mkdir(parents=True, exist_ok=True)


def main() -> int:
    args = parse_args()
    writers = [item for item in args.writers.split(",") if item]
    readers = [item for item in args.readers.split(",") if item]
    ensure_root(args.keep)

    print("=== Repository Conformance Matrix ===")
    print(f"tmp root: {TMP_ROOT}")
    print()

    for writer in writers:
        writer_root = TMP_ROOT / writer
        if writer_root.exists() and not args.keep:
            shutil.rmtree(writer_root)
        writer_root.mkdir(parents=True, exist_ok=True)

        print(f"[writer={writer}] authoring scenario")
        manifest = parse_json_output(
            run_helper(writer, "write-scenario", str(writer_root), str(FIXTURE_SCRIPT))
        )
        checkpoints = manifest["checkpoints"]

        for checkpoint in checkpoints:
            repo_path = checkpoint["repo"]
            expected = parse_json_output(run_helper(writer, "snapshot", repo_path))
            for reader in readers:
                print(f"  - reader={reader} checkpoint={checkpoint['name']}")
                actual = parse_json_output(run_helper(reader, "snapshot", repo_path))
                compare_snapshots(writer, reader, checkpoint, expected, actual)

    print()
    print("Conformance matrix passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
