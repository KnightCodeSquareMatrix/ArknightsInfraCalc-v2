import hashlib
import json
import subprocess
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "completion_gate.py"


def envelope(obligations: list[dict[str, object]], *, base_sha: str = "a" * 40) -> dict[str, object]:
    return {
        "protocol_version": "agent-harness-v0",
        "task": {
            "id": "test-task",
            "protocol_version": "agent-harness-v0",
            "project_profile": "test",
            "state": "succeeded",
            "write_policy": "readonly",
            "data_classification": "public",
            "base_sha": base_sha,
            "dirty_worktree": False,
            "children": [],
            "proof_obligations": obligations,
        },
    }


def run_gate(tmp_path: Path, value: dict[str, object]) -> subprocess.CompletedProcess[str]:
    task = tmp_path / "task.json"
    task.write_text(json.dumps(value), encoding="utf-8")
    return subprocess.run([sys.executable, str(SCRIPT), "--task", str(task)], check=False, capture_output=True, text=True)


class CompletionGateTests(unittest.TestCase):
    def test_verified_obligation_without_evidence_is_rejected(self) -> None:
        from tempfile import TemporaryDirectory

        with TemporaryDirectory() as directory:
            result = run_gate(Path(directory), envelope([{"id": "children", "kind": "structure", "required": True, "status": "verified"}]))
        self.assertEqual(result.returncode, 2)
        self.assertIn("cannot be verified without evidence_refs", result.stderr)

    def test_non_verified_required_obligation_blocks(self) -> None:
        from tempfile import TemporaryDirectory

        with TemporaryDirectory() as directory:
            result = run_gate(Path(directory), envelope([{"id": "join", "kind": "review", "required": True, "status": "blocked"}]))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(json.loads(result.stdout)["status"], "blocked")

    def test_legacy_top_level_wave_is_rejected(self) -> None:
        from tempfile import TemporaryDirectory

        with TemporaryDirectory() as directory:
            result = run_gate(Path(directory), {"protocol_version": "research-wave-v0", "proof_obligations": []})
        self.assertEqual(result.returncode, 2)
        self.assertIn("legacy top-level wave is rejected", result.stderr)

    def test_manifest_artifact_digest_and_ref_are_required(self) -> None:
        from tempfile import TemporaryDirectory

        with TemporaryDirectory() as directory:
            root = Path(directory)
            base_sha = subprocess.run(["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
            artifact = root / "proof.json"
            artifact.write_text(json.dumps({"artifact_type": "proof_status", "obligation_id": "proof", "proof_status": "verified"}), encoding="utf-8")
            digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
            manifest = {
                "schema_version": 3,
                "task": {"base_sha": base_sha, "cwd": str(Path.cwd())},
                "runs": [{"id": "run-1", "result": "PASS", "exit_code": 0, "base_sha": base_sha, "input_sha256": "b" * 64, "command_sha256": "c" * 64, "log": str(root / "run.log"), "status_file": str(root / "run.status")}],
                "artifacts": [{
                    "id": "artifact:proof",
                    "kind": "evidence_log",
                    "path": str(artifact),
                    "sha256": digest,
                    "visibility": "project",
                    "retention": "run",
                    "producer": "test",
                    "run_id": "run-1",
                    "base_sha": base_sha,
                    "input_sha256": "b" * 64,
                    "command_sha256": "c" * 64,
                }],
            }
            (root / "run.log").write_text("exit_code=0\nresult_summary=PASS\n", encoding="utf-8")
            (root / "run.status").write_text("exit_code=0\nresult_summary=PASS\n", encoding="utf-8")
            task = root / "task.json"
            task_value = envelope([{
                "id": "proof", "kind": "structure", "required": True, "status": "verified", "evidence_refs": ["artifact:proof"]
            }])
            task_value["task"]["base_sha"] = base_sha
            task_value["task"]["dirty_worktree"] = False
            task.write_text(json.dumps(task_value), encoding="utf-8")
            manifest_path = root / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            result = subprocess.run([sys.executable, str(SCRIPT), "--task", str(task), "--manifest", str(manifest_path)], check=False, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
