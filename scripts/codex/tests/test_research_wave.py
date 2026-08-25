from __future__ import annotations

import sys
import unittest
from pathlib import Path


CODEX_DIR = Path(__file__).resolve().parents[1]
ROOT = CODEX_DIR.parents[1]
sys.path.insert(0, str(CODEX_DIR))

import build_research_wave  # noqa: E402


class ResearchWaveTests(unittest.TestCase):
    def test_dirty_worktree_is_blocked_by_default(self) -> None:
        plan = build_research_wave.build_plan(ROOT, allow_dirty_readonly=False)

        self.assertTrue(plan["task"]["dirty_worktree"])
        self.assertEqual(plan["task"]["state"], "blocked")
        self.assertFalse(plan["task"]["research_wave"]["scope"]["writes"])
        self.assertFalse(plan["task"]["research_wave"]["scope"]["private_inputs"])

    def test_explicit_public_readonly_override_plans_three_children(self) -> None:
        plan = build_research_wave.build_plan(ROOT, allow_dirty_readonly=True)

        self.assertEqual(plan["task"]["state"], "queued")
        self.assertEqual(len(plan["task"]["children"]), 3)
        self.assertEqual(plan["task"]["research_wave"]["join_policy"]["mode"], "all_required")
        self.assertTrue(plan["task"]["research_wave"]["join_policy"]["fail_closed"])
        self.assertTrue(all(child["write_policy"] == "readonly" for child in plan["task"]["children"]))
        self.assertEqual(
            [child["profile"] for child in plan["task"]["children"]],
            ["terra-explorer", "terra-explorer", "luna-extractor"],
        )


if __name__ == "__main__":
    unittest.main()
