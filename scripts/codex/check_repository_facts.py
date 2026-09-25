#!/usr/bin/env python3
"""Check repository-wide stable documentation, lifecycle, and CLI facts."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from urllib.parse import unquote

import docs_inventory
import generate_agent_index
from check_feedback_evidence import check_tracking


PROJECT_MAP_RETRIEVAL_FACTS = (
    "| 制造 full_pool / standalone 与填房 |",
    "`layout/assign/manufacture_fill.rs::manu_options` 设置排班 `full_pool=true`",
    "`search/manufacture.rs::search_manufacture_single_recipe` 只在 `false` 应用 standalone",
    "| 动态 producer admission / 依赖 / 同班 |",
    "`data/producer_rules.json` 声明 admission/target/relation → `response_dependency.rs` 加载并规范化依赖",
    "| 办公室/会客室静态求值与出口 |",
    "当前 `support_facility.rs` → `layout/resolve.rs` → `commands/layout.rs::layout_eval_cmd`",
    "未实现的 `plan.compute` 扩展点是 `schedule/team_rotation.rs::TeamShiftResult` → `commands/serve.rs::RotationShiftSummary`",
)


INDEX_RETRIEVAL_FACTS = (
    "[Agent 路由](生成索引/Agent路由.md)",
    "[知识库引用合同](知识库引用.md)",
    "[生成代码与文档地图](生成索引/代码与文档地图.md)",
)


PROJECT_MAP_PROTECTED_FACTS = (
    "`src/commands/plan.rs` | `plan` argv/文件适配",
    "`src/commands/plan_compute.rs` | `plan` / `serve` 共用的单次 rotation + profile/MAA 编排",
    "机器主入口为内联 `plan.compute`",
    "`plan.compute` 内联协议和 legacy 路径适配",
    "[FRONTEND_CLI.md](FRONTEND_CLI.md)",
    "[FRONTEND_SERVE_GUIDE.md](FRONTEND_SERVE_GUIDE.md)",
    "Next BFF 使用 `serve` / `plan.compute`；一次性调用使用 `plan` / `--maa-out`。",
    "`expect_rule_id`",
) + PROJECT_MAP_RETRIEVAL_FACTS


def _mask_rust_noncode(
    text: str, *, keep_strings: bool = False
) -> tuple[str, str | None]:
    masked = list(text)

    def blank(start: int, end: int) -> None:
        for index in range(start, end):
            if text[index] != "\n":
                masked[index] = " "

    index = 0
    while index < len(text):
        if text.startswith("//", index):
            end = text.find("\n", index + 2)
            end = len(text) if end < 0 else end
            blank(index, end)
            index = end
            continue
        if text.startswith("/*", index):
            depth = 1
            end = index + 2
            while end < len(text) and depth:
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            if depth:
                return "", "unterminated Rust block comment"
            blank(index, end)
            index = end
            continue

        raw = re.match(r'(?:br|r)(?P<hashes>#{0,16})"', text[index:])
        if raw:
            delimiter = '"' + raw.group("hashes")
            content_start = index + raw.end()
            end = text.find(delimiter, content_start)
            if end < 0:
                return "", "unterminated Rust raw string"
            end += len(delimiter)
            if not keep_strings:
                blank(index, end)
            index = end
            continue

        if text[index] == '"':
            end = index + 1
            escaped = False
            while end < len(text):
                character = text[end]
                if character == '"' and not escaped:
                    end += 1
                    break
                if character == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                end += 1
            else:
                return "", "unterminated Rust string"
            if not keep_strings:
                blank(index, end)
            index = end
            continue

        character = re.match(r"'(?:\\.|[^'\\\n])'", text[index:])
        if character:
            end = index + character.end()
            if not keep_strings:
                blank(index, end)
            index = end
            continue
        index += 1

    return "".join(masked), None


def _extract_rust_match_body(text: str, marker: str) -> tuple[str, str | None]:
    masked, mask_error = _mask_rust_noncode(text)
    if mask_error:
        return "", mask_error
    if masked.count(marker) != 1:
        return "", f"expected exactly one Rust match marker: {marker}"

    marker_start = masked.find(marker)
    opening = masked.find("{", marker_start + len(marker))
    if opening < 0:
        return "", f"Rust match marker has no block: {marker}"
    depth = 0
    for index in range(opening, len(masked)):
        if masked[index] == "{":
            depth += 1
        elif masked[index] == "}":
            depth -= 1
            if depth == 0:
                return text[opening + 1 : index], None
    return "", f"Rust match block is unclosed: {marker}"


def _top_level_arm_position(body: str, arm: str) -> tuple[int | None, str | None]:
    positions, position_error = _top_level_match_positions(
        body, re.compile(rf"^[ \t]*{re.escape(arm)}[ \t]*=>", re.MULTILINE)
    )
    if position_error:
        return None, position_error
    if positions:
        return positions[0], None
    return None, f"Rust match has no top-level {arm} fallback"


def _top_level_match_positions(
    body: str, pattern: re.Pattern[str]
) -> tuple[list[int], str | None]:
    masked, mask_error = _mask_rust_noncode(body)
    if mask_error:
        return [], mask_error
    comment_free, comment_error = _mask_rust_noncode(body, keep_strings=True)
    if comment_error:
        return [], comment_error
    depth = 0
    depth_at: list[int] = []
    for character in masked:
        depth_at.append(depth)
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
    return [
        match.start()
        for match in pattern.finditer(comment_free)
        if depth_at[match.start()] == 0
    ], None


def parse_bake_actions(text: str) -> tuple[set[str], str | None]:
    marker = "match args[i].as_str()"
    match_body, block_error = _extract_rust_match_body(text, marker)
    if block_error:
        return set(), block_error
    fallback, fallback_error = _top_level_arm_position(match_body, "other")
    if fallback_error or fallback is None:
        return set(), fallback_error
    match_body = match_body[:fallback]
    labels: set[str] = set()
    arm_re = re.compile(r'^\s*((?:"[^"]+"\s*\|\s*)*"[^"]+")\s*=>', re.MULTILINE)
    arm_positions, arm_error = _top_level_match_positions(match_body, arm_re)
    if arm_error:
        return set(), arm_error
    comment_free, comment_error = _mask_rust_noncode(match_body, keep_strings=True)
    if comment_error:
        return set(), comment_error
    for position in arm_positions:
        arm = arm_re.match(comment_free, position)
        if arm:
            labels.update(re.findall(r'"([^"]+)"', arm.group(1)))
    if not labels.intersection({"--help", "-h"}):
        return set(), "bake action match has no help arm"
    actions = {label for label in labels if not label.startswith("-")}
    if not actions:
        return set(), "bake action match has no actions"
    return actions, None


def parse_profile_actions(text: str) -> tuple[set[str], str | None]:
    marker = "match args.first().map(String::as_str)"
    match_body, block_error = _extract_rust_match_body(text, marker)
    if block_error:
        return set(), block_error
    fallback, fallback_error = _top_level_arm_position(match_body, "_")
    if fallback_error or fallback is None:
        return set(), fallback_error
    match_body = match_body[:fallback]
    arm_re = re.compile(
        r'^\s*Some\(\s*"([a-z][a-z-]*)"\s*\)\s*=>', re.MULTILINE
    )
    arm_positions, arm_error = _top_level_match_positions(match_body, arm_re)
    if arm_error:
        return set(), arm_error
    comment_free, comment_error = _mask_rust_noncode(match_body, keep_strings=True)
    if comment_error:
        return set(), comment_error
    actions = {
        arm.group(1)
        for position in arm_positions
        if (arm := arm_re.match(comment_free, position))
    }
    if not actions:
        return set(), "profile action match has no actions"
    return actions, None


def check_markdown_links(repo: Path, documents: list[str]) -> list[str]:
    errors: list[str] = []
    link_re = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
    for document in documents:
        path = repo / document
        if path.suffix.lower() != ".md" or not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        for raw_target in link_re.findall(text):
            target = raw_target.strip()
            if target.startswith("<") and ">" in target:
                target = target[1 : target.index(">")]
            else:
                target = target.split(" ", 1)[0]
            if not target or target.startswith(("#", "http://", "https://", "mailto:")):
                continue
            local = unquote(target.split("#", 1)[0])
            if not (path.parent / local).resolve(strict=False).exists():
                errors.append(f"broken Markdown link in {document}: {raw_target}")
    return errors


def check_cli_help_map(repo: Path) -> list[str]:
    errors: list[str] = []
    main_path = repo / "crates/infra-cli/src/main.rs"
    layout_path = repo / "crates/infra-cli/src/commands/layout.rs"
    map_path = repo / "docs/PROJECT_MAP.md"
    for path in (main_path, layout_path, map_path):
        if not path.is_file():
            return [f"cli-help-map input is missing: {path}"]

    main_text = main_path.read_text(encoding="utf-8")
    layout_text = layout_path.read_text(encoding="utf-8")
    map_text = map_path.read_text(encoding="utf-8")
    match_start = main_text.find("match args[1].as_str()")
    match_end = main_text.find("other =>", match_start)
    if match_start < 0 or match_end < 0:
        return ["cannot parse top-level CLI dispatch in main.rs"]
    source_commands = set(
        re.findall(r'^\s*"([a-z][a-z-]*)"\s*=>', main_text[match_start:match_end], re.MULTILINE)
    )

    section_start = map_text.find("## `infra-cli` 命令")
    if section_start < 0:
        return ["cannot find infra-cli command section in PROJECT_MAP.md"]
    next_heading = re.search(r"\n#{2,3} ", map_text[section_start + 1 :])
    section_end = section_start + 1 + next_heading.start() if next_heading else len(map_text)
    section = map_text[section_start:section_end]
    doc_commands: set[str] = set()
    doc_layout_commands: set[str] = set()
    for line in section.splitlines():
        if not line.startswith("|"):
            continue
        in_code = False
        column_end = None
        for index, character in enumerate(line[1:], start=1):
            if character == "`":
                in_code = not in_code
            elif character == "|" and not in_code:
                column_end = index
                break
        if column_end is None:
            continue
        for value in re.findall(r"`([^`]+)`", line[1:column_end]):
            words = value.split()
            if not words or words[0].startswith("-"):
                continue
            doc_commands.add(words[0])
            if words[0] == "layout" and len(words) > 1:
                doc_layout_commands.add(words[1])

    source_layout_commands = set(re.findall(r'Some\("([a-z][a-z-]*)"\)\s*=>', layout_text))
    usage_commands = set(re.findall(r"infra-cli ([a-z][a-z-]*)", main_text))
    usage_layout_commands = set(re.findall(r"infra-cli layout ([a-z][a-z-]*)", main_text))
    if source_commands != doc_commands:
        errors.append(
            f"top-level CLI map mismatch: source_only={sorted(source_commands - doc_commands)} "
            f"docs_only={sorted(doc_commands - source_commands)}"
        )
    missing_usage = source_commands - usage_commands
    if missing_usage:
        errors.append(f"top-level usage is missing commands: {sorted(missing_usage)}")
    if source_layout_commands != doc_layout_commands:
        errors.append(
            f"layout CLI map mismatch: source_only={sorted(source_layout_commands - doc_layout_commands)} "
            f"docs_only={sorted(doc_layout_commands - source_layout_commands)}"
        )
    missing_layout_usage = source_layout_commands - usage_layout_commands
    if missing_layout_usage:
        errors.append(f"top-level usage is missing layout subcommands: {sorted(missing_layout_usage)}")
    return errors


def check_project_map_owner_contract(repo: Path) -> list[str]:
    errors: list[str] = []
    lib_path = repo / "crates/infra-core/src/lib.rs"
    bake_path = repo / "crates/infra-cli/src/commands/bake.rs"
    profile_path = repo / "crates/infra-cli/src/commands/profile.rs"
    map_path = repo / "docs/PROJECT_MAP.md"
    for path in (lib_path, bake_path, profile_path, map_path):
        if not path.is_file():
            return [f"project-map owner input is missing: {path}"]

    lib_text = lib_path.read_text(encoding="utf-8")
    bake_text = bake_path.read_text(encoding="utf-8")
    profile_text = profile_path.read_text(encoding="utf-8")
    map_text = map_path.read_text(encoding="utf-8")
    source_modules = set(re.findall(r"^pub mod ([a-z_]+);", lib_text, re.MULTILINE))
    section_start = map_text.find("## `infra-core` 模块索引")
    if section_start < 0:
        return ["cannot find infra-core module section in PROJECT_MAP.md"]
    section_tail = map_text[section_start + 1 :]
    next_heading = re.search(r"\n### ", section_tail)
    section_end = section_start + 1 + next_heading.start() if next_heading else len(map_text)
    section = map_text[section_start:section_end]
    documented_modules = set(
        re.findall(r"^\| `([a-z_]+)` \|", section, flags=re.MULTILINE)
    )
    if source_modules != documented_modules:
        errors.append(
            "infra-core module map mismatch: "
            f"source_only={sorted(source_modules - documented_modules)} "
            f"docs_only={sorted(documented_modules - source_modules)}"
        )

    command_start = map_text.find("## `infra-cli` 命令")
    command_tail = map_text[command_start + 1 :] if command_start >= 0 else ""
    command_heading = re.search(r"\n### ", command_tail)
    command_end = (
        command_start + 1 + command_heading.start()
        if command_start >= 0 and command_heading
        else len(map_text)
    )
    command_section = map_text[command_start:command_end] if command_start >= 0 else ""

    def documented_actions(command: str) -> set[str]:
        actions: set[str] = set()
        for value in re.findall(r"`([^`]+)`", command_section):
            words = value.split(maxsplit=1)
            if len(words) == 2 and words[0] == command:
                actions.update(re.findall(r"[a-z][a-z-]*", words[1]))
        return actions

    source_bake_actions, bake_parse_error = parse_bake_actions(bake_text)
    if bake_parse_error:
        errors.append(bake_parse_error)
    else:
        usage_bake_actions: set[str] = set()
        for value in re.findall(r"infra-cli bake (\[[^\]]+\]|[a-z][a-z-]*)", bake_text):
            usage_bake_actions.update(re.findall(r"[a-z][a-z-]*", value))
        if source_bake_actions != usage_bake_actions:
            errors.append(
                "bake source/help mismatch: "
                f"source_only={sorted(source_bake_actions - usage_bake_actions)} "
                f"help_only={sorted(usage_bake_actions - source_bake_actions)}"
            )
        doc_bake_actions = documented_actions("bake")
        if source_bake_actions != doc_bake_actions:
            errors.append(
                "bake CLI map mismatch: "
                f"source_only={sorted(source_bake_actions - doc_bake_actions)} "
                f"docs_only={sorted(doc_bake_actions - source_bake_actions)}"
            )

    source_profile_actions, profile_parse_error = parse_profile_actions(profile_text)
    if profile_parse_error:
        errors.append(profile_parse_error)
    else:
        usage_profile_actions = set(
            re.findall(r"infra-cli profile ([a-z][a-z-]*)", profile_text)
        )
        if source_profile_actions != usage_profile_actions:
            errors.append(
                "profile source/help mismatch: "
                f"source_only={sorted(source_profile_actions - usage_profile_actions)} "
                f"help_only={sorted(usage_profile_actions - source_profile_actions)}"
            )
        doc_profile_actions = documented_actions("profile")
        if source_profile_actions != doc_profile_actions:
            errors.append(
                "profile CLI map mismatch: "
                f"source_only={sorted(source_profile_actions - doc_profile_actions)} "
                f"docs_only={sorted(doc_profile_actions - source_profile_actions)}"
            )

    errors.extend(check_project_map_protected_facts(map_text))
    for stale in ("expect_shortcut", "tests/fixtures/"):
        if stale in map_text:
            errors.append(f"PROJECT_MAP.md retains stale owner fact: {stale}")
    return errors


def check_project_map_protected_facts(map_text: str) -> list[str]:
    return [
        f"PROJECT_MAP.md is missing current owner fact: {fact}"
        for fact in PROJECT_MAP_PROTECTED_FACTS
        if fact not in map_text
    ]


def check_navigation_contract(repo: Path) -> list[str]:
    errors: list[str] = []
    agents_path = repo / "AGENTS.md"
    index_path = repo / "docs/INDEX.md"
    glossary_path = repo / "docs/GLOSSARY.md"
    for path in (agents_path, index_path, glossary_path):
        if not path.is_file():
            return [f"navigation contract input is missing: {path}"]

    agents_text = agents_path.read_text(encoding="utf-8")
    index_text = index_path.read_text(encoding="utf-8")
    glossary_text = glossary_path.read_text(encoding="utf-8")
    if "根 `AGENTS.md` 是任务分类、真源顺序和项目硬门禁的唯一入口" not in agents_text:
        errors.append("AGENTS.md does not declare the unique task-classification boundary")
    if "本文不维护第二份分类表" not in index_text:
        errors.append("INDEX.md does not defer task classification to AGENTS.md")
    expected_sections = [
        "## 1. 从症状开始",
        "## 2. 按入口阅读",
        "## 3. 按模块阅读",
        "## 4. 接口与验证",
        "## 5. 外部机制与历史材料",
    ]
    actual_sections = re.findall(r"^## .+$", index_text, re.MULTILINE)
    if actual_sections != expected_sections:
        errors.append(
            "INDEX.md top-level sections do not match the six-section reader IA: "
            f"actual={actual_sections}"
        )
    if "## 按任务意图进入" in index_text:
        errors.append("INDEX.md retains an intent-classification table")
    for stale in ("debug -> arknights-maintenance", "quality-refactor -> arknights-quality"):
        if stale in index_text:
            errors.append(f"INDEX.md retains a parallel task-classification row: {stale}")
    project_skill_link = r"\(\.\./\.agents/skills/[^)]+/SKILL\.md(?:#[^)]*)?\)"
    project_skill_id = r"\b(?:arknights|gongsun)-[a-z][a-z-]+\b"
    if re.search(project_skill_link, index_text) or re.search(project_skill_id, index_text):
        errors.append("INDEX.md routes project Skills directly instead of deferring to AGENTS.md")
    for fact in INDEX_RETRIEVAL_FACTS:
        if fact not in index_text:
            errors.append(f"INDEX.md is missing bounded owner retrieval route: {fact}")

    stale_glossary = (
        "当前默认项目阶段",
        "维护期 Markdown",
        "target/codex-logs/",
        "select → plan → execute → fill → resolve → rotation → export",
        "当前唯一的三队轮换模型",
    )
    for phrase in stale_glossary:
        if phrase in glossary_text:
            errors.append(f"GLOSSARY.md retains stale current-reference wording: {phrase}")

    section_reference = re.compile(r"AGENTS\.md[^\n]*§")
    for root in (repo / "docs", repo / "data"):
        for path in root.rglob("*.md"):
            relative = path.relative_to(repo).as_posix()
            if relative.startswith("docs/ARCHIVE/"):
                continue
            for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
                if section_reference.search(line):
                    errors.append(f"fragile AGENTS section-number reference: {relative}:{line_no}")
    if "docs/agent-index/routes.json" not in index_text:
        errors.append("INDEX.md does not name the machine-readable route registry")
    return errors


def check_agent_index(repo: Path) -> list[str]:
    """Check generated Agent navigation without rewriting it."""
    errors: list[str] = []
    registry_path = repo / "docs/agent-index/routes.json"
    if not registry_path.is_file():
        return ["missing machine-readable Agent route registry: docs/agent-index/routes.json"]
    try:
        registry = generate_agent_index.load_registry(repo)
    except (OSError, UnicodeError, ValueError) as error:
        return [f"invalid Agent route registry: {error}"]
    if registry.get("schema") != 1:
        errors.append(f"unsupported Agent route registry schema: {registry.get('schema')!r}")
    routes = registry.get("routes")
    if not isinstance(routes, list):
        return errors + ["Agent route registry routes must be a list"]
    expected = {
        repo / "docs/生成索引/Agent路由.md": generate_agent_index.render_routes(repo, registry),
        repo / "docs/生成索引/代码与文档地图.md": generate_agent_index.render_code_map(repo, registry),
    }
    for path, content in expected.items():
        if not path.is_file():
            errors.append(f"missing generated Agent index: {path.relative_to(repo)}")
        elif path.read_text(encoding="utf-8") != content:
            errors.append(f"generated Agent index drift: {path.relative_to(repo)}")
    seen_ids: set[str] = set()
    required_fields = {
        "id", "title", "symptoms", "entrypoints", "chain_doc",
        "module_docs", "mechanics", "verification",
    }
    for route in routes:
        route_id = route.get("id", "?")
        if route_id in seen_ids:
            errors.append(f"duplicate Agent route id: {route_id}")
        seen_ids.add(route_id)
        missing = sorted(field for field in required_fields if field not in route)
        if missing:
            errors.append(f"Agent route {route_id} missing fields: {', '.join(missing)}")
            continue
        for field in ("symptoms", "entrypoints", "module_docs", "mechanics", "verification"):
            if not isinstance(route[field], list) or not route[field]:
                errors.append(f"Agent route {route_id} requires non-empty list: {field}")
        for field in ("chain_doc",):
            target = route.get(field, "")
            if not (repo / target).is_file():
                errors.append(f"Agent route {route_id} points to missing {field}: {target}")
        for field in ("module_docs",):
            for target in route.get(field, []):
                if not (repo / target).is_file():
                    errors.append(f"Agent route {route_id} points to missing {field}: {target}")
        for field in ("mechanics",):
            for target in route.get(field, []):
                if not (repo / target.split("#", 1)[0]).is_file():
                    errors.append(f"Agent route {route_id} points to missing {field}: {target}")
        for entrypoint in route.get("entrypoints", []):
            source = entrypoint.split("::", 1)[0]
            if not (repo / source).is_file():
                errors.append(f"Agent route {route_id} points to missing entrypoint file: {entrypoint}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--allow-in-progress", action="store_true")
    args = parser.parse_args()
    repo = args.repo_root.resolve()

    documents, errors = docs_inventory.load_inventory(repo)
    errors.extend(docs_inventory.validate_inventory(repo, documents, final=not args.allow_in_progress))
    errors.extend(docs_inventory.check_generated_sections(repo, documents))
    errors.extend(check_markdown_links(repo, [document.path for document in documents]))
    errors.extend(check_tracking(repo, verify_local=False))
    errors.extend(check_cli_help_map(repo))
    errors.extend(check_project_map_owner_contract(repo))
    errors.extend(check_navigation_contract(repo))
    errors.extend(check_agent_index(repo))
    if errors:
        for error in sorted(set(errors)):
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(
        "PASS repository_facts "
        f"documents={len(documents)} domains={sum(len(document.domains) for document in documents)} "
        "lifecycle=ok links=ok feedback=ok cli_help_map=ok "
        "project_map_owner=ok navigation=ok"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
