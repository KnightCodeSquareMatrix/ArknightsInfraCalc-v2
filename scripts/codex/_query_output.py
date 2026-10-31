"""Shared JSON loading, source references and bounded query output."""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
from typing import Any


class QueryError(ValueError):
    pass


def pointer(parent: str, key: str | int) -> str:
    return parent + "/" + str(key).replace("~", "~0").replace("/", "~1")


def read_json(path: Path) -> tuple[Any, dict[str, Any]]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw, parse_constant=lambda value: _invalid_number(value))
    except (OSError, UnicodeError, ValueError) as error:
        raise QueryError(f"cannot read JSON {path}: {error}") from error
    return value, {
        "path": str(path.resolve()), "bytes": len(raw),
        "sha256": hashlib.sha256(raw).hexdigest(),
    }


def _invalid_number(value: str) -> None:
    raise ValueError(f"non-finite JSON number: {value}")


def add_output_arguments(parser: Any) -> None:
    parser.add_argument("--max-chars", type=int, default=6000,
                        help="终端 JSON 字符预算（至少 1500，默认 6000）")
    parser.add_argument("--full", action="store_true", help="终端输出完整查询结果")
    parser.add_argument("--output", type=Path, help="把完整查询结果写入文件，终端仍受预算限制")


def emit(payload: dict[str, Any], args: Any, *, input_paths: list[Path] | None = None) -> None:
    if args.max_chars < 1500:
        raise QueryError("--max-chars must be at least 1500")
    rendered = json.dumps(payload, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    if args.output:
        def source_paths(value: Any) -> set[Path]:
            paths: set[Path] = set()
            if isinstance(value, dict):
                for key, child in value.items():
                    if key == "path" and isinstance(child, str) and Path(child).is_file():
                        paths.add(Path(child).resolve())
                    paths.update(source_paths(child))
            elif isinstance(value, list):
                for child in value:
                    paths.update(source_paths(child))
            return paths

        protected = source_paths(payload) | {p.resolve() for p in (input_paths or [])}
        if args.output.resolve() in protected or (args.output.exists() and any(
                args.output.samefile(p) for p in protected if p.exists())):
            raise QueryError("--output must not overwrite an input/source file")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    if args.full or len(rendered) <= args.max_chars:
        print(rendered, end="")
        return
    bounded = copy.deepcopy(payload)
    omissions: dict[str, dict[str, Any]] = {}
    metadata: dict[str, Any] = {
        "truncated": True, "complete_chars": len(rendered),
        "recovery": "rerun with --full or --output <path>; narrow the query to inspect a section",
    }
    if args.output:
        metadata["complete_file"] = str(args.output.resolve())
    bounded["output"] = metadata

    def candidates(value: Any, path: str = "") -> list[tuple[int, Any, Any, str]]:
        found = []
        if isinstance(value, dict):
            for key, child in value.items():
                if path == "" and key in {"output", "sources", "source", "previous_source", "selection"}:
                    continue
                child_path = pointer(path, key)
                inner = candidates(child, child_path)
                if isinstance(child, str) and len(child) > 256:
                    found.append((len(child), value, key, child_path))
                if isinstance(child, list) and child and (len(child) > 1 or not inner):
                    found.append((len(json.dumps(child, ensure_ascii=False)), value, key, child_path))
                found.extend(inner)
        elif isinstance(value, list):
            for index, child in enumerate(value):
                found.extend(candidates(child, pointer(path, index)))
        return found

    while True:
        metadata["omissions"] = list(omissions.values())[:12]
        metadata["omission_paths"] = len(omissions)
        rendered = json.dumps(bounded, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
        if len(rendered) <= args.max_chars:
            break
        available = candidates(bounded)
        if not available:
            # Even source metadata can exceed a tiny budget; retain a recoverable envelope.
            envelope = {"truncated": True, "complete_chars": metadata["complete_chars"],
                        "recovery": metadata["recovery"], "omissions": "query omitted; use recovery command"}
            if args.output:
                envelope["complete_file"] = "the path supplied via --output"
            bounded = {"schema": str(payload.get("schema", "query"))[:80],
                       "status": str(payload.get("status", "ok"))[:80], "output": envelope}
            rendered = json.dumps(bounded, ensure_ascii=False, indent=2) + "\n"
            break
        _, parent, key, path = max(available, key=lambda item: item[0])
        value = parent[key]
        if isinstance(value, list):
            shown = len(value) // 2
            omissions.setdefault(path, {"path": path, "total_items": len(value)})["shown_items"] = shown
            parent[key] = value[:shown]
        else:
            omissions[path] = {"path": path, "total_chars": len(value), "shown_chars": 256}
            parent[key] = value[:256]
    print(rendered, end="")
