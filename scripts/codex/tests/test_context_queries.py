from __future__ import annotations

import contextlib
import copy
import io
import json
import os
import subprocess
import tempfile
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from scripts import assert_rotation_invariants as batch
from scripts.codex import _query_output as output
from scripts.codex import index_research_reports as index
from scripts.codex import query_rotation_report as rotation
from scripts.codex import query_runtime_data as runtime


def report() -> dict:
    return {
        "profile": "abc", "peak_plan": {}, "assignment_plans": [{}], "teams": [],
        "daily": {"trade": 1, "manufacture": 2, "power": 3},
        "shifts": [{"index": 4, "plan_index": 0, "duration_hours": 8,
                    "transition": "replace_rooms", "active_teams": [], "resting_team": "alpha",
                    "efficiencies": {"trade_efficiency": 1, "manufacture_efficiency": 2, "power_efficiency": 3},
                    "weighted_trade": 1, "weighted_manufacture": 2, "weighted_power": 3,
                    "assignment": {"rooms": [{"room_id": "control", "operators": [
                        {"name": "灵知", "elite": 2, "level": 60, "rarity": 6}]}]}}],
    }


class RotationQueries(unittest.TestCase):
    def load(self, value):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rotation.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            return rotation.load_report(path)[0]

    def test_valid_omitted_optional_fields_and_saved_index(self):
        value = self.load(report())
        args = SimpleNamespace(shift=5, room="control", operator="灵知")
        summary = rotation.summary(value, "", args)
        self.assertEqual(summary["matched_shift_count"], 1)
        self.assertEqual(summary["warning_count"], 0)
        self.assertEqual(summary["shifts"][0]["plan_path"], "/assignment_plans/0")
        args.shift = 1
        self.assertEqual(rotation.summary(value, "", args)["matched_shift_count"], 0)

    def test_reject_wrong_containers_indices_and_missing_metrics(self):
        variants = []
        value = report(); value["shifts"][0]["assignment"]["rooms"] = {}; variants.append(value)
        value = report(); value["shifts"][0]["index"] = True; variants.append(value)
        value = report(); value["shifts"][0]["plan_index"] = 1; variants.append(value)
        value = report(); value["shifts"].append(copy.deepcopy(value["shifts"][0])); variants.append(value)
        value = report(); del value["daily"]["trade"]; variants.append(value)
        value = report(); value["shifts"][0]["assignment"]["rooms"] *= 2; variants.append(value)
        value = report(); value["shifts"][0]["efficiencies"]["room_lines"] = "wrong"; variants.append(value)
        variants.append({"schema_version": 4, "rotation": {"daily_trade_efficiency": 1}})
        for value in variants:
            with self.subTest(value=value), self.assertRaises(output.QueryError):
                self.load(value)

    def test_diff_retains_level_duration_actions_and_nested_elapsed(self):
        old = report(); old["elapsed"] = 1; old["peak_plan"]["elapsed"] = 4
        new = copy.deepcopy(old); new["elapsed"] = 2; new["peak_plan"]["elapsed"] = 5
        new["shifts"][0]["duration_hours"] = 12
        new["shifts"][0]["assignment"]["rooms"][0]["operators"][0]["level"] = 90
        new["shifts"][0]["drone"] = {"room_id": "trade_1"}
        paths = {x["path"] for x in rotation.differences(old, new)}
        self.assertNotIn("/elapsed", paths)
        self.assertIn("/peak_plan/elapsed", paths)
        self.assertIn("/shifts/0/duration_hours", paths)
        self.assertIn("/shifts/0/assignment/rooms/0/operators/0/level", paths)
        self.assertIn("/shifts/0/drone", paths)


class RuntimeQueries(unittest.TestCase):
    def test_raw_bindings_exact_references_and_missing_skill_are_facts(self):
        records = [
            {"catalog": "operator_instances.json", "id": "灵知@tier_up", "json_pointer": "/instances/x",
             "record": {"name": "灵知", "facilities": {"control": {"buff_ids": ["buff", "missing"], "stepwise": True}}}},
            {"catalog": "skill_table.json", "id": "buff", "json_pointer": "/skills/0", "record": {"id": "buff", "atoms": []}},
            {"catalog": "base_systems.json", "id": "named", "json_pointer": "/systems/0",
             "record": {"label": "含灵知的说明"}},
            {"catalog": "base_systems.json", "id": "reference", "json_pointer": "/systems/1",
             "record": {"operators": [{"name": "灵知"}]}},
        ]
        matches, missing = runtime.query(records, "operator", "灵知")
        self.assertEqual({m["id"] for m in matches}, {"灵知@tier_up", "buff", "reference"})
        self.assertEqual(missing, ["missing"])
        self.assertEqual(matches[0]["record"]["facilities"]["control"]["buff_ids"], ["buff", "missing"])
        self.assertEqual(runtime.exact_references({"a/b~c": "x"}, "x"), ["/a~1b~0c"])

    def test_malformed_catalog_fails_instead_of_empty_query(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "operator_instances.json").write_text('{"instances": []}', encoding="utf-8")
            with self.assertRaisesRegex(output.QueryError, "instances"):
                runtime.load_catalogs(path)


