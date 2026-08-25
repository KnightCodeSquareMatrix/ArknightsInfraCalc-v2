import json
import hashlib
import subprocess
import subprocess
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory


SCRIPT = Path(__file__).parents[1] / "build_evidence_overlay.py"


def run_overlay(task: dict, manifest: dict, directory: str) -> subprocess.CompletedProcess[str]:
    root = Path(directory)
    base_sha = subprocess.run(["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
    task["task"]["base_sha"] = base_sha
    manifest = dict(manifest)
    manifest["schema_version"] = 3
    manifest["task"] = {"base_sha": task["task"]["base_sha"], "cwd": str(Path.cwd())}
    runs = []
    for index, run in enumerate(manifest.get("runs", [])):
        run = dict(run)
        run.setdefault("base_sha", task["task"]["base_sha"])
        run.setdefault("input_sha256", "b" * 64)
        run.setdefault("command_sha256", "c" * 64)
        log = root / f"run-{index}.log"
        status = root / f"run-{index}.status"
        log.write_text("exit_code=0\nresult_summary=PASS\n", encoding="utf-8")
        status.write_text("exit_code=0\nresult_summary=PASS\n", encoding="utf-8")
        run["log"] = str(log)
        run["status_file"] = str(status)
        runs.append(run)
    manifest["runs"] = runs
    artifacts = []
    for index, artifact in enumerate(manifest.get("artifacts", [])):
        artifact = dict(artifact)
        artifact.setdefault("id", f"artifact-{index}")
        artifact.setdefault("run_id", runs[0]["id"] if runs else f"run-artifact-{index}")
        if not runs:
            run = {"id": artifact["run_id"], "result": "PASS", "exit_code": 0, "base_sha": task["task"]["base_sha"], "input_sha256": "b" * 64, "command_sha256": "c" * 64, "log": str(root / "artifact.log"), "status_file": str(root / "artifact.status")}
            (root / "artifact.log").write_text("exit_code=0\nresult_summary=PASS\n", encoding="utf-8")
            (root / "artifact.status").write_text("exit_code=0\nresult_summary=PASS\n", encoding="utf-8")
            runs.append(run)
        artifact.setdefault("sha256", hashlib.sha256(Path(artifact["path"]).read_bytes()).hexdigest())
        artifact.setdefault("visibility", "project")
        artifact.setdefault("retention", "run")
        artifact.setdefault("producer", "test")
        artifact.setdefault("base_sha", task["task"]["base_sha"])
        artifact.setdefault("input_sha256", "b" * 64)
        artifact.setdefault("command_sha256", "c" * 64)
        artifacts.append(artifact)
    manifest["artifacts"] = artifacts
    task_path = root / "task.json"
    manifest_path = root / "manifest.json"
    output_path = root / "overlay.json"
    task_path.write_text(json.dumps(task), encoding="utf-8")
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--task",
            str(task_path),
            "--manifest",
            str(manifest_path),
            "--output",
            str(output_path),
        ],
        check=False,
        capture_output=True,
        text=True,
    )


class EvidenceOverlayTests(unittest.TestCase):
    def envelope(self, obligations: list[dict]) -> dict:
        normalized = [dict(obligation, kind=obligation.get("kind", "structure"), status=obligation.get("status", "pending")) for obligation in obligations]
        return {
            "protocol_version": "agent-harness-v0",
            "task": {
                "id": "test-task",
                "protocol_version": "agent-harness-v0",
                "project_profile": "test",
                "state": "succeeded",
                "write_policy": "readonly",
                "data_classification": "public",
                "base_sha": "a" * 40,
                "dirty_worktree": False,
                "children": [],
                "proof_obligations": normalized,
            },
        }

    def test_pass_run_verifies_explicit_obligation(self) -> None:
        with TemporaryDirectory() as directory:
            (Path(directory) / "pass.log").write_text("pass", encoding="utf-8")
            (Path(directory) / "pass.status").write_text("pass", encoding="utf-8")
            result = run_overlay(
                self.envelope([{"id": "test", "required": True, "evidence_refs": ["run-pass"]}]),
                {"runs": [{"id": "run-pass", "result": "PASS", "log": str(Path(directory) / "pass.log"), "status_file": str(Path(directory) / "pass.status")}], "artifacts": []},
                directory,
            )
            payload = json.loads((Path(directory) / "overlay.json").read_text())
        self.assertEqual(result.returncode, 0)
        self.assertEqual(payload["status"], "verified")
        self.assertEqual(payload["obligations"][0]["status"], "verified")

    def test_failed_run_blocks_required_obligation(self) -> None:
        with TemporaryDirectory() as directory:
            result = run_overlay(
                self.envelope([{"id": "test", "required": True, "evidence_refs": ["run-fail"]}]),
                {"runs": [{"id": "run-fail", "result": "FAIL", "log": "/tmp/fail.log"}], "artifacts": []},
                directory,
            )
            payload = json.loads((Path(directory) / "overlay.json").read_text())
        self.assertEqual(result.returncode, 1)
        self.assertEqual(payload["status"], "blocked")
        self.assertEqual(payload["obligations"][0]["status"], "failed")

    def test_missing_refs_and_no_refs_fail_closed(self) -> None:
        with TemporaryDirectory() as directory:
            result = run_overlay(
                self.envelope([
                        {"id": "missing", "required": True, "evidence_refs": ["unknown"]},
                        {"id": "not-run", "required": True},
                    ]),
                {"runs": [], "artifacts": []},
                directory,
            )
            payload = json.loads((Path(directory) / "overlay.json").read_text())
        self.assertEqual(result.returncode, 1)
        self.assertEqual(payload["status"], "blocked")
        self.assertEqual(payload["required_failures"], ["missing", "not-run"])
        self.assertEqual(payload["obligations"][1]["status"], "not_run")

    def test_artifact_reference_is_partial_not_verified(self) -> None:
        with TemporaryDirectory() as directory:
            artifact = Path(directory) / "report.md"
            artifact.write_text("semantic_join: blocked\n", encoding="utf-8")
            result = run_overlay(
                self.envelope([{"id": "join", "required": True, "evidence_refs": ["artifact-join"]}]),
                {"runs": [], "artifacts": [{"id": "artifact-join", "path": str(artifact), "kind": "join_report"}]},
                directory,
            )
            payload = json.loads((Path(directory) / "overlay.json").read_text())
        self.assertEqual(result.returncode, 1)
        self.assertEqual(payload["obligations"][0]["status"], "partial")

    def test_proof_status_artifact_preserves_blocked_semantics(self) -> None:
        with TemporaryDirectory() as directory:
            artifact = Path(directory) / "proof.json"
            artifact.write_text(
                json.dumps(
                    {
                        "artifact_type": "proof_status",
                        "obligation_id": "join",
                        "proof_status": "blocked",
                    }
                ),
                encoding="utf-8",
            )
            result = run_overlay(
                self.envelope([{"id": "join", "required": True, "evidence_refs": ["artifact-join"]}]),
                {"runs": [], "artifacts": [{"id": "artifact-join", "path": str(artifact), "kind": "proof_status"}]},
                directory,
            )
            payload = json.loads((Path(directory) / "overlay.json").read_text())
        self.assertEqual(result.returncode, 1)
        self.assertEqual(payload["obligations"][0]["status"], "blocked")
        self.assertEqual(payload["obligations"][0]["evidence"][0]["kind"], "proof_artifact")
