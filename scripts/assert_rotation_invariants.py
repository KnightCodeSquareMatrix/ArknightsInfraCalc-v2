#!/usr/bin/env python3
"""Batch-check persistent operboxes through the Rust rotation invariant gate."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GROUPS = {
    "representative": ROOT / "data/fixtures/box-bundles/collected/collected_operboxes",
    "high-owned": ROOT / "data/fixtures/box-bundles/high-owned",
}


# 每次运行都会变化的耗时字段，比较前剔除。
VOLATILE_KEYS = {"elapsed"}


def canonical_sha(path: Path) -> str | None:
    """规范化后的内容哈希：JSON 去掉耗时字段并按键排序；缺文件返回 None。"""
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text())
    except ValueError:
        return hashlib.sha256(path.read_bytes()).hexdigest()[:16]
    if isinstance(data, dict):
        data = {key: value for key, value in data.items() if key not in VOLATILE_KEYS}
    text = json.dumps(data, ensure_ascii=False, sort_keys=True)
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def box_digests(output: Path) -> dict[str, str | None]:
    return {name: canonical_sha(output / f"{name}.json")
            for name in ("profile", "rotation", "maa")}


def compare_runs(current: Path, previous: Path) -> int:
    """逐盒比较两次运行的规范化产物，打印差异；返回有差异的盒数。"""
    changed = 0
    only_current = only_previous = same = 0
    for group in sorted(p.name for p in current.iterdir() if p.is_dir()):
        for box in sorted(p.name for p in (current / group).iterdir() if p.is_dir()):
            old = previous / group / box
            if not old.is_dir():
                only_current += 1
                continue
            new_digest, old_digest = box_digests(current / group / box), box_digests(old)
            if new_digest == old_digest:
                same += 1
                continue
            changed += 1
            kinds = [k for k in new_digest if new_digest[k] != old_digest[k]]
            print(f"CHANGED {group}/{box}: {','.join(kinds)}")
    for group in sorted(p.name for p in previous.iterdir() if p.is_dir()):
        for box in sorted(p.name for p in (previous / group).iterdir() if p.is_dir()):
            if not (current / group / box).is_dir():
                only_previous += 1
    print(f"compare: same={same} changed={changed} "
          f"only_current={only_current} only_previous={only_previous}")
    return changed


def source_state() -> dict[str, str]:
    def git(*arguments: str) -> str:
        return subprocess.run(["git", *arguments], cwd=ROOT, capture_output=True,
                              text=True).stdout
    return {
        "head": git("rev-parse", "HEAD").strip(),
        "status_sha": hashlib.sha256(git("status", "--porcelain").encode()).hexdigest()[:16],
        "diff_sha": hashlib.sha256(git("diff", "HEAD").encode()).hexdigest()[:16],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--group", choices=["representative", "high-owned", "all"], default="all")
    parser.add_argument("--rotation", default="3")
    parser.add_argument("--layout", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--binary", default="target/debug/infra-cli")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--enable-support-combinations", action="store_true")
    parser.add_argument("--no-fiammetta", action="store_true")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--compare-to", type=Path,
                        help="与此前的 --output-dir 逐盒比较规范化后的产物")
    args = parser.parse_args()
    if args.output_dir:
        args.output_dir.mkdir(parents=True, exist_ok=False)
    groups = GROUPS.items() if args.group == "all" else [(args.group, GROUPS[args.group])]
    binary = Path(args.binary)
    if not binary.is_absolute():
        binary = ROOT / binary
    if not args.skip_build:
        subprocess.run(["cargo", "build", "-p", "infra-cli"], cwd=ROOT, check=True)

    failures: dict[str, list[tuple[str, str, str]]] = {
        "invariant": [],
        "solver": [],
        "input": [],
        "timeout": [],
    }
    total = 0
    completed = 0
    work = [(group, box_path) for group, directory in groups
            for box_path in sorted(directory.glob("*.json"))
            if not box_path.stem.endswith("_profile")]

    def solve(item: tuple[str, Path]) -> tuple[str, str, str, str]:
        group, box_path = item
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            if args.output_dir:
                output = args.output_dir / group / box_path.stem
                output.mkdir(parents=True)
            command = [str(binary), "plan", "--operbox", str(box_path),
                       "--rotation", args.rotation, "--assert-invariants",
                       "--profile-out", str(output / "profile.json")]
            if args.layout:
                command.extend(["--layout", str(args.layout)])
            if args.output_dir:
                command.extend(["--rotation-out", str(output / "rotation.json"),
                                "--maa-out", str(output / "maa.json")])
            if args.enable_support_combinations:
                command.append("--enable-support-combinations")
            if args.no_fiammetta:
                command.append("--no-fiammetta")
            env = os.environ.copy()
            if args.jobs > 1:
                env.setdefault("RAYON_NUM_THREADS", "2")
            try:
                result = subprocess.run(command, cwd=ROOT, capture_output=True,
                                        text=True, timeout=args.timeout, env=env)
                stdout, stderr = result.stdout, result.stderr
                reason = stderr.strip().splitlines()[-1] if stderr.strip() else "failed"
                if result.returncode == 0:
                    category = "completed"
                elif "assert-invariants" in stderr:
                    category = "invariant"
                elif "operbox parse" in reason or "invalid type" in reason:
                    category = "input"
                else:
                    category = "solver"
            except subprocess.TimeoutExpired as error:
                category, reason = "timeout", f"timeout after {args.timeout}s"
                stdout = error.stdout or ""
                stderr = error.stderr or ""
                if isinstance(stdout, bytes):
                    stdout = stdout.decode("utf-8", errors="replace")
                if isinstance(stderr, bytes):
                    stderr = stderr.decode("utf-8", errors="replace")
            if args.output_dir:
                (output / "stdout.log").write_text(stdout)
                (output / "stderr.log").write_text(stderr)
                (output / "command.json").write_text(json.dumps(command, indent=2) + "\n")
                (output / "digests.json").write_text(
                    json.dumps(box_digests(output), indent=2) + "\n")
            return category, group, box_path.name, reason

    with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as executor:
        for category, group, name, reason in executor.map(solve, work):
            total += 1
            if category == "completed":
                completed += 1
            else:
                failures[category].append((group, name, reason))
            print(f"progress={total}/{len(work)} {category} {group}/{name}", flush=True)

    failed = sum(len(items) for items in failures.values())
    print(f"checked={total} completed={completed} failed={failed}")
    for category, items in failures.items():
        print(f"{category}_failures={len(items)}")
        for group, name, reason in items:
            print(f"FAIL {category} {group}/{name}: {reason}")
    if args.output_dir:
        (args.output_dir / "summary.json").write_text(json.dumps({
            "rotation": args.rotation,
            "layout": str(args.layout) if args.layout else None,
            "no_fiammetta": args.no_fiammetta,
            "binary": str(binary),
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "source": source_state(),
            "checked": total,
            "completed": completed,
            "failures": {category: [
                {"group": group, "box": name, "reason": reason}
                for group, name, reason in items
            ] for category, items in failures.items()},
        }, ensure_ascii=False, indent=2) + "\n")
    changed = 0
    if args.compare_to:
        if not args.output_dir:
            parser.error("--compare-to 需要同时指定 --output-dir")
        changed = compare_runs(args.output_dir, args.compare_to)
    return int(bool(failed) or bool(changed))


if __name__ == "__main__":
    sys.exit(main())
