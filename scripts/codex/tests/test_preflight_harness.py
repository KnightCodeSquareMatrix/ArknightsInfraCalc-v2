from __future__ import annotations

import sys
import unittest
from pathlib import Path


CODEX_DIR = Path(__file__).resolve().parents[1]
ROOT = CODEX_DIR.parents[1]
sys.path.insert(0, str(CODEX_DIR))

import preflight_harness  # noqa: E402


class HarnessPreflightTests(unittest.TestCase):
    def test_project_preflight_passes_without_starting_runtime(self) -> None:
        report = preflight_harness.preflight(ROOT)

        self.assertEqual(report["status"], "preflight_passed")
        self.assertFalse(report["scope"]["writes"])
        self.assertFalse(report["scope"]["private_inputs"])
        self.assertFalse(report["scope"]["runtime_start"])
        self.assertEqual(
            report["profiles_checked"],
            ["luna-extractor", "sol-reviewer", "terra-explorer"],
        )
        self.assertTrue(report["warnings"])

    def test_project_profiles_are_readonly(self) -> None:
        profiles = preflight_harness.discover_profiles(ROOT)

        for profile in ("terra-explorer", "luna-extractor", "sol-reviewer"):
            self.assertEqual(profiles[profile]["mode"], "subagent")
            for permission in ("edit", "bash", "task"):
                self.assertEqual(profiles[profile][f"permission.{permission}"], "deny")


if __name__ == "__main__":
    unittest.main()
