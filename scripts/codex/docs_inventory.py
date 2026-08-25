#!/usr/bin/env python3
"""Parse, validate, and render the repository documentation inventory."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


SCHEMA_VERSION = 3
META_KEYS = {
    "文档角色",
    "生命周期状态",
    "领域键",
    "当前真源",
    "摘要",
    "生成器",
    "替代项",
    "历史原因",
    "快照日期",
}
ROLES = {
    "canonical",
    "current-reference",
    "active-change",
    "decision",
    "generated-reference",
    "evidence",
    "archive",
}
ROLE_STATES = {
    "canonical": {"current"},
    "current-reference": {"current"},
    "active-change": {"proposal", "ready-on-request", "in-progress", "blocked"},
    "decision": {"proposed", "accepted", "superseded"},
    "generated-reference": {"generated"},
    "evidence": {"current", "closed"},
    "archive": {"completed", "superseded", "historical"},
}
GENERATED_SECTIONS = {
    "canonical": ("<!-- BEGIN GENERATED CANONICAL -->", "<!-- END GENERATED CANONICAL -->"),
    "active": ("<!-- BEGIN GENERATED ACTIVE CHANGES -->", "<!-- END GENERATED ACTIVE CHANGES -->"),
    "decisions": ("<!-- BEGIN GENERATED DECISIONS -->", "<!-- END GENERATED DECISIONS -->"),
}
META_RE = re.compile(r"^>\s*([^：:]+)[：:]\s*(.*?)\s*$")
H1_RE = re.compile(r"^#\s+(.+?)\s*$")


class InventoryError(ValueError):
    pass


@dataclass(frozen=True)
class Document:
    path: str
    title: str
    metadata: dict[str, str]
    text: str

    @property
    def role(self) -> str:
        return self.metadata.get("文档角色", "")

    @property
    def status(self) -> str:
        return self.metadata.get("生命周期状态", "")

    @property
    def domains(self) -> list[str]:
        return split_values(self.metadata.get("领域键", ""))

    @property
    def owners(self) -> list[str]:
        return split_values(self.metadata.get("当前真源", ""))


def split_values(value: str) -> list[str]:
    if not value:
        return []
    return [item.strip().replace("\\", "/") for item in re.split(r"[；;]", value) if item.strip()]


def _repo_relative(repo: Path, path: Path) -> str:
    return path.resolve(strict=False).relative_to(repo.resolve()).as_posix()


def governed_markdown_paths(repo: Path) -> list[Path]:
    paths: set[Path] = set()
    for root_name in ("docs", "plans"):
        root = repo / root_name
        if root.is_dir():
            paths.update(root.rglob("*.md"))
    feedback = repo / "feedback"
    if feedback.is_dir():
        paths.update(path for path in feedback.glob("*.md") if path.is_file())
    return sorted(paths, key=lambda path: _repo_relative(repo, path))


def parse_document(repo: Path, path: Path, text: str | None = None) -> Document:
    relative = _repo_relative(repo, path)
    source = path.read_text(encoding="utf-8") if text is None else text
    lines = source.splitlines()
    if not lines:
        raise InventoryError(f"empty Markdown document: {relative}")
    title_match = H1_RE.match(lines[0])
    if not title_match:
        raise InventoryError(f"document must start with one H1 title: {relative}")

    index = 1
    while index < len(lines) and not lines[index].strip():
        index += 1
    start = index
    metadata: dict[str, str] = {}
    while index < len(lines) and lines[index].startswith(">"):
        match = META_RE.match(lines[index])
        if not match:
            raise InventoryError(f"invalid lifecycle metadata line: {relative}:{index + 1}")
        key, value = match.groups()
        key = key.strip()
        if key not in META_KEYS:
            raise InventoryError(f"unknown document metadata key {key!r}: {relative}:{index + 1}")
        if key in metadata:
            raise InventoryError(f"duplicate document metadata key {key!r}: {relative}:{index + 1}")
        metadata[key] = value.strip()
        index += 1
    if not metadata:
        raise InventoryError(f"document lacks lifecycle metadata after H1: {relative}")
    return Document(relative, title_match.group(1), metadata, source)


def load_inventory(repo: Path) -> tuple[list[Document], list[str]]:
    documents: list[Document] = []
    errors: list[str] = []
    for path in governed_markdown_paths(repo):
        try:
            documents.append(parse_document(repo, path))
        except (OSError, UnicodeError, InventoryError) as error:
            errors.append(str(error))
    return documents, errors


def _is_repo_path(value: str) -> bool:
    if not value or value.startswith(("/", "../")) or "\\" in value:
        return False
    return not any(part == ".." for part in Path(value).parts)


def _path_role_errors(document: Document) -> list[str]:
    errors: list[str] = []
    path = document.path
    role = document.role
    if path.startswith("plans/"):
        errors.append(f"root plans must not contain Markdown: {path}")
    if path.startswith("docs/TODO/"):
        expected = "current-reference" if path == "docs/TODO/README.md" else "active-change"
        if role != expected:
            errors.append(f"TODO path must use {expected} role: {path}")
    elif path.startswith("docs/ADR/") and role != "decision":
        errors.append(f"ADR document must be decision: {path}")
    elif path.startswith("docs/ARCHIVE/"):
        expected = "current-reference" if path == "docs/ARCHIVE/README.md" else "archive"
        if role != expected:
            errors.append(f"archive path must use {expected} role: {path}")
    elif path.startswith("feedback/") and role not in {"evidence", "current-reference"}:
        errors.append(f"tracked feedback Markdown must be evidence/current-reference: {path}")
    elif path.startswith("docs/") and role in {"active-change", "archive"}:
        errors.append(f"current docs path cannot use {role} role: {path}")
    if role == "active-change" and not (path.startswith("docs/TODO/") and path != "docs/TODO/README.md"):
        errors.append(f"active-change role is only allowed under docs/TODO: {path}")
    if role == "decision" and not path.startswith("docs/ADR/"):
        errors.append(f"decision role is only allowed under docs/ADR: {path}")
    if role == "archive" and not path.startswith("docs/ARCHIVE/"):
        errors.append(f"archive role is only allowed under docs/ARCHIVE: {path}")
    if role == "evidence" and not path.startswith("feedback/"):
        errors.append(f"evidence role requires a named evidence root: {path}")
    if role in {"canonical", "generated-reference"} and not path.startswith("docs/"):
        errors.append(f"{role} role is only allowed in docs current paths: {path}")
    return errors


def validate_document(document: Document, *, final: bool) -> list[str]:
    errors: list[str] = []
    role = document.role
    status = document.status
    if role not in ROLES:
        return [f"invalid document role {role!r}: {document.path}"]
    if status not in ROLE_STATES[role]:
        errors.append(f"invalid lifecycle status {status!r} for {role}: {document.path}")
    if role != "canonical" and "self" in document.owners:
        errors.append(f"only canonical documents may use self as current owner: {document.path}")
    if not document.metadata.get("摘要"):
        errors.append(f"document summary is required: {document.path}")
    errors.extend(_path_role_errors(document))

    if role == "canonical":
        if not document.domains:
            errors.append(f"canonical document requires at least one domain key: {document.path}")
        if document.owners != ["self"]:
            errors.append(f"canonical current owner must be self: {document.path}")
    elif role in {"current-reference", "active-change", "decision", "generated-reference", "evidence"}:
        if not document.owners:
            errors.append(f"{role} document requires current owner links: {document.path}")
    elif role == "archive" and not (
        document.metadata.get("替代项") or document.metadata.get("历史原因")
    ):
        errors.append(f"archive document requires replacement or historical reason: {document.path}")

    if role == "generated-reference" and not document.metadata.get("生成器"):
        errors.append(f"generated-reference requires a generator: {document.path}")
    generator = document.metadata.get("生成器", "")
    if generator and (not _is_repo_path(generator) or any(char in generator for char in "*?[")):
        errors.append(f"generator must be one concrete repository path: {document.path}: {generator}")
    for owner in document.owners:
        if owner != "self" and (not _is_repo_path(owner) or any(char in owner for char in "*?[")):
            errors.append(f"current owner must be a concrete repository path: {document.path}: {owner}")
    if final and role == "active-change" and status == "in-progress":
        errors.append(f"in-progress change cannot pass final check: {document.path}")
    return errors


def validate_inventory(repo: Path, documents: list[Document], *, final: bool) -> list[str]:
    errors: list[str] = []
    by_path = {document.path: document for document in documents}
    for document in documents:
        errors.extend(validate_document(document, final=final))
        generator = document.metadata.get("生成器", "")
        if generator and not (repo / generator).is_file():
            errors.append(f"generator path does not exist: {document.path}: {generator}")
    domain_owners: dict[str, list[str]] = {}
    for document in documents:
        if document.role == "canonical":
            for domain in document.domains:
                domain_owners.setdefault(domain, []).append(document.path)
    for domain, owners in sorted(domain_owners.items()):
        if len(owners) != 1:
            errors.append(f"domain key must have exactly one canonical owner: {domain}: {owners}")
    for document in documents:
        for owner in document.owners:
            if owner == "self":
                continue
            target = by_path.get(owner)
            if target is None:
                errors.append(f"current owner must be a governed current document: {document.path}: {owner}")
            elif target.role not in {"canonical", "current-reference"}:
                errors.append(f"current owner is not current truth: {document.path}: {owner}")
    return sorted(set(errors))


def render_canonical_table(documents: Iterable[Document]) -> str:
    rows: list[tuple[str, str, str]] = []
    for document in documents:
        if document.role != "canonical":
            continue
        link = Path(document.path).relative_to("docs").as_posix().replace(" ", "%20")
        for domain in document.domains:
            rows.append((domain, document.title, link))
    lines = ["| 领域键 | 权威入口 |", "|---|---|"]
    lines.extend(f"| `{domain}` | [{title}]({link}) |" for domain, title, link in sorted(rows))
    return "\n".join(lines)


def render_active_table(documents: Iterable[Document]) -> str:
    rows = [document for document in documents if document.role == "active-change"]
    lines = ["| 文件 | 状态 | 用途 |", "|---|---|---|"]
    for document in sorted(rows, key=lambda item: item.path):
        link = Path(document.path).name.replace(" ", "%20")
        lines.append(f"| [{document.title}]({link}) | `{document.status}` | {document.metadata['摘要']} |")
    return "\n".join(lines)


def render_decision_table(documents: Iterable[Document]) -> str:
    rows = [document for document in documents if document.role == "decision"]
    lines = ["| 决策 | 状态 | 摘要 |", "|---|---|---|"]
    for document in sorted(rows, key=lambda item: item.path):
        link = Path(document.path).relative_to("docs").as_posix().replace(" ", "%20")
        lines.append(
            f"| [{document.title}]({link}) | `{document.status}` | {document.metadata['摘要']} |"
        )
    return "\n".join(lines)


def replace_generated_section(text: str, section: str, body: str) -> str:
    begin, end = GENERATED_SECTIONS[section]
    replacement = f"{begin}\n{body}\n{end}"
    pattern = re.compile(re.escape(begin) + r".*?" + re.escape(end), re.DOTALL)
    if pattern.search(text):
        return pattern.sub(replacement, text)
    raise InventoryError(f"generated section markers are missing: {section}")


def _generated_targets(repo: Path, documents: list[Document]) -> dict[str, tuple[Path, str]]:
    return {
        "canonical": (repo / "docs/INDEX.md", render_canonical_table(documents)),
        "active": (repo / "docs/TODO/README.md", render_active_table(documents)),
        "decisions": (repo / "docs/INDEX.md", render_decision_table(documents)),
    }


def check_generated_sections(repo: Path, documents: list[Document]) -> list[str]:
    errors: list[str] = []
    for section, (path, body) in _generated_targets(repo, documents).items():
        try:
            text = path.read_text(encoding="utf-8")
            expected = replace_generated_section(text, section, body)
        except (OSError, UnicodeError, InventoryError) as error:
            errors.append(str(error))
            continue
        if expected != text:
            errors.append(f"generated documentation section drift: {_repo_relative(repo, path)}")
    return errors


def write_generated_sections(repo: Path, documents: list[Document]) -> None:
    for section, (path, body) in _generated_targets(repo, documents).items():
        text = path.read_text(encoding="utf-8")
        path.write_text(replace_generated_section(text, section, body), encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--check", action="store_true", help="run final hard checks")
    parser.add_argument("--report", action="store_true", help="print inventory violations without final-only rules")
    parser.add_argument("--write-indexes", action="store_true")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    repo = args.repo_root.resolve()
    documents, errors = load_inventory(repo)
    errors.extend(validate_inventory(repo, documents, final=args.check))
    if args.write_indexes and not errors:
        write_generated_sections(repo, documents)
    if args.check:
        errors.extend(check_generated_sections(repo, documents))
    if errors:
        for error in sorted(set(errors)):
            print(f"error: {error}", file=sys.stderr)
        print(f"FAIL docs_inventory documents={len(documents)} errors={len(set(errors))}")
        return 1
    print(f"PASS docs_inventory documents={len(documents)} schema={SCHEMA_VERSION}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
