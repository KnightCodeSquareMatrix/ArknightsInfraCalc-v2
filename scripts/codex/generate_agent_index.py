#!/usr/bin/env python3
"""Generate the agent route and source/document maps from the route registry."""

from __future__ import annotations

import argparse
import json
import posixpath
import re
from pathlib import Path

import doc_layout


def load_registry(repo: Path) -> dict:
    return json.loads((repo / doc_layout.ROUTES).read_text(encoding="utf-8"))


def link(repo: Path, target: str, from_path: str) -> str:
    return posixpath.relpath(target, posixpath.dirname(from_path)).replace(" ", "%20")


def render_routes(repo: Path, registry: dict) -> str:
    lines = [
        "# Agent 路由（生成）",
        "",
        "> 文档角色：generated-reference",
        "> 生命周期状态：generated",
        "> 当前真源：docs/agent-index/README.md",
        "> 生成器：scripts/codex/generate_agent_index.py",
        "> 摘要：由稳定 route 注册表生成的症状、入口、调用链、模块、机制和验证导航",
        "",
        "<!-- GENERATED: do not edit -->",
    ]
    for route in registry["routes"]:
        lines.extend([f"## `{route['id']}`：{route['title']}", ""])
        lines.append(f"**优先级**：`{route['priority']}`")
        lines.append("")
        lines.append(f"**症状**：{'、'.join(route['symptoms'])}")
        lines.append("")
        lines.append(f"**判别**：{route['when']}")
        lines.append("")
        lines.append("**入口**：" + "；".join(f"`{item}`" for item in route["entrypoints"]))
        lines.append("")
        lines.append(f"**完整调用链**：[{route['chain_doc']}]({link(repo, route['chain_doc'], doc_layout.ROUTES_PAGE)})")
        lines.append("")
        lines.append("**模块 owner**：" + "；".join(f"[{item}]({link(repo, item, doc_layout.ROUTES_PAGE)})" for item in route["module_docs"]))
        lines.append("")
        lines.append("**机制引用**：" + "；".join(f"[{item}]({link(repo, item.split('#', 1)[0], doc_layout.ROUTES_PAGE)}{'#' + item.split('#', 1)[1] if '#' in item else ''})" for item in route["mechanics"]))
        lines.append("")
        if route.get("vault_refs"):
            lines.append(f"**知识库原文（vault @ {registry['vault_commit']}）**：" + "；".join(f"`{item}`" for item in route["vault_refs"]))
            lines.append("")
        lines.append("**验证**：" + "；".join(f"`{item}`" for item in route["verification"]))
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def rust_modules(repo: Path) -> list[str]:
    text = (repo / "crates/infra-core/src/lib.rs").read_text(encoding="utf-8")
    return sorted(set(re.findall(r"^pub mod ([a-z_]+);", text, re.MULTILINE)))


def cli_commands(repo: Path) -> list[str]:
    text = (repo / "crates/infra-cli/src/main.rs").read_text(encoding="utf-8")
    start = text.find("match args[1].as_str()")
    end = text.find("other =>", start)
    return sorted(set(re.findall(r'^\s*"([a-z][a-z-]*)"\s*=>', text[start:end], re.MULTILINE)))


def render_code_map(repo: Path, registry: dict) -> str:
    lines = [
        "# 代码与文档地图（生成）",
        "",
        "> 文档角色：generated-reference",
        "> 生命周期状态：generated",
        "> 当前真源：docs/agent-index/README.md",
        "> 生成器：scripts/codex/generate_agent_index.py",
        "> 摘要：从 Rust 公共模块、CLI dispatch 和 route 注册表生成的快速定位表",
        "",
        "<!-- GENERATED: do not edit -->",
        "## `infra-core` 公共模块",
        "",
        "| 模块 | 源码入口 | 当前路由 |",
        "|---|---|---|",
    ]
    for module in rust_modules(repo):
        refs = []
        for route in registry["routes"]:
            for entrypoint in route["entrypoints"]:
                source = entrypoint.split("::", 1)[0]
                if module in Path(source).parts:
                    refs.append(route["id"])
                    break
        lines.append(f"| `{module}` | `crates/infra-core/src/{module}` | {', '.join(f'`{x}`' for x in refs) or '按 docs/模块/ 定向检索'} |")
    lines.extend(["", "## `infra-cli` 顶层命令", "", "| 命令 | dispatch | 当前路由 |", "|---|---|---|"])
    for command in cli_commands(repo):
        refs = [r["id"] for r in registry["routes"] if command in r["title"].lower() or command in json.dumps(r, ensure_ascii=False)]
        lines.append(f"| `{command}` | `crates/infra-cli/src/main.rs::run` | {', '.join(f'`{x}`' for x in refs) or '按 docs/接口/ 定向检索'} |")
    lines.extend(["", "## 注册调用链", "", "| 链 ID | 链路文档 |", "|---|---|"])
    for route in registry["routes"]:
        lines.append(f"| `{route['id']}` | [{route['chain_doc']}]({link(repo, route['chain_doc'], doc_layout.CODE_MAP_PAGE)}) |")
    return "\n".join(lines).rstrip() + "\n"


def expected_outputs(repo: Path, registry: dict) -> dict[Path, str]:
    return {
        repo / doc_layout.ROUTES_PAGE: render_routes(repo, registry),
        repo / doc_layout.CODE_MAP_PAGE: render_code_map(repo, registry),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    repo = args.repo_root.resolve()
    registry = load_registry(repo)
    outputs = expected_outputs(repo, registry)
    mismatches = []
    for path, expected in outputs.items():
        actual = path.read_text(encoding="utf-8") if path.exists() else ""
        if actual != expected:
            mismatches.append(path.relative_to(repo).as_posix())
        if not args.check:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(expected, encoding="utf-8")
    if mismatches and args.check:
        for item in mismatches:
            print(f"error: generated agent index drift: {item}")
        return 1
    print(f"{'FAIL' if mismatches and args.check else 'PASS'} agent_index routes={len(registry['routes'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
