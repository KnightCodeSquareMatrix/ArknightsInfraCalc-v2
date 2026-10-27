from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


CODEX_DIR = Path(__file__).resolve().parents[1]
FIXTURES = Path(__file__).resolve().parent / "fixtures"
sys.path.insert(0, str(CODEX_DIR))

import check_repository_facts  # noqa: E402
import check_task_scope  # noqa: E402
import compare_test_failures  # noqa: E402
import docs_inventory  # noqa: E402
import render_evidence  # noqa: E402


RUNNER = CODEX_DIR / "run_evidence.sh"
COMPARE = CODEX_DIR / "compare_test_failures.py"


class FailureParserTests(unittest.TestCase):
    def test_no_failures(self) -> None:
        self.assertEqual(compare_test_failures.parse_log(FIXTURES / "cargo_ok.txt"), set())

    def test_exact_names_and_duplicates(self) -> None:
        self.assertEqual(
            compare_test_failures.parse_log(FIXTURES / "cargo_fail_ab.txt"),
            {"suite::alpha", "suite::beta"},
        )
        self.assertEqual(
            compare_test_failures.parse_log(FIXTURES / "cargo_duplicate.txt"),
            {"shared::same_name"},
        )

    def test_same_added_and_removed_policy(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            same = subprocess.run(
                [
                    sys.executable,
                    str(COMPARE),
                    "--baseline",
                    str(FIXTURES / "cargo_fail_ab.txt"),
                    "--current",
                    str(FIXTURES / "cargo_fail_ab_reordered.txt"),
                    "--json-out",
                    str(root / "same.json"),
                ],
                check=False,
            )
            self.assertEqual(same.returncode, 0)
            added = subprocess.run(
                [
                    sys.executable,
                    str(COMPARE),
                    "--baseline",
                    str(FIXTURES / "cargo_fail_ab.txt"),
                    "--current",
                    str(FIXTURES / "cargo_fail_abc.txt"),
                ],
                check=False,
                stdout=subprocess.DEVNULL,
            )
            self.assertEqual(added.returncode, 1)
            reduced = subprocess.run(
                [
                    sys.executable,
                    str(COMPARE),
                    "--baseline",
                    str(FIXTURES / "cargo_fail_ab.txt"),
                    "--current",
                    str(FIXTURES / "cargo_fail_a.txt"),
                ],
                check=False,
                stdout=subprocess.DEVNULL,
            )
            self.assertEqual(reduced.returncode, 0)

    def test_truncated_and_malformed_logs_fail_closed(self) -> None:
        for fixture in ("cargo_truncated.txt", "cargo_malformed.txt"):
            with self.subTest(fixture=fixture):
                with self.assertRaises(compare_test_failures.ParseError):
                    compare_test_failures.parse_log(FIXTURES / fixture)


class EvidenceRunnerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_evidence(
        self,
        task: str,
        command: list[str],
        *,
        inputs: str = "self-test input",
        artifacts: list[str] | None = None,
        metadata: Path | None = None,
    ) -> subprocess.CompletedProcess[str]:
        args = [
            "bash",
            str(RUNNER),
            "--task",
            task,
            "--category",
            "targeted-test",
            "--stem",
            "case",
            "--inputs",
            inputs,
        ]
        for artifact in artifacts or []:
            args.extend(["--artifact", artifact])
        if metadata is not None:
            args.extend(["--metadata", str(metadata)])
        args.extend(["--", *command])
        return subprocess.run(args, cwd=self.root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def manifest(self, task: str) -> dict[str, object]:
        path = self.root / "target/codex-runs" / task / "manifest.json"
        return json.loads(path.read_text(encoding="utf-8"))

    def test_pass_fail_command_not_found_and_signal_exit_codes(self) -> None:
        cases = [
            ("pass", [sys.executable, "-c", "print('ok')"], 0),
            ("fail", ["bash", "-c", "exit 7"], 7),
            ("missing", ["codex-command-that-does-not-exist-9f91"], 127),
            ("signal", ["bash", "-c", "kill -TERM $$"], 143),
        ]
        for task, command, expected in cases:
            with self.subTest(task=task):
                result = self.run_evidence(task, command)
                self.assertEqual(result.returncode, expected, result.stderr)
                run = self.manifest(task)["runs"][0]
                self.assertEqual(run["exit_code"], expected)

    def test_special_characters_are_preserved(self) -> None:
        inputs = "中文 空格 $HOME 'quote'\nsecond line"
        argument = "参数 with spaces $() 'quoted'"
        result = self.run_evidence(
            "special",
            [sys.executable, "-c", "import sys; assert sys.argv[1] == sys.argv[2]", argument, argument],
            inputs=inputs,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        run = self.manifest("special")["runs"][0]
        self.assertEqual(run["inputs"], inputs)
        self.assertEqual(run["command"][-2:], [argument, argument])

    def test_manifest_schema_three_has_no_docs_impact(self) -> None:
        result = self.run_evidence("schema-three", [sys.executable, "-c", "pass"])
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.manifest("schema-three")
        self.assertEqual(manifest["schema_version"], 3)
        self.assertNotIn("docs_impact", manifest)

        legacy = copy.deepcopy(manifest)
        legacy["docs_impact"] = {"status": "not-needed"}
        with self.assertRaises(render_evidence.ManifestError):
            render_evidence.validate_manifest(legacy)

    def test_docs_impact_metadata_is_rejected(self) -> None:
        metadata = self.root / "legacy-metadata.json"
        metadata.write_text('{"docs_impact": {"status": "not-needed"}}\n', encoding="utf-8")
        result = self.run_evidence(
            "legacy-docs-impact",
            [sys.executable, "-c", "pass"],
            metadata=metadata,
        )
        self.assertEqual(result.returncode, 70)
        self.assertIn("unsupported task metadata keys: docs_impact", result.stderr)

    def test_command_arguments_after_double_dash_are_preserved(self) -> None:
        command = [
            sys.executable,
            "-c",
            "import sys; assert sys.argv[1:] == ['--', '--check']",
            "--",
            "--check",
        ]
        result = self.run_evidence("double-dash", command)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest("double-dash")["runs"][0]["command"], command)

    def test_manifest_update_failure_returns_nonzero(self) -> None:
        metadata = self.root / "invalid-metadata.json"
        metadata.write_text("not json\n", encoding="utf-8")

        result = self.run_evidence(
            "manifest-failure",
            [sys.executable, "-c", "pass"],
            metadata=metadata,
        )

        self.assertEqual(result.returncode, 70)
        self.assertIn("manifest_update=FAIL", result.stderr)

    def test_concurrent_runs_do_not_overwrite(self) -> None:
        base = [
            "bash",
            str(RUNNER),
            "--task",
            "parallel",
            "--category",
            "targeted-test",
            "--stem",
            "case",
            "--inputs",
            "parallel fixture",
            "--",
            "bash",
            "-c",
        ]
        first = subprocess.Popen([*base, "printf first"], cwd=self.root)
        second = subprocess.Popen([*base, "printf second"], cwd=self.root)
        self.assertEqual(first.wait(), 0)
        self.assertEqual(second.wait(), 0)
        runs = self.manifest("parallel")["runs"]
        self.assertEqual(len(runs), 2)
        self.assertEqual(len({run["log"] for run in runs}), 2)
        self.assertTrue(all(Path(run["log"]).is_file() for run in runs))

    def test_renderer_fails_on_missing_or_inconsistent_evidence(self) -> None:
        result = self.run_evidence("render", [sys.executable, "-c", "print('ok')"])
        self.assertEqual(result.returncode, 0)
        manifest = self.manifest("render")
        render_evidence.validate_manifest(manifest)
        rendered = render_evidence.render(manifest)
        self.assertIn("Build：未跑", rendered)
        self.assertIn("定向测试：", rendered)

        bad_status = copy.deepcopy(manifest)
        status_path = Path(bad_status["runs"][0]["status_file"])
        original = status_path.read_text(encoding="utf-8")
        status_path.write_text(original.replace("exit_code=0", "exit_code=9"), encoding="utf-8")
        with self.assertRaises(render_evidence.ManifestError):
            render_evidence.validate_manifest(bad_status)

        status_path.write_text(original, encoding="utf-8")
        Path(manifest["runs"][0]["log"]).unlink()
        with self.assertRaises(render_evidence.ManifestError):
            render_evidence.validate_manifest(manifest)

    def test_renderer_rejects_missing_registered_artifact(self) -> None:
        result = self.run_evidence(
            "artifact",
            [sys.executable, "-c", "print('ok')"],
            artifacts=["json=out/missing.json"],
        )
        self.assertEqual(result.returncode, 0)
        with self.assertRaises(render_evidence.ManifestError):
            render_evidence.validate_manifest(self.manifest("artifact"))

    def test_renderer_wraps_links_with_spaces(self) -> None:
        path = self.root / "evidence with spaces.log"
        path.write_text("ok\n", encoding="utf-8")
        self.assertIn(f"(<{path.resolve()}>)", render_evidence._link("evidence", str(path)))


class RepositoryFactsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.repo = Path(self.temporary.name)
        (self.repo / "docs").mkdir()
        (self.repo / "docs/A.md").write_text(
            "# A\n\n"
            "> 文档角色：canonical\n"
            "> 生命周期状态：current\n"
            "> 领域键：test.a\n"
            "> 当前真源：self\n"
            "> 摘要：documents the stable application fixture\n\n"
            "Current application behavior.\n",
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_broken_markdown_link_is_reported(self) -> None:
        original = (self.repo / "docs/A.md").read_text(encoding="utf-8")
        (self.repo / "docs/A.md").write_text(original + "\n[missing](missing.md)\n", encoding="utf-8")
        errors = check_repository_facts.check_markdown_links(self.repo, ["docs/A.md"])
        self.assertTrue(any("broken Markdown link" in error for error in errors))

    def test_markdown_link_with_spaces(self) -> None:
        (self.repo / "docs/target file.md").write_text("# Target\n", encoding="utf-8")
        original = (self.repo / "docs/A.md").read_text(encoding="utf-8")
        (self.repo / "docs/A.md").write_text(original + "\n[target](<target file.md>)\n", encoding="utf-8")
        self.assertEqual(check_repository_facts.check_markdown_links(self.repo, ["docs/A.md"]), [])

    def test_secondary_command_parsers_accept_multiline_arms_and_fail_closed(self) -> None:
        bake = '''
match args[i].as_str() {
    "all"
    | "trade"
    | "manufacture" => {
        match nested {
            "internal" => {}
            other => {}
        }
    }
    "validate" => {}
    "verify" => {}
    /* "commented" => {} */
    "--help" | "-h" => {}
    other => {}
}
'''
        self.assertEqual(
            check_repository_facts.parse_bake_actions(bake),
            ({"all", "trade", "manufacture", "validate", "verify"}, None),
        )
        profile = '''
match args.first().map(String::as_str) {
    Some(
        "layout-full"
    ) => {
        match nested {
            Some("internal") => action(),
            _ => fallback(),
        }
    },
    Some("analyze-compare") => action(),
    Some("bake-dependencies") => action(),
    /* Some("commented") => action(), */
    _ => fallback(),
}
'''
        self.assertEqual(
            check_repository_facts.parse_profile_actions(profile),
            ({"layout-full", "analyze-compare", "bake-dependencies"}, None),
        )
        for malformed in ("", 'match args[i].as_str() { "all" => {} }'):
            with self.subTest(parser="bake", malformed=malformed):
                self.assertIsNotNone(check_repository_facts.parse_bake_actions(malformed)[1])
        for malformed in ("", 'match args.first().map(String::as_str) { _ => {} }'):
            with self.subTest(parser="profile", malformed=malformed):
                self.assertIsNotNone(check_repository_facts.parse_profile_actions(malformed)[1])

        pseudo_bake_fallback = '''
match args[i].as_str() {
    "all" => {}
    "--help" | "-h" => {}
    /* other => {} */
}
match unrelated { other => {} }
'''
        self.assertIsNotNone(
            check_repository_facts.parse_bake_actions(pseudo_bake_fallback)[1]
        )
        later_profile_fallback = '''
match args.first().map(String::as_str) {
    Some("layout-full") => action(),
}
match unrelated { _ => fallback() }
'''
        self.assertIsNotNone(
            check_repository_facts.parse_profile_actions(later_profile_fallback)[1]
        )

    def test_secondary_command_contract_rejects_source_and_docs_omitting_help_action(self) -> None:
        core_src = self.repo / "crates/infra-core/src"
        cli_commands = self.repo / "crates/infra-cli/src/commands"
        core_src.mkdir(parents=True)
        cli_commands.mkdir(parents=True)
        (core_src / "lib.rs").write_text("pub mod layout;\n", encoding="utf-8")
        (cli_commands / "bake.rs").write_text(
            '''
match args[i].as_str() {
    "all" => {}
    "--help" | "-h" => {}
    other => {}
}
eprintln!("infra-cli bake all");
eprintln!("infra-cli bake verify");
''',
            encoding="utf-8",
        )
        (cli_commands / "profile.rs").write_text(
            '''
match args.first().map(String::as_str) {
    Some("layout-full") => action(),
    _ => fallback(),
}
eprintln!("infra-cli profile layout-full");
''',
            encoding="utf-8",
        )
        (self.repo / "docs/PROJECT_MAP.md").write_text(
            "## `infra-core` 模块索引\n\n"
            "| 模块 | 职责 |\n"
            "|---|---|\n"
            "| `layout` | layout |\n\n"
            "### `layout/` Owner\n\n"
            "## `infra-cli` 命令\n\n"
            "| 命令 | 用途 |\n"
            "|---|---|\n"
            "| `bake all` | bake |\n"
            "| `profile layout-full` | profile |\n\n"
            "### `infra-cli` 源码 Owner\n",
            encoding="utf-8",
        )

        errors = check_repository_facts.check_project_map_owner_contract(self.repo)
        self.assertTrue(
            any("bake source/help mismatch" in error and "verify" in error for error in errors)
        )

    def test_project_map_protected_facts_reject_semantic_reversal(self) -> None:
        valid = "\n".join(check_repository_facts.PROJECT_MAP_PROTECTED_FACTS)
        self.assertEqual(check_repository_facts.check_project_map_protected_facts(valid), [])

        shared_owner = (
            "`src/commands/plan_compute.rs` | `plan` / `serve` 共用的单次 rotation + "
            "profile/MAA 编排"
        )
        reversed_owner = valid.replace(
            shared_owner,
            "`src/commands/plan_compute.rs` | `plan` / `serve` 各自重新计算 rotation",
        )
        self.assertTrue(
            any(
                shared_owner in error
                for error in check_repository_facts.check_project_map_protected_facts(
                    reversed_owner
                )
            )
        )

        frontend_route = (
            "Next BFF 使用 `serve` / `plan.compute`；一次性调用使用 `plan` / "
            "`--maa-out`。"
        )
        reversed_route = valid.replace(
            frontend_route,
            "Next BFF 使用 `plan`；一次性调用使用 `serve` / `plan.compute`。",
        )
        self.assertTrue(
            any(
                frontend_route in error
                for error in check_repository_facts.check_project_map_protected_facts(
                    reversed_route
                )
            )
        )

    def test_project_map_retrieval_routes_cannot_disappear(self) -> None:
        valid = "\n".join(check_repository_facts.PROJECT_MAP_PROTECTED_FACTS)
        for route in check_repository_facts.PROJECT_MAP_RETRIEVAL_FACTS:
            with self.subTest(route=route):
                errors = check_repository_facts.check_project_map_protected_facts(
                    valid.replace(route, "")
                )
                self.assertTrue(any(route in error for error in errors))

    def _write_navigation_fixture(
        self, *, extra: str = "", sections: list[str] | None = None
    ) -> None:
        (self.repo / "AGENTS.md").write_text(
            "根 `AGENTS.md` 是任务分类、真源顺序和项目硬门禁的唯一入口。\n",
            encoding="utf-8",
        )
        if sections is None:
            sections = [
                "## 1. 从症状开始",
                "## 2. 按入口阅读",
                "## 3. 按模块阅读",
                "## 4. 接口与验证",
                "## 5. 外部机制与历史材料",
            ]
        body = "\n\n".join(f"{heading}\n\nReader navigation." for heading in sections)
        retrieval_routes = "\n".join(check_repository_facts.INDEX_RETRIEVAL_FACTS)
        (self.repo / "docs/INDEX.md").write_text(
            "本文不维护第二份分类表；任务路由见 [AGENTS.md](../AGENTS.md)。\n\n"
            "机器可读注册表：docs/agent-index/routes.json。\n\n"
            f"{body}\n{retrieval_routes}\n{extra}",
            encoding="utf-8",
        )
        (self.repo / "docs/GLOSSARY.md").write_text("current terms\n", encoding="utf-8")

    def test_navigation_contract_rejects_parallel_classifier_shapes(self) -> None:
        self._write_navigation_fixture()
        self.assertEqual(check_repository_facts.check_navigation_contract(self.repo), [])

        direct_skill_route = (
            "\n[Debug Skill](../.agents/skills/arknights-maintenance/SKILL.md)\n"
        )
        self._write_navigation_fixture(extra=direct_skill_route)
        self.assertTrue(
            any(
                "routes project Skills directly" in error
                for error in check_repository_facts.check_navigation_contract(self.repo)
            )
        )

        self._write_navigation_fixture(sections=["## 1. 开始", "## 2. 使用与集成"])
        self.assertTrue(
            any(
                "five-section reader IA" in error
                for error in check_repository_facts.check_navigation_contract(self.repo)
            )
        )

    def test_navigation_contract_rejects_missing_owner_retrieval_route(self) -> None:
        self._write_navigation_fixture()
        index_path = self.repo / "docs/INDEX.md"
        valid = index_path.read_text(encoding="utf-8")
        for route in check_repository_facts.INDEX_RETRIEVAL_FACTS:
            with self.subTest(route=route):
                index_path.write_text(valid.replace(route, ""), encoding="utf-8")
                errors = check_repository_facts.check_navigation_contract(self.repo)
                self.assertTrue(any(route in error for error in errors))
        index_path.write_text(valid, encoding="utf-8")

    def test_repository_cli_map_matches_dispatch(self) -> None:
        repo = CODEX_DIR.parents[1]
        self.assertEqual(check_repository_facts.check_cli_help_map(repo), [])

    def test_repository_project_map_matches_current_owners(self) -> None:
        repo = CODEX_DIR.parents[1]
        self.assertEqual(check_repository_facts.check_project_map_owner_contract(repo), [])

    def test_repository_navigation_contract(self) -> None:
        repo = CODEX_DIR.parents[1]
        self.assertEqual(check_repository_facts.check_navigation_contract(repo), [])


class TaskScopeTests(unittest.TestCase):
    def manifest(self) -> dict[str, object]:
        invariant = "all evidence commands remain inside the declared task boundary"
        return {
            "schema_version": 3,
            "change_scope": {
                "invariant": invariant,
                "root_cause_layer": "scripts/codex",
                "required_paths": ["src/app.py"],
                "allowed_consumers": [],
                "proof_paths": ["tests/**"],
                "explicitly_deferred": ["src/deferred.py"],
            },
            "scope_expansions": [],
            "side_findings": [],
            "reviewer": {
                "status": "reviewed",
                "scope_invariant": invariant,
                "changed_paths": ["src/app.py"],
                "scope_expansion_ids": [],
            },
        }

    def test_valid_scope(self) -> None:
        self.assertEqual(check_task_scope.run_checks(self.manifest(), {"src/app.py"}), [])

    def test_legacy_or_unversioned_manifest_is_rejected(self) -> None:
        manifest = self.manifest()
        del manifest["schema_version"]
        self.assertTrue(
            any("schema_version=3" in error for error in check_task_scope.run_checks(manifest, {"src/app.py"}))
        )

        manifest = self.manifest()
        manifest["docs_impact"] = {"status": "not-needed"}
        self.assertTrue(
            any("does not allow docs_impact" in error for error in check_task_scope.run_checks(manifest, {"src/app.py"}))
        )

    def test_undeclared_and_deferred_paths_fail(self) -> None:
        manifest = self.manifest()
        manifest["reviewer"]["changed_paths"] = ["other.py"]
        errors = check_task_scope.run_checks(manifest, {"other.py"})
        self.assertTrue(any("outside declared scope" in error for error in errors))

        manifest = self.manifest()
        manifest["reviewer"]["changed_paths"] = ["src/deferred.py"]
        errors = check_task_scope.run_checks(manifest, {"src/deferred.py"})
        self.assertTrue(any("explicitly deferred" in error for error in errors))

    def test_expansion_requires_reason_and_reviewer_history(self) -> None:
        manifest = self.manifest()
        manifest["scope_expansions"] = [
            {"id": "more", "paths": ["src/more.py"], "reason": "short", "evidence": "line 1"}
        ]
        manifest["reviewer"]["changed_paths"] = ["src/app.py", "src/more.py"]
        errors = check_task_scope.run_checks(manifest, {"src/app.py", "src/more.py"})
        self.assertTrue(any("reason must explain" in error for error in errors))
        self.assertTrue(any("scope_expansion_ids" in error for error in errors))

    def test_deferred_side_finding_cannot_be_implemented(self) -> None:
        manifest = self.manifest()
        manifest["change_scope"]["allowed_consumers"] = ["src/adjacent.py"]
        manifest["side_findings"] = [
            {
                "summary": "adjacent issue",
                "disposition": "deferred",
                "paths": ["src/adjacent.py"],
            }
        ]
        manifest["reviewer"]["changed_paths"] = ["src/adjacent.py"]
        errors = check_task_scope.run_checks(manifest, {"src/adjacent.py"})
        self.assertTrue(any("deferred side finding" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
