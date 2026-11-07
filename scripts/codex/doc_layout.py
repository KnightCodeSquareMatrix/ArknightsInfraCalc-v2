"""Single repository layout contract for documentation tooling."""

from __future__ import annotations

from pathlib import Path


DOCS_ROOT = "docs"
INDEX = "docs/INDEX.md"
PROJECT_MAP = "docs/PROJECT_MAP.md"
LIFECYCLE = "docs/文档生命周期.md"

ACTIVE_ROOT = "docs/TODO"
ACTIVE_INDEX = "docs/TODO/README.md"
ACTIVE_DETAILS_ROOT = "docs/TODO/详情"
ACTIVE_TRACKERS = (
    "docs/TODO/核心求解.md",
    "docs/TODO/排班轮换.md",
    "docs/TODO/接口产品.md",
    "docs/TODO/性能质量.md",
    "docs/TODO/文档协作.md",
)
ACTIVE_DETAIL_GROUPS = {
    "核心求解": "docs/TODO/核心求解.md",
    "排班轮换": "docs/TODO/排班轮换.md",
    "接口产品": "docs/TODO/接口产品.md",
    "性能质量": "docs/TODO/性能质量.md",
    "文档协作": "docs/TODO/文档协作.md",
}

DOMAIN_READERS = {
    "指南": "docs/指南/README.md",
    "架构": "docs/架构/README.md",
    "机制": "docs/机制/README.md",
    "排班": "docs/排班/README.md",
    "接入与前端": "docs/接入与前端/README.md",
    "质量与维护": "docs/质量与维护/README.md",
    "产品与训练": "docs/产品与训练/README.md",
}
DOMAIN_ROOTS = tuple(f"docs/{name}" for name in DOMAIN_READERS)

ADR_ROOT = "docs/ADR"
ARCHIVE_ROOT = "docs/ARCHIVE"
ARCHIVE_INDEX = "docs/ARCHIVE/README.md"

AGENT_INDEX_ROOT = "docs/agent-index"
ROUTES = "docs/agent-index/routes.json"
KNOWLEDGE = "docs/agent-index/knowledge.json"
GENERATED_ROOT = "docs/生成索引"
ROUTES_PAGE = "docs/生成索引/Agent路由.md"
CODE_MAP_PAGE = "docs/生成索引/代码与文档地图.md"
DOCUMENT_CATALOG = "docs/生成索引/文档目录.md"

STRUCTURED_ROOTS = ("docs/模块/", "docs/调用链/", "docs/接口/")

ROOT_REFERENCE_FILES = (INDEX, PROJECT_MAP, LIFECYCLE)


def under(path: str, root: str) -> bool:
    """Return whether a repository-relative path is the root or below it."""
    return path == root or path.startswith(root.rstrip("/") + "/")


def generated_agent_outputs(repo: Path) -> tuple[Path, Path]:
    return repo / ROUTES_PAGE, repo / CODE_MAP_PAGE
