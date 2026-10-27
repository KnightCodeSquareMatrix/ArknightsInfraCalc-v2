import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "render_training_recommendations.py"
SPEC = importlib.util.spec_from_file_location("render_training_recommendations", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RenderTrainingRecommendationsTests(unittest.TestCase):
    def test_current_rules_render_v2_fields(self) -> None:
        rules = MODULE.load_rules(MODULE.DEFAULT_INPUT)
        rendered = MODULE.render_rules(rules, "data/training_recommendations.json")

        self.assertIn("# 基建练卡推荐规则验收稿", rendered)
        self.assertIn("schema version：2", rendered)
        self.assertIn("巫恋裁缝核", rendered)
        self.assertIn("裁缝β第三人", rendered)
        self.assertIn("source_system_id：`witch_long_beta`", rendered)
        self.assertIn("石英", rendered)
        self.assertIn("Castle-3", rendered)
        self.assertIn("红云", rendered)
        self.assertIn("类型：`system`", rendered)
        self.assertIn("类型：`standalone`", rendered)
        self.assertIn("核心组「红松制造成员」至少 2 人", rendered)

    def test_invalid_version_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bad.json"
            path.write_text(
                '{"version": 1, "rules": []}',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MODULE.RuleFormatError, "version"):
                MODULE.load_rules(path)

    def test_invalid_member_elite_rejected(self) -> None:
        rules = {
            "version": 2,
            "rules": [
                {
                    "id": "bad",
                    "kind": "standalone",
                    "scope": "independent",
                    "label": "bad",
                    "admission": {"required_core": [], "pick_one_core": []},
                    "members": [
                        {
                            "operator": "干员",
                            "role": "independent",
                            "priority": "P0",
                            "target": {"elite": "1"},
                        }
                    ],
                    "evidence": [],
                    "review": {"status": "confirmed", "conflicts": []},
                }
            ],
        }
        with self.assertRaisesRegex(MODULE.RuleFormatError, "elite"):
            MODULE.render_rules(rules, "fixture.json")

    def test_render_is_deterministic(self) -> None:
        rules = MODULE.load_rules(MODULE.DEFAULT_INPUT)
        first = MODULE.render_rules(rules, "fixture.json")
        second = MODULE.render_rules(rules, "fixture.json")
        self.assertEqual(first, second)

    def test_cli_schema_error_has_no_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bad.json"
            path.write_text(
                '{"version": 1, "rules": []}',
                encoding="utf-8",
            )
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--input", str(path)],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertEqual(result.returncode, 1)
        self.assertIn("error:", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