class ResearchIndex(unittest.TestCase):
    def test_claim_ranges_and_two_line_coordinates(self):
        text = "[0] 首条\n - docs/模块/贸易求解.md:33 :: 引文\n\n[1] NOT-FOUND\n - rules.rs:288 :: 引文\n"
        rows = index.sections(text)
        self.assertEqual((rows[0]["start_line"], rows[0]["end_line"]), (1, 3))
        self.assertEqual(rows[0]["references"][0]["report_line"], 2)
        self.assertEqual(rows[0]["references"][0]["source_line"], 33)
        self.assertEqual(rows[1]["references"][0]["source_path"], "rules.rs")
        self.assertEqual(rows[1]["explicit_unknown_markers"][0]["text"], "NOT-FOUND")

    def test_user_metadata_fences_and_verbatim_ranges(self):
        text = ("## 0001 user time\r\nmessage `m` | part `p` | synthetic=True\r\n\r\n"
                "```md\r\n## forged heading\r\n```\r\n### Child\r\ntext\r\n## 0002 user later\r\nbody\r\n")
        rows = index.sections(text)
        self.assertEqual([r["id"] for r in rows], ["0001", "L7", "0002"])
        self.assertEqual(rows[0]["end_line"], 8)
        self.assertEqual(rows[0]["message"]["flags"], {"synthetic": True})
        self.assertEqual(rows[-1]["message"]["flags"], {})
        self.assertEqual("".join(text.splitlines(keepends=True)[:rows[0]["end_line"]]),
                         text[:text.index("## 0002")])

    def test_workflow_uses_decoded_report_lines_and_pointer(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "workflow.output"
            path.write_text(json.dumps({"result": [{"key": "rules", "report": "[0] claim\nnext\n"}]}), encoding="utf-8")
            doc = index.documents(path)[0]
            self.assertEqual(doc["source"]["json_pointer"], "/result/0/report")
            self.assertEqual(index.sections(doc["text"])[0]["end_line"], 2)
            path.write_text('{"result": [{"key": "rules", "report": []}]}', encoding="utf-8")
            with self.assertRaises(output.QueryError):
                index.documents(path)

    def test_flags_in_body_do_not_impersonate_export_metadata(self):
        text = ("## 0001 user time\nmessage `real` | part `part`\n\n"
                "```md\nsynthetic=True ignored=True\n```\nmessage `fake`\n")
        metadata = index.sections(text)[0]["message"]
        self.assertEqual(metadata["flags"], {})
        self.assertEqual(metadata["message_id"], "real")


class BoundedOutput(unittest.TestCase):
    def test_valid_bounded_json_and_complete_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "complete.json"
            payload = {"schema": "test", "status": "ok", "matches": [{"text": "中文" * 2000}] * 12}
            args = SimpleNamespace(max_chars=1500, full=False, output=path)
            stream = io.StringIO()
            with contextlib.redirect_stdout(stream):
                output.emit(payload, args)
            self.assertLessEqual(len(stream.getvalue()), 1500)
            compact = json.loads(stream.getvalue())
            self.assertTrue(compact["output"]["truncated"])
            self.assertEqual(json.loads(path.read_text()), payload)

    def test_output_cannot_overwrite_input(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.json"; path.write_text("{}", encoding="utf-8")
            args = SimpleNamespace(max_chars=1500, full=False, output=path)
            with self.assertRaises(output.QueryError):
                output.emit({"source": {"path": str(path)}}, args)
            self.assertEqual(path.read_text(), "{}")

    def test_no_match_and_hardlink_cannot_overwrite_report(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.md"; path.write_text("# Original\ntext\n", encoding="utf-8")
            alias = Path(directory) / "alias.md"; os.link(path, alias)
            for destination in (path, alias):
                with patch("sys.argv", ["index", "--file", str(path), "--section", "missing",
                                         "--output", str(destination)]), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(index.main(), 2)
                self.assertEqual(path.read_text(), "# Original\ntext\n")

    def test_fallback_budget_includes_long_complete_file_path(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory).joinpath(*(["long" * 25] * 20), "complete.json")
            args = SimpleNamespace(max_chars=1500, full=False, output=path)
            stream = io.StringIO()
            with contextlib.redirect_stdout(stream):
                output.emit({"schema": "test", "status": "ok", "matches": ["x" * 7000]}, args)
            self.assertLessEqual(len(stream.getvalue()), 1500)
            self.assertEqual(json.loads(path.read_text())["matches"], ["x" * 7000])

    def test_large_index_keeps_a_recoverable_section(self):
        payload = {"schema": "test", "status": "ok", "document_count": 11,
                   "documents": [{"source": {"path": "r00.txt"}, "section_count": 30,
                                  "sections": [{"title": "claim" * 200, "start_line": i} for i in range(30)]} for _ in range(11)]}
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            output.emit(payload, SimpleNamespace(max_chars=6000, full=False, output=None))
        value = json.loads(stream.getvalue())
        self.assertLessEqual(len(stream.getvalue()), 6000)
        self.assertTrue(value["documents"])
        self.assertTrue(value["documents"][0]["sections"])


class BatchSnapshots(unittest.TestCase):
    def args(self, path, **values):
        return SimpleNamespace(output_dir=path, jobs=2, timeout=1, rotation="3", layout=None,
                               enable_support_combinations=False, no_fiammetta=False,
                               compare_to=None, **values)

    def test_parallel_later_box_updates_snapshot_before_first_returns(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); first = root / "first.json"; later = root / "later.json"
            ready = threading.Event(); release = threading.Event(); errors = []
            progress = batch.BatchProgress(root, 2, 999)

            def run(command, **kwargs):
                if command[command.index("--operbox") + 1] == str(first):
                    ready.set(); release.wait(5)
                return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

            def run_batch():
                try:
                    batch.run_batch(self.args(root), Path("/bin/true"), [("g", first), ("g", later)], progress)
                except Exception as error:
                    errors.append(error)

            with patch.object(batch.subprocess, "run", side_effect=run), patch.object(batch, "source_state", return_value={}):
                worker = threading.Thread(target=run_batch); worker.start(); ready.wait(5)
                try:
                    for _ in range(100):
                        if batch.read_progress(root)["checked"] == 1:
                            break
                        threading.Event().wait(.01)
                    snapshot = batch.read_progress(root)
                    self.assertEqual(snapshot["checked"], 1)
                    self.assertEqual(snapshot["last_result"]["box"], "later.json")
                    self.assertEqual(snapshot["state"], "running")
                finally:
                    release.set(); worker.join(5)
            self.assertFalse(errors)
            self.assertEqual(batch.read_progress(root)["state"], "completed")

    def test_timeout_and_compare_failure_terminal_states(self):
        for timeout, changed in [(True, 0), (False, 1)]:
            with self.subTest(timeout=timeout), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); progress = batch.BatchProgress(root, 1, 999)
                args = self.args(root); args.compare_to = Path("previous")
                result = subprocess.CompletedProcess([], 0, stdout="", stderr="")
                effect = subprocess.TimeoutExpired([], 1) if timeout else None
                with patch.object(batch.subprocess, "run", return_value=result, side_effect=effect), \
                     patch.object(batch, "source_state", return_value={}), \
                     patch.object(batch, "compare_runs", return_value=changed):
                    self.assertEqual(batch.run_batch(args, Path("/bin/true"), [("g", root / "box.json")], progress), 1)
                value = batch.read_progress(root)
                self.assertEqual(value["state"], "failed")
                self.assertEqual(value["failure_counts"]["timeout"], int(timeout))
                self.assertEqual(value["changed"], changed)

    def test_status_short_circuits_build_and_zero_box_finishes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); progress = batch.BatchProgress(root, 0, 999)
            with patch.object(batch, "source_state", return_value={}):
                self.assertEqual(batch.run_batch(self.args(root), Path("/bin/true"), [], progress), 0)
            with patch("sys.argv", ["batch", "--status", str(root)]), \
                 patch.object(batch.subprocess, "run", side_effect=AssertionError("must not run")), \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(batch.main(), 0)

    def test_build_failure_records_error_and_reject_corrupt_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); destination = root / "run"
            with patch("sys.argv", ["batch", "--output-dir", str(destination)]), \
                 patch.object(batch, "GROUPS", {}), \
                 patch.object(batch.subprocess, "run", side_effect=FileNotFoundError("cargo")), \
                 contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(batch.main(), 2)
            value = batch.read_progress(destination)
            self.assertEqual(value["phase"], "build")
            self.assertEqual(value["state"], "failed")
            value["checked"] = True
            (destination / "progress.json").write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(ValueError):
                batch.read_progress(destination)

    def test_process_start_failure_is_batch_error_not_solver_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); box = root / "box.json"; box.write_text("[]", encoding="utf-8")
            destination = root / "run"
            with patch("sys.argv", ["batch", "--skip-build", "--output-dir", str(destination)]), \
                 patch.object(batch, "GROUPS", {"g": root}), \
                 patch.object(batch.subprocess, "run", side_effect=FileNotFoundError("cli")), \
                 contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(batch.main(), 2)
            value = batch.read_progress(destination)
            self.assertEqual(value["state"], "failed")
            self.assertEqual(value["phase"], "solve")
            self.assertEqual(value["checked"], 0)
            self.assertEqual(sum(value["failure_counts"].values()), 0)

    def test_snapshot_rejects_false_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); progress = batch.BatchProgress(root, 2, 999)
            progress.update(state="completed", phase="finished", exit_code=0)
            with self.assertRaisesRegex(ValueError, "complete successful batch"):
                batch.read_progress(root)


if __name__ == "__main__":
    unittest.main()
