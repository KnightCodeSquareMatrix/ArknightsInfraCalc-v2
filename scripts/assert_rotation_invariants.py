#!/usr/bin/env python3
"""Batch-check persistent operboxes through the Rust rotation invariant gate."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GROUPS = {
    "representative": ROOT / "data/fixtures/box-bundles/collected/collected_operboxes",
    "high-owned": ROOT / "data/fixtures/box-bundles/high-owned",
}
REPOSITORY_BOX_DIR = "data/fixtures/collected_operboxes/collected_operboxes"
HIGH_OWNED_MIN = 300


class BatchProgress:
    """A recorded snapshot, not a process-liveness probe."""

    def __init__(self, output: Path | None, total: int, interval: float):
        self.output = output
        self.started = time.monotonic()
        self.last_print = self.started
        self.interval = interval
        self.lock = threading.Lock()
        self.value = {
            "schema_version": 1, "state": "running", "phase": "build",
            "total": total, "checked": 0, "completed": 0,
            "failure_counts": {k: 0 for k in ("invariant", "solver", "input", "timeout")},
            "last_result": None, "exit_code": None, "error": None, "changed": None,
            "started_at": datetime.now(timezone.utc).isoformat(),
            "output_dir": str(output.resolve()) if output else None,
            "summary_path": str((output / "summary.json").resolve()) if output else None,
        }
        self.update()

    def _write(self) -> None:
        self.value["updated_at"] = datetime.now(timezone.utc).isoformat()
        self.value["elapsed_seconds"] = round(time.monotonic() - self.started, 3)
        if self.output:
            temporary = None
            try:
                with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=self.output,
                                                 prefix=".progress-", delete=False) as handle:
                    temporary = Path(handle.name)
                    json.dump(self.value, handle, ensure_ascii=False, indent=2, allow_nan=False)
                    handle.write("\n")
                os.replace(temporary, self.output / "progress.json")
            finally:
                if temporary and temporary.exists():
                    temporary.unlink()

    def update(self, **values) -> None:
        with self.lock:
            self.value.update(values)
            self._write()

    def record(self, result: tuple[str, str, str, str]) -> None:
        category, group, name, _ = result
        with self.lock:
            self.value["checked"] += 1
            if category == "completed":
                self.value["completed"] += 1
            else:
                self.value["failure_counts"][category] += 1
            self.value["last_result"] = {"category": category, "group": group, "box": name}
            self._write()
            now = time.monotonic()
            if now - self.last_print >= self.interval:
                self.last_print = now
                print(f"progress={self.value['checked']}/{self.value['total']} "
                      f"completed={self.value['completed']} "
                      f"failed={sum(self.value['failure_counts'].values())}", flush=True)


def read_progress(directory: Path) -> dict:
    value = json.loads((directory / "progress.json").read_text(encoding="utf-8"))
    if (not isinstance(value, dict) or value.get("schema_version") != 1 or
            value.get("state") not in {"running", "completed", "failed"} or
            value.get("phase") not in {"build", "solve", "compare", "finished"}):
        raise ValueError("unsupported progress schema/state/phase")
    for key in ("total", "checked", "completed"):
        if type(value.get(key)) is not int or value[key] < 0:
            raise ValueError(f"invalid progress {key}")
    counts = value.get("failure_counts")
    if (not isinstance(counts, dict) or set(counts) != {"invariant", "solver", "input", "timeout"}
            or any(type(v) is not int or v < 0 for v in counts.values()) or
            value["checked"] != value["completed"] + sum(counts.values()) or
            not value["completed"] <= value["checked"] <= value["total"]):
        raise ValueError("inconsistent progress counters")
    if value["state"] == "running" and value.get("exit_code") is not None:
        raise ValueError("running progress cannot have a final exit_code")
    if value["state"] != "running" and type(value.get("exit_code")) is not int:
        raise ValueError("terminal progress requires exit_code")
    if value["state"] == "completed" and (value["phase"] != "finished" or
            value["checked"] != value["total"] or sum(counts.values()) or value["exit_code"] != 0):
        raise ValueError("completed progress must be a complete successful batch")
    if value["state"] == "failed" and value["exit_code"] == 0:
        raise ValueError("failed progress requires nonzero exit_code")
    elapsed = value.get("elapsed_seconds")
    if type(elapsed) not in (int, float) or not math.isfinite(elapsed) or elapsed < 0:
        raise ValueError("invalid elapsed_seconds")
    if not isinstance(value.get("updated_at"), str):
        raise ValueError("missing updated_at")
    return value


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
    parser.add_argument("--box-source", choices=["persistent", "repository"], default="persistent",
                        help="persistent 使用本地持久集合；repository 使用 Git 跟踪的既有样本并按实际持有数筛选 high-owned")
    parser.add_argument("--rotation", default="3")
    parser.add_argument("--layout", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--binary", default="target/release/infra-cli")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--enable-support-combinations", action="store_true")
    parser.add_argument("--no-fiammetta", action="store_true")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--compare-to", type=Path,
                        help="与此前的 --output-dir 逐盒比较规范化后的产物")
    parser.add_argument("--progress-interval", type=float, default=15,
                        help="进度终端输出的最小间隔秒数；progress.json 每盒完成更新")
    parser.add_argument("--status", type=Path,
                        help="只读取指定运行目录的 progress.json，不构建或求解")
    args = parser.parse_args()
    if args.status:
        try:
            print(json.dumps(read_progress(args.status), ensure_ascii=False, indent=2))
            return 0
        except (OSError, ValueError) as error:
            print(f"progress error: {error}", file=sys.stderr)
            return 2
    if args.progress_interval < 0 or not math.isfinite(args.progress_interval):
        parser.error("--progress-interval must be finite and non-negative")
    if args.compare_to and not args.output_dir:
        parser.error("--compare-to 需要同时指定 --output-dir")
    if args.output_dir:
        args.output_dir.mkdir(parents=True, exist_ok=False)
    binary = Path(args.binary)
    if not binary.is_absolute():
        binary = ROOT / binary

    progress = BatchProgress(args.output_dir, 0, args.progress_interval)
    try:
        selected_groups = list(GROUPS) if args.group == "all" else [args.group]
        if args.box_source == "repository":
            tracked = subprocess.check_output(
                ["git", "ls-files", "-z", "--", REPOSITORY_BOX_DIR], cwd=ROOT
            ).decode("utf-8").split("\0")
            boxes = sorted(ROOT / path for path in tracked
                           if path.endswith(".json") and not Path(path).stem.endswith("_profile"))
            high_owned = [path for path in boxes
                          if sum(operator.get("own", True) for operator in
                                 json.loads(path.read_text(encoding="utf-8"))) >= HIGH_OWNED_MIN]
            sources = {"representative": boxes, "high-owned": high_owned}
        else:
            sources = {group: sorted(path for path in GROUPS[group].glob("*.json")
                                    if not path.stem.endswith("_profile"))
                       for group in selected_groups}
        missing = [group for group in selected_groups if not sources[group]]
        if missing:
            raise ValueError(f"no operboxes in requested groups: {', '.join(missing)} "
                             f"(box source: {args.box_source})")
        work = [(group, path) for group in selected_groups for path in sources[group]]
        progress.update(total=len(work))
        if not args.skip_build:
            subprocess.run(["cargo", "build", "--release", "-p", "infra-cli"], cwd=ROOT, check=True)
        return run_batch(args, binary, work, progress)
    except (Exception, KeyboardInterrupt) as error:
        progress.update(state="failed", error=f"{type(error).__name__}: {error}", exit_code=2)
        print(f"batch error: {error}", file=sys.stderr)
        return 2


def run_batch(args, binary: Path, work: list[tuple[str, Path]], progress: BatchProgress) -> int:
    progress.update(phase="solve")

    failures: dict[str, list[tuple[str, str, str]]] = {
        "invariant": [],
        "solver": [],
        "input": [],
        "timeout": [],
    }
    total = 0
    completed = 0

    def solve(item: tuple[str, Path]) -> tuple[str, str, str, str]:
        group, box_path = item
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            if args.output_dir:
                output = args.output_dir / group / box_path.stem
                output.mkdir(parents=True)
            command = [str(binary), "plan", "--operbox", str(box_path),
                       "--rotation", args.rotation, "--assert-invariants"]
            if not args.rotation.startswith("auto@"):
                command.extend(["--profile-out", str(output / "profile.json")])
            if args.layout:
                command.extend(["--layout", str(args.layout)])
            if args.output_dir:
                command.extend(["--rotation-out", str(output / "rotation.json")])
                if not args.rotation.startswith("auto@"):
                    command.extend(["--maa-out", str(output / "maa.json")])
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

    def tracked_solve(item):
        result = solve(item)
        progress.record(result)
        return result

    with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as executor:
        for category, group, name, reason in executor.map(tracked_solve, work):
            total += 1
            if category == "completed":
                completed += 1
            else:
                failures[category].append((group, name, reason))

    failed = sum(len(items) for items in failures.values())
    print(f"checked={total} completed={completed} failed={failed}")
    for category, items in failures.items():
        print(f"{category}_failures={len(items)}")
        for group, name, reason in items:
            print(f"FAIL {category} {group}/{name}: {reason}")
    if args.output_dir:
        (args.output_dir / "summary.json").write_text(json.dumps({
            "rotation": args.rotation,
            "box_source": args.box_source,
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
        progress.update(phase="compare")
        changed = compare_runs(args.output_dir, args.compare_to)
    result = int(bool(failed) or bool(changed))
    progress.update(state="failed" if result else "completed", phase="finished",
                    exit_code=result, changed=changed)
    return result


if __name__ == "__main__":
    sys.exit(main())
