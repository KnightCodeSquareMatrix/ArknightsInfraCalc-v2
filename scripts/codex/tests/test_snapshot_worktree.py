import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from scripts.codex.snapshot_worktree import SnapshotError, build_snapshot


class SnapshotWorktreeTests(unittest.TestCase):
    def init_repo(self, root: Path) -> None:
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.email", "test@example.invalid"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.name", "Snapshot Test"], check=True)
        (root / ".gitkeep").write_text("", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", ".gitkeep"], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "init"], check=True)

    def test_snapshot_records_files_symlinks_and_pending_disposition(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_repo(root)
            (root / "tracked.txt").write_text("content", encoding="utf-8")
            (root / "link").symlink_to("tracked.txt")
            output = root / "target" / "codex-runs" / "phase0.json"
            snapshot, ledger = build_snapshot(root, output, retries=1)

            entries = {entry["path"]: entry for entry in snapshot["entries"]}
            self.assertEqual(entries["tracked.txt"]["kind"], "file")
            self.assertEqual(entries["link"]["kind"], "symlink")
            self.assertEqual(ledger["entries"][0]["disposition"], "pending_user_disposition")
            self.assertEqual(snapshot["entry_count"], 3)

    def test_directory_digest_is_stable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_repo(root)
            (root / "a").mkdir()
            (root / "a" / "file").write_text("content", encoding="utf-8")
            first, _ = build_snapshot(root, root / "snapshot.json", retries=1)
            second, _ = build_snapshot(root, root / "snapshot.json", retries=1)
            self.assertEqual(first["inventory_sha256"], second["inventory_sha256"])

    def test_snapshot_marks_ignored_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_repo(root)
            (root / ".gitignore").write_text("ignored/\n", encoding="utf-8")
            (root / "ignored").mkdir()
            (root / "ignored" / "raw.log").write_text("local", encoding="utf-8")
            snapshot, ledger = build_snapshot(root, root / "snapshot.json", retries=1)
            records = {record["path"]: record for record in ledger["entries"]}
            self.assertIn("ignored", records["ignored/raw.log"]["state"])
            self.assertIn("untracked", records["ignored/raw.log"]["state"])
            self.assertEqual(snapshot["ignored_entry_count"], 2)

    def test_nested_git_directory_is_excluded(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_repo(root)
            (root / "nested").mkdir()
            (root / "nested" / ".git").mkdir()
            (root / "nested" / ".git" / "secret").write_text("private", encoding="utf-8")
            snapshot, _ = build_snapshot(root, root / "snapshot.json", retries=1)
            paths = {entry["path"] for entry in snapshot["entries"]}
            self.assertNotIn("nested/.git", paths)
            self.assertNotIn("nested/.git/secret", paths)

    def test_existing_output_is_not_overwritten(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_repo(root)
            output = root / "snapshot.json"
            output.write_text("keep", encoding="utf-8")
            from scripts.codex.snapshot_worktree import main
            import sys

            original = sys.argv
            try:
                sys.argv = ["snapshot_worktree.py", "--repo-root", str(root), "--output", str(output)]
                self.assertEqual(main(), 2)
            finally:
                sys.argv = original
            self.assertEqual(output.read_text(encoding="utf-8"), "keep")

    def test_snapshot_shape_is_json_serializable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_repo(root)
            (root / "file").write_text("x", encoding="utf-8")
            snapshot, ledger = build_snapshot(root, root / "snapshot.json", retries=1)
            json.dumps(snapshot)
            json.dumps(ledger)


if __name__ == "__main__":
    unittest.main()
