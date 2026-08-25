#!/usr/bin/env python3
"""Create a deterministic, read-only snapshot and disposition ledger."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


class SnapshotError(RuntimeError):
    pass


@dataclass(frozen=True)
class Entry:
    path: str
    kind: str
    mode: int
    size: int
    digest: str
    link_target: str | None = None

    def as_dict(self) -> dict[str, Any]:
        value: dict[str, Any] = {
            "path": self.path,
            "kind": self.kind,
            "mode": self.mode,
            "size": self.size,
            "sha256": self.digest,
        }
        if self.link_target is not None:
            value["link_target"] = self.link_target
        return value


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _repo_path(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _run_git(root: Path, arguments: list[str], input_text: str = "") -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(root), *arguments],
        input=input_text,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return [line for line in result.stdout.splitlines() if line]


def _git_status(root: Path) -> dict[str, set[str]]:
    result = subprocess.run(
        ["git", "-C", str(root), "status", "--porcelain=v1", "-z", "--untracked-files=all"],
        text=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    statuses: dict[str, set[str]] = {}
    fields = result.stdout.decode("utf-8", errors="surrogateescape").split("\0")
    for field in fields:
        if len(field) < 4:
            continue
        code = field[:2]
        path = field[3:]
        statuses.setdefault(path, set())
        if code[0] != " " and code[0] != "?":
            statuses[path].add("staged")
        if code[1] != " " and code[1] != "?":
            statuses[path].add("modified")
        if code == "??":
            statuses[path].add("untracked")
    return statuses


def _ignored_paths(root: Path, paths: Iterable[str]) -> set[str]:
    values = list(paths)
    if not values:
        return set()
    payload = "\0".join(values) + "\0"
    result = subprocess.run(
        ["git", "-C", str(root), "check-ignore", "--no-index", "--stdin", "-z"],
        input=payload,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return set(value for value in result.stdout.split("\0") if value)


def _enumerate(root: Path, excluded: set[Path]) -> list[Path]:
    entries: list[Path] = []

    def visit(directory: Path) -> None:
        try:
            children = sorted(os.scandir(directory), key=lambda item: item.name)
        except OSError as error:
            raise SnapshotError(f"cannot enumerate {directory}: {error}") from error
        for child in children:
            path = Path(child.path)
            if child.name == ".git" or path in excluded:
                continue
            entries.append(path)
            try:
                if child.is_dir(follow_symlinks=False):
                    visit(path)
            except OSError as error:
                raise SnapshotError(f"cannot inspect {path}: {error}") from error

    visit(root)
    return entries


def _entry(path: Path, root: Path) -> Entry:
    before = path.lstat()
    mode = stat.S_IMODE(before.st_mode)
    relative = _repo_path(path, root)
    if stat.S_ISLNK(before.st_mode):
        target = os.readlink(path)
        digest = _sha256_bytes(target.encode("utf-8", errors="surrogateescape"))
        result = Entry(relative, "symlink", mode, len(target), digest, target)
    elif stat.S_ISREG(before.st_mode):
        digest_builder = hashlib.sha256()
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest_builder.update(chunk)
        digest = digest_builder.hexdigest()
        second_digest_builder = hashlib.sha256()
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                second_digest_builder.update(chunk)
        if digest != second_digest_builder.hexdigest():
            raise SnapshotError(f"file content changed while hashing: {relative}")
        result = Entry(relative, "file", mode, before.st_size, digest)
    elif stat.S_ISDIR(before.st_mode):
        result = Entry(relative, "directory", mode, 0, "")
    else:
        result = Entry(relative, "special", mode, before.st_size, "")
    after = path.lstat()
    stable_before = (before.st_dev, before.st_ino, before.st_mode, before.st_nlink, before.st_size, before.st_mtime_ns)
    stable_after = (after.st_dev, after.st_ino, after.st_mode, after.st_nlink, after.st_size, after.st_mtime_ns)
    if stable_before != stable_after:
        raise SnapshotError(f"path changed while hashing: {relative}")
    return result


def _directory_digests(entries: list[Entry]) -> list[Entry]:
    by_path = {entry.path: entry for entry in entries}
    children: dict[str, list[Entry]] = {}
    for entry in entries:
        parent = entry.path.rpartition("/")[0]
        children.setdefault(parent, []).append(entry)
    for path in sorted((entry.path for entry in entries if entry.kind == "directory"), key=lambda item: item.count("/"), reverse=True):
        parts: list[str] = []
        for child in sorted(children.get(path, []), key=lambda item: item.path):
            parts.append(json.dumps(by_path[child.path].as_dict(), ensure_ascii=False, sort_keys=True, separators=(",", ":")))
        directory = by_path[path]
        by_path[path] = Entry(directory.path, directory.kind, directory.mode, len(children.get(path, [])), _sha256_bytes("\n".join(parts).encode("utf-8")))
    return [by_path[entry.path] for entry in entries]


def _snapshot_once(root: Path, output_paths: set[Path]) -> tuple[list[Entry], str]:
    paths = _enumerate(root, output_paths)
    entries = [_entry(path, root) for path in paths]
    entries = _directory_digests(entries)
    current_paths = _enumerate(root, output_paths)
    if [path.as_posix() for path in paths] != [path.as_posix() for path in current_paths]:
        raise SnapshotError("worktree changed while enumerating")
    inventory = "\n".join(json.dumps(entry.as_dict(), ensure_ascii=False, sort_keys=True, separators=(",", ":")) for entry in entries)
    return entries, _sha256_bytes(inventory.encode("utf-8"))


def build_snapshot(root: Path, output: Path, retries: int) -> dict[str, Any]:
    root = root.resolve()
    output = output.resolve()
    ledger_path = output.with_name(output.stem + "-disposition.json")
    excluded = {output, ledger_path}
    for attempt in range(1, retries + 1):
        try:
            entries, inventory_digest = _snapshot_once(root, excluded)
            statuses = _git_status(root)
            ignored = _ignored_paths(root, [entry.path for entry in entries])
            head = _run_git(root, ["rev-parse", "HEAD"])[0]
            break
        except SnapshotError:
            if attempt == retries:
                raise
            time.sleep(0.05)
    else:
        raise SnapshotError("snapshot retries exhausted")

    records: list[dict[str, Any]] = []
    for entry in entries:
        state = set(statuses.get(entry.path, set()))
        if entry.path in ignored:
            state.add("ignored")
            state.add("untracked")
        records.append(
            {
                "path": entry.path,
                "kind": entry.kind,
                "state": sorted(state) or ["tracked-clean"],
                "sha256": entry.digest,
                "owner": "unassigned:user-review-required",
                "disposition": "pending_user_disposition",
                "approval_status": "pending",
                "rollback": "retain unchanged path until signed disposition",
            }
        )
    return {
        "schema_version": 1,
        "snapshot_type": "worktree",
        "repository": str(root),
        "base_sha": head,
        "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "excluded_paths": sorted(path.relative_to(root).as_posix() for path in excluded if path.is_relative_to(root)),
        "inventory_sha256": inventory_digest,
        "entries": [entry.as_dict() for entry in entries],
        "disposition_status": "pending_user_review",
        "entry_count": len(entries),
        "ignored_entry_count": sum("ignored" in record["state"] for record in records),
    }, {"schema_version": 1, "snapshot_inventory_sha256": inventory_digest, "base_sha": head, "entries": records}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--retries", type=int, default=3)
    args = parser.parse_args()
    if args.retries < 1:
        parser.error("--retries must be positive")
    output = args.output.resolve()
    ledger_path = output.with_name(output.stem + "-disposition.json")
    for path in (output, ledger_path):
        if os.path.lexists(path):
            print(f"error: refusing to overwrite existing output: {path}", file=sys.stderr)
            return 2
    try:
        snapshot, ledger = build_snapshot(args.repo_root, args.output, args.retries)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        _write_noclobber(args.output, snapshot)
        _write_noclobber(ledger_path, ledger)
    except (OSError, SnapshotError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print(f"PASS snapshot entries={snapshot['entry_count']} ignored={snapshot['ignored_entry_count']} sha256={snapshot['inventory_sha256']}")
    return 0


def _write_noclobber(path: Path, value: dict[str, Any]) -> None:
    payload = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            descriptor = -1
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, path)
    finally:
        if descriptor != -1:
            os.close(descriptor)
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
