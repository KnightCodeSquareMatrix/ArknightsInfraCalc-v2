#!/usr/bin/env python3
"""Index local TXT/Markdown/Workflow evidence and extract original sections."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

if __package__:
    from ._query_output import QueryError, add_output_arguments, emit, read_json
else:
    from _query_output import QueryError, add_output_arguments, emit, read_json


SOURCE_RE = re.compile(r"(?<![\w./-])(?P<path>/?(?:[\w.\-]+/)*[^\s`\[\]()<>:]+\.[\w]+):(?P<line>\d+)(?:[-–](?P<end>\d+))?")
UNKNOWN_RE = re.compile(r"NOT[-_]FOUND|未找到|尚未唯一规定|待裁决|未知项|\bunknowns?\b", re.IGNORECASE)
USER_RE = re.compile(r"^(\d+)\s+(user|assistant|context)\s+(\S+)")


def sections(text: str) -> list[dict[str, Any]]:
    lines = text.splitlines(keepends=True)
    starts = []
    fence: tuple[str, int] | None = None
    for i, raw in enumerate(lines):
        line = raw.rstrip("\r\n")
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})", line)
        if marker:
            token = marker.group(1)
            if fence is None:
                fence = (token[0], len(token))
            elif token[0] == fence[0] and len(token) >= fence[1] and not line[marker.end():].strip():
                fence = None
            continue
        if fence:
            continue
        heading = re.match(r"^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$", line)
        claim = re.match(r"^\[(\d+)\]\s*(.*)$", line)
        if heading:
            starts.append((i, len(heading.group(1)), heading.group(2), None))
        elif claim:
            starts.append((i, 7, claim.group(2), claim.group(1)))
    if not starts:
        starts = [(0, 0, "document", None)]
    elif starts[0][0] > 0:
        starts.insert(0, (0, 0, "preamble", None))
    result = []
    for n, (start, level, title, claim_id) in enumerate(starts):
        end = len(lines)
        for next_start, next_level, _, _ in starts[n + 1:]:
            if level in (0, 7) or next_level <= level:
                end = next_start
                break
        body = "".join(lines[start:end])
        user = USER_RE.match(title)
        identifier = claim_id if claim_id is not None else (user.group(1) if user else f"L{start + 1}")
        entry: dict[str, Any] = {"id": identifier, "title": title, "level": level,
                                 "start_line": start + 1, "end_line": end, "chars": len(body),
                                 "references": [], "explicit_unknown_markers": []}
        if user:
            metadata = ""
            for raw in lines[start + 1:end]:
                if not raw.strip():
                    continue
                if re.match(r"^message `[^`]+` \| part `[^`]+`(?: \||\s*$)", raw):
                    metadata = raw
                break
            flags = {k: v == "True" for k, v in re.findall(
                r"\b(synthetic|ignored|assistant_compaction_summary)=(True|False)\b", metadata)}
            entry["message"] = {"role": user.group(2), "timestamp": user.group(3), "flags": flags}
            for key in ("message", "part"):
                match = re.search(rf"\b{key}\s+`([^`]+)`", metadata)
                if match:
                    entry["message"][key + "_id"] = match.group(1)
        for i in range(start, end):
            for match in SOURCE_RE.finditer(lines[i]):
                entry["references"].append({"report_line": i + 1, "source_path": match.group("path"),
                                             "source_line": int(match.group("line")),
                                             "source_end_line": int(match.group("end")) if match.group("end") else None})
            for match in UNKNOWN_RE.finditer(lines[i]):
                entry["explicit_unknown_markers"].append({"line": i + 1, "text": match.group(0)})
        result.append(entry)
    return result


def documents(path: Path) -> list[dict[str, Any]]:
    try:
        raw = path.read_bytes()
        text = raw.decode("utf-8")
    except (OSError, UnicodeError) as error:
        raise QueryError(f"cannot read report {path}: {error}") from error
    source = {"path": str(path.resolve()), "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}
    if path.suffix == ".json" or (path.suffix == ".output" and text.lstrip().startswith("{")):
        obj, _ = read_json(path)
        if not isinstance(obj, dict) or not isinstance(obj.get("result"), list):
            raise QueryError(f"{path}: Workflow JSON requires result array")
        result = []
        keys = set()
        for i, item in enumerate(obj["result"]):
            if not isinstance(item, dict) or not isinstance(item.get("key"), str) or not isinstance(item.get("report"), str):
                raise QueryError(f"{path}:/result/{i} requires key and report strings")
            if item["key"] in keys:
                raise QueryError(f"{path}: duplicate Workflow key {item['key']}")
            keys.add(item["key"])
            result.append({"source": {**source, "json_pointer": f"/result/{i}/report",
                                      "workflow_key": item["key"], "line_coordinate": "decoded report, one-based"},
                           "text": item["report"]})
        return result
    if path.suffix not in {".md", ".txt", ".output"}:
        raise QueryError(f"unsupported report format: {path}")
    return [{"source": {**source, "line_coordinate": "source file, one-based"}, "text": text}]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument("--dir", type=Path, help="读取直接子文件，不递归")
    choice.add_argument("--file", type=Path)
    parser.add_argument("--source", help="按报告引用的源文件路径精确筛选")
    parser.add_argument("--section", help="原文段 ID；必须同时给 --file，可用 --key 区分 Workflow")
    parser.add_argument("--key", help="Workflow result[].key 精确筛选")
    parser.add_argument("--expected", action="append", default=[], help="预期文件名，可重复；只检查是否存在")
    add_output_arguments(parser)
    args = parser.parse_args()
    if args.section is not None and args.file is None:
        parser.error("--section requires --file")
    if args.expected and args.dir is None:
        parser.error("--expected requires --dir")
    try:
        if args.dir:
            if not args.dir.is_dir():
                raise QueryError(f"report directory does not exist: {args.dir}")
            paths = sorted(p for p in args.dir.iterdir() if p.is_file() and
                           p.suffix in {".md", ".txt", ".json", ".output"})
        else:
            paths = [args.file]
        if any(Path(name).name != name for name in args.expected):
            raise QueryError("--expected accepts filenames, not paths")
        missing = [name for name in args.expected if not (args.dir / name).is_file()]
        docs = [doc for path in paths for doc in documents(path)]
        if args.key is not None:
            docs = [doc for doc in docs if doc["source"].get("workflow_key") == args.key]
        indexed, extracted = [], []
        for doc in docs:
            entries = sections(doc["text"])
            if args.source:
                entries = [s for s in entries if any(r["source_path"] == args.source for r in s["references"])]
            if args.section is not None:
                entries = [s for s in entries if s["id"] == args.section]
                lines = doc["text"].splitlines(keepends=True)
                extracted.extend({"source": doc["source"], "section": s,
                                  "text": "".join(lines[s["start_line"] - 1:s["end_line"]])} for s in entries)
            indexed.append({"source": doc["source"], "line_count": len(doc["text"].splitlines()),
                            "chars": len(doc["text"]), "section_count": len(entries), "sections": entries})
        if args.section is not None and len(extracted) > 1:
            raise QueryError("ambiguous section ID; select Workflow --key or a unique heading ID")
        match_count = len(extracted) if args.section is not None else sum(x["section_count"] for x in indexed)
        payload: dict[str, Any] = {"schema": "research-index-v1",
                                  "status": "missing_expected" if missing else ("ok" if match_count else "no_match"),
                                  "selection": {"source": args.source, "section": args.section, "key": args.key},
                                  "document_count": len(docs), "match_count": match_count,
                                  "missing_expected_files": missing,
                                  "scope": "mechanical index; no correctness, contradiction or decision-precedence judgment"}
        if args.section is not None:
            payload["extracted"] = extracted
        else:
            payload["documents"] = indexed
        emit(payload, args, input_paths=paths)
        return int(payload["status"] != "ok")
    except (QueryError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
