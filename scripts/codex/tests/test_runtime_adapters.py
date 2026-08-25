from __future__ import annotations

import sys
import unittest
from pathlib import Path


CODEX_DIR = Path(__file__).resolve().parents[1]
ROOT = CODEX_DIR.parents[1]
sys.path.insert(0, str(CODEX_DIR))

import probe_runtime_adapters  # noqa: E402


class RuntimeAdapterProbeTests(unittest.TestCase):
    def test_probe_is_non_launching_and_non_private(self) -> None:
        report = probe_runtime_adapters.probe(ROOT)

        self.assertEqual(report["protocol_version"], "agent-harness-v0")
        scope = report["task"]["adapter_probe"]["probe_scope"]
        self.assertFalse(scope["launch_runtime"])
        self.assertFalse(scope["network"])
        self.assertFalse(scope["private_inputs"])
        self.assertFalse(scope["writes"])
        self.assertEqual(
            {runtime["id"] for runtime in report["task"]["adapter_probe"]["runtimes"]},
            {
                "opencode-native",
                "mimo-code",
                "claude-agent-sdk",
                "openai-responses-multi-agent",
            },
        )

    def test_unavailable_or_partial_local_runtime_is_not_called_available(self) -> None:
        report = probe_runtime_adapters.probe(ROOT)
        for runtime in report["task"]["adapter_probe"]["runtimes"]:
            if runtime["id"] in {"mimo-code", "claude-agent-sdk"}:
                self.assertNotEqual(runtime["status"], "available")


if __name__ == "__main__":
    unittest.main()
