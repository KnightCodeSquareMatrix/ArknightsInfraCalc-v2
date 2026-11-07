#!/usr/bin/env python3
"""Local source retrieval: lifecycle, navigation, lexical ranking and parent context."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import time

from _query_output import QueryError, add_output_arguments, emit
import doc_layout
from docs_inventory import InventoryError, parse_document, split_values

REPO = Path(__file__).resolve().parents[2]
SCHEMA = 1
HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
DECL = re.compile(
    r"^(\s*)(?:(?:pub(?:\([^)]*\))?|unsafe|async|const(?=\s+fn)|extern\s+\"[^\"]*\")\s+)*"
    r"(fn|struct|enum|trait|type|const|static|impl)\s+([^\s{(=;]+)"
)
HISTORICAL = "<!-- knowledge:historical -->"


def git(*args: str, root: Path = REPO) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def terms(text: str) -> Counter:
    tokens = re.findall(r"[a-z0-9_]+|[\u3400-\u9fff]+", text.lower())
    result: list[str] = []
    for token in tokens:
        if re.fullmatch(r"[\u3400-\u9fff]+", token):
            result.extend(token)
            result.extend(token[i:i + 2] for i in range(len(token) - 1))
        else:
            result.append(token)
    return Counter(result)


def safe_path(value: str) -> Path:
    path = REPO / value
    if Path(value).is_absolute() or ".." in Path(value).parts:
        raise QueryError(f"not a repository path: {value}")
    if not path.resolve().is_relative_to(REPO):
        raise QueryError(f"source escapes repository: {value}")
    if not path.is_file():
        raise QueryError(f"source not found: {value}")
    return path


def chunk_id(path: str, trail: str) -> str:
    return f"{path}::{trail}"


def markdown_chunks(path: str, text: str) -> list[dict]:
    document = parse_document(REPO, REPO / path, text)
    lines = text.splitlines(keepends=True)
    headings: list[dict] = []
    stack: list[dict] = []
    counts: Counter = Counter()
    fenced = False
    fence = ""
    pending_history = False
    for index, line in enumerate(lines):
        stripped = line.lstrip()
        if stripped.startswith(("```", "~~~")):
            marker = stripped[:3]
            if not fenced:
                fenced, fence = True, marker
            elif marker == fence:
                fenced = False
            continue
        if fenced:
            continue
        if line.strip() == HISTORICAL:
            pending_history = True
            continue
        match = HEADING.match(line.rstrip())
        if not match:
            continue
        level, title = len(match[1]), match[2]
        while stack and stack[-1]["level"] >= level:
            stack.pop()["parent_end"] = index
        trail = "/".join([h["heading"] for h in stack] + [title])
        counts[trail] += 1
        suffix = f"~{counts[trail]}" if counts[trail] > 1 else ""
        historical = pending_history or any(h["historical"] for h in stack)
        current_role = document.role in {"canonical", "current-reference"}
        node = {
            "id": chunk_id(path, trail + suffix), "path": path, "kind": "doc",
            "heading": title, "trail": trail, "level": level, "start": index,
            "parent_id": stack[-1]["id"] if stack else None,
            "parent_start": stack[-1]["start"] if stack else index,
            "parent_end": len(lines), "historical": historical,
            "role": document.role, "status": document.status,
            "section_status": "historical" if historical else (
                "current" if current_role else document.status),
            "metadata": document.metadata,
        }
        headings.append(node)
        stack.append(node)
        pending_history = False
    for index, node in enumerate(headings):
        node["end"] = headings[index + 1]["start"] if index + 1 < len(headings) else len(lines)
        # Search direct sections only: a current parent must not index historical children.
        node["text"] = "".join(lines[node["start"]:node["end"]])
        node["terms"] = dict(terms(node["heading"] + "\n" + node["text"]))
        node.pop("level")
    return headings


def code_chunks(path: str, text: str) -> list[dict]:
    lines = text.splitlines(keepends=True)
    starts = [(i, m) for i, line in enumerate(lines) if (m := DECL.match(line))]
    result: list[dict] = []
    counts: Counter = Counter()
    for position, (start, match) in enumerate(starts):
        symbol = match[3]
        label = f"{match[2]} {symbol}"
        counts[label] += 1
        suffix = f"~{counts[label]}" if counts[label] > 1 else ""
        end = min(starts[position + 1][0] if position + 1 < len(starts) else len(lines), start + 180)
        prelude = start
        while prelude and lines[prelude - 1].lstrip().startswith(("///", "//!", "#[")):
            prelude -= 1
        content = "".join(lines[prelude:end])
        result.append({
            "id": chunk_id(path, label + suffix), "path": path, "kind": "code",
            "heading": label, "symbol": symbol, "start": prelude, "end": end,
            "parent_start": prelude, "parent_end": end, "parent_id": None,
            "historical": False, "role": "code", "status": "working-tree",
            "section_status": "current", "metadata": {}, "text": content,
            "terms": dict(terms(label + "\n" + content)),
            "boundary": "regex declaration excerpt, at most 180 lines; not AST/call proof",
        })
    return result


def navigation() -> tuple[dict, list[Path]]:
    nav_path = safe_path(doc_layout.KNOWLEDGE)
    routes_path = safe_path(doc_layout.ROUTES)
    nav = json.loads(nav_path.read_text())
    routes = json.loads(routes_path.read_text())
    if nav.get("schema") != 1 or not isinstance(nav.get("cards"), list):
        raise QueryError("invalid knowledge navigation schema")
    nav["routes"] = routes["routes"]
    nav["vault_commit"] = routes["vault_commit"]
    return nav, [nav_path, routes_path]


def source_index(use_cache: bool) -> tuple[list[dict], dict, list[Path], dict]:
    paths = sorted([*REPO.joinpath("docs").rglob("*.md"),
                    *REPO.joinpath("crates").rglob("*.rs")])
    cache_path = REPO / "target/codex-runs/knowledge-index.json"
    old: dict = {}
    parser_hash = digest(
        Path(__file__).read_bytes()
        + (REPO / "scripts/codex/docs_inventory.py").read_bytes()
        + (REPO / "scripts/codex/doc_layout.py").read_bytes()
    )
    if use_cache and cache_path.is_file():
        try:
            candidate = json.loads(cache_path.read_text())
            if candidate.get("schema") == SCHEMA and candidate.get("parser_sha256") == parser_hash:
                old = candidate.get("files", {})
        except (OSError, ValueError):
            pass
    files: dict = {}
    chunks: list[dict] = []
    reused = 0
    for path in paths:
        relative = path.relative_to(REPO).as_posix()
        raw = safe_path(relative).read_bytes()
        sha = digest(raw)
        saved = old.get(relative)
        if isinstance(saved, dict) and saved.get("sha256") == sha and isinstance(saved.get("chunks"), list):
            entries = saved["chunks"]
            reused += 1
        else:
            content = raw.decode("utf-8")
            entries = markdown_chunks(relative, content) if path.suffix == ".md" else code_chunks(relative, content)
        files[relative] = {"sha256": sha, "bytes": len(raw), "chunks": entries}
        chunks.extend(entries)
    if use_cache and (reused != len(files) or old.keys() != files.keys()):
        cache_path.parent.mkdir(parents=True, exist_ok=True)
        # One writer per project. Queries may race: replacement never exposes partial JSON.
        import tempfile
        with tempfile.NamedTemporaryFile(mode="w", dir=cache_path.parent, encoding="utf-8", delete=False) as handle:
            json.dump({"schema": SCHEMA, "parser_sha256": parser_hash, "files": files}, handle, ensure_ascii=False)
            temporary = Path(handle.name)
        temporary.replace(cache_path)
    stats = {"files": len(files), "chunks": len(chunks), "cache_reused": reused,
             "rebuilt": len(files) - reused, "cache": str(cache_path) if use_cache else None}
    return chunks, files, paths, stats


def source_identity(path: str, files: dict, dirty: set[str]) -> dict:
    record = files[path]
    return {"path": str(safe_path(path)), "repo_path": path, "sha256": record["sha256"],
            "bytes": record["bytes"], "dirty": path in dirty}


def render_chunk(node: dict, files: dict, dirty: set[str]) -> dict:
    raw = safe_path(node["path"]).read_bytes()
    if digest(raw) != files[node["path"]]["sha256"]:
        raise QueryError("source changed during query; rerun to refresh index")
    text = "".join(raw.decode().splitlines(keepends=True)[node["start"]:node["end"]])
    return {
        **{key: node.get(key) for key in ("id", "kind", "heading", "role", "status", "section_status")},
        "source": source_identity(node["path"], files, dirty),
        "line_start": node["start"] + 1, "line_end": node["end"],
        "metadata": node["metadata"], "text": text,
        "parent_id": node.get("parent_id"),
        "boundary": node.get("boundary", "heading section, not independent authority"),
        "recovery": {"id": node["id"], "options": "--parent --full or --output <path>"},
    }


def current(node: dict) -> bool:
    return node["section_status"] == "current"


def rank(nodes: list[dict], query: str) -> list[tuple[float, dict]]:
    q = terms(query)
    if not q:
        return []
    frequency = Counter(term for node in nodes for term in q if term in node["terms"])
    avg = sum(sum(n["terms"].values()) for n in nodes) / max(1, len(nodes))
    ranked = []
    for node in nodes:
        length = sum(node["terms"].values())
        score = 0.0
        for term in q:
            tf = node["terms"].get(term, 0)
            if tf:
                inverse = math.log(1 + (len(nodes) - frequency[term] + 0.5) / (frequency[term] + 0.5))
                score += inverse * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * length / max(1, avg)))
        if query.lower() in (node["heading"] + " " + node["text"]).lower():
            score += 20
        if score:
            ranked.append((score, node))
    return sorted(ranked, key=lambda item: (-item[0], item[1]["id"]))


def check_navigation(nav: dict, nodes: list[dict]) -> list[str]:
    errors = []
    ids = set()
    for card in nav["cards"]:
        if card["id"] in ids:
            errors.append(f"duplicate navigation ID: {card['id']}")
        ids.add(card["id"])
        matches = [n for n in nodes if n["path"] == card["path"] and n["heading"] == card["heading"]]
        if len(matches) != 1 or not current(matches[0]):
            errors.append(f"card heading must resolve once to current section: {card['id']}")
        for owner in card["owners"]:
            document = parse_document(REPO, safe_path(owner))
            if document.role not in {"canonical", "current-reference"}:
                errors.append(f"owner not current: {owner}")
        for ref in card["code"]:
            path, _, symbol = ref.partition("::")
            text = safe_path(path).read_text()
            if symbol and not re.search(r"\b" + re.escape(symbol) + r"\b", text):
                errors.append(f"navigation symbol absent: {ref}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    select = parser.add_mutually_exclusive_group(required=True)
    select.add_argument("--query")
    select.add_argument("--id")
    select.add_argument("--check", action="store_true")
    select.add_argument("--vault-path")
    parser.add_argument("--vault-root", type=Path)
    parser.add_argument("--kind", choices=["all", "doc", "code"], default="all")
    parser.add_argument("--mode", choices=["auto", "timed"])
    parser.add_argument("--history", action="store_true")
    parser.add_argument("--parent", action="store_true")
    parser.add_argument("--limit", type=int, default=5)
    parser.add_argument("--no-cache", action="store_true")
    add_output_arguments(parser)
    args = parser.parse_args()
    start = time.perf_counter()
    try:
        if not 1 <= args.limit <= 20 or (args.query is not None and not terms(args.query)):
            raise QueryError("--limit must be 1..20 and query must contain searchable terms")
        if args.output:
            destination = args.output.resolve()
            if not any(destination.is_relative_to(REPO / folder) for folder in ("target", "out")):
                raise QueryError("--output must be under this repository's target/ or out/")
            if destination == REPO / "target/codex-runs/knowledge-index.json":
                raise QueryError("--output must not overwrite the index cache")
        nav, inputs = navigation()
        inputs.extend([Path(__file__), REPO / "scripts/codex/_query_output.py",
                       REPO / "scripts/codex/docs_inventory.py",
                       REPO / "target/codex-runs/knowledge-index.json"])
        if args.vault_path:
            allowed = {p for r in nav["routes"] for p in r.get("vault_refs", [])}
            if args.vault_root is None or args.vault_path not in allowed:
                raise QueryError("vault requires explicit root and a registered vault_refs path")
            raw = subprocess.check_output(["git", "-C", str(args.vault_root), "show",
                                           f"{nav['vault_commit']}:{args.vault_path}"])
            content = raw.decode()
            status = re.search(r"^status:\s*(.+)$", content, re.M)
            payload = {"schema": "project-knowledge/v1", "status": "ok", "vault": {
                "root": str(args.vault_root.resolve()), "path_at_commit": args.vault_path,
                "commit": nav["vault_commit"], "sha256": digest(raw),
                "declared_status": status[1] if status else "unmarked", "text": content,
                "authority": "original status retained; not automatically canonical",
            }}
            emit(payload, args, input_paths=inputs + [args.vault_root / args.vault_path])
            return 0
        nodes, files, paths, stats = source_index(not args.no_cache)
        inputs.extend(paths)
        raw_dirty = subprocess.check_output(
            ["git", "-C", str(REPO), "ls-files", "-z", "--modified", "--others", "--exclude-standard"]
        ).decode()
        dirty = set(raw_dirty.split("\0"))
        dirty.update(subprocess.check_output(
            ["git", "-C", str(REPO), "diff", "HEAD", "--name-only", "-z"]
        ).decode().split("\0"))
        payload = {"schema": "project-knowledge/v1", "status": "ok", "head": git("rev-parse", "HEAD"),
                   "index": stats, "retrieval": "lexical BM25 + exact terms + explicit navigation; no semantic embedding"}
        if args.check:
            errors = check_navigation(nav, nodes)
            payload.update(status="error" if errors else "ok", cards=len(nav["cards"]), errors=errors)
            emit(payload, args, input_paths=inputs)
            return 2 if errors else 0
        eligible = [n for n in nodes if (args.history or current(n)) and
                    (args.kind == "all" or n["kind"] == args.kind)]
        if args.id:
            selected = [n for n in eligible if n["id"] == args.id]
            scored = [(0.0, n) for n in selected]
        else:
            scored = rank(eligible, args.query)
            score_by_id = {n["id"]: (score, n) for score, n in scored}
            matched_cards = []
            query_terms = terms(args.query)
            for card in nav["cards"]:
                if args.mode and card["modes"] and args.mode not in card["modes"]:
                    continue
                # Short generic words must not override more specific source context.
                generic = {"缓存", "耗时", "实例", "后置", "漏算", "旧规则", "技能面"}
                matches = []
                for alias in card["aliases"]:
                    overlap = len(set(terms(alias)) & set(query_terms)) / max(1, len(set(terms(alias))))
                    exact = alias.lower() in args.query.lower()
                    specificity = 0.2 if alias in generic else 1.0
                    matches.append((overlap, exact, specificity))
                affinity = max((overlap * specificity for overlap, _, specificity in matches), default=0)
                exact = any(hit and specificity == 1.0 for _, hit, specificity in matches)
                if exact or affinity >= 0.65:
                    matched_cards.append({**card, "match": "alias" if exact else "lexical-overlap"})
                    for node in eligible:
                        if node["path"] == card["path"] and node["heading"] == card["heading"]:
                            before = score_by_id.get(node["id"], (0.0, node))[0]
                            score_by_id[node["id"]] = (before + (40 if exact else 12) * affinity, node)
            payload["navigation"] = matched_cards
            payload["routes"] = [{key: route.get(key) for key in (
                "id", "title", "chain_doc", "entrypoints", "module_docs", "when")}
                for route in nav["routes"] if any(
                    symptom in args.query for symptom in route["symptoms"])]
            scored = sorted(score_by_id.values(), key=lambda item: (-item[0], item[1]["id"]))
        payload["selection"] = {"query": args.query, "id": args.id, "mode_hint": args.mode,
                                "history": args.history, "kind": args.kind, "limit": args.limit,
                                "matches": len(scored)}
        selected = scored[:args.limit]
        payload["results"] = [{"score": round(score, 4), **render_chunk(n, files, dirty)} for score, n in selected]
        if args.parent:
            parents = []
            seen = set()
            for _, node in selected:
                parent = next((n for n in nodes if n["id"] == node.get("parent_id")), node)
                if parent["id"] in seen:
                    continue
                seen.add(parent["id"])
                descendants = [n for n in nodes if n["path"] == parent["path"] and
                               parent["start"] <= n["start"] < parent["parent_end"]]
                parts = [render_chunk(n, files, dirty) for n in descendants if args.history or current(n)]
                parents.append({"id": parent["id"], "parts": parts,
                                "historical_parts_omitted": sum(not current(n) for n in descendants) if not args.history else 0})
            payload["parents"] = parents
        # Resolve current owner references into a relevant current excerpt, not a new summary.
        owner_paths = set()
        for _, node in selected:
            cards = [c for c in nav["cards"] if c["path"] == node["path"] and c["heading"] == node["heading"]]
            if cards:
                owner_paths.update(p for c in cards for p in c["owners"])
            elif not payload.get("navigation"):
                owner_paths.update(node["path"] if p == "self" else p
                                   for p in split_values(node["metadata"].get("当前真源", ""))
                                   if p == "self" or (p.startswith("docs/") and p in files))
        for card in payload.get("navigation", []):
            owner_paths.update(card["owners"])
        owners = []
        for path in sorted(owner_paths):
            choices = [n for n in nodes if n["path"] == path and current(n)]
            owner_query = args.query or (selected[0][1]["heading"] if selected else "")
            ranked = rank(choices, owner_query)
            if ranked:
                owners.append(render_chunk(ranked[0][1], files, dirty))
        payload["owners"] = owners
        payload["status"] = "ok" if selected else "no-match"
        payload["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
        payload["limitations"] = ["ranking is navigation, not authority or semantic conflict resolution",
                                   "current documents may contain unmarked historical statements; verify original scope",
                                   "mode is a navigation hint; unknown scope is not silently filtered"]
        emit(payload, args, input_paths=inputs)
        return 0 if selected else 1
    except (QueryError, InventoryError, OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(json.dumps({"schema": "project-knowledge/v1", "status": "error", "error": str(error)}, ensure_ascii=False))
        return 2


if __name__ == "__main__":
    sys.exit(main())
