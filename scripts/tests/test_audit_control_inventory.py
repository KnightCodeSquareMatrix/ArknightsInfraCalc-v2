import csv
import json
import tempfile
import unittest
from pathlib import Path

from scripts.audit_control_inventory import extract_inventory, resolve_buff_ids


class ControlInventoryTests(unittest.TestCase):
    def test_stepwise_replaces_tier_zero_stem(self):
        result = resolve_buff_ids(
            "tier_2",
            {"stepwise": True, "buff_ids": ["control_x[010]", "control_y[000]"]},
            {"buff_ids": ["control_x[000]", "control_z[000]"]},
        )
        self.assertEqual(result, ["control_z[000]", "control_x[010]", "control_y[000]"])

    def test_empty_atoms_are_emitted_and_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "registry.csv").write_text(
                "序号,技能名,工作设施,产物限定,干员,需求精英,效率值,游戏原文\n"
                "1,Test,控制中枢,,,精0,,\n".replace(",,,精0", ",,Op,精0"),
                encoding="utf-8",
            )
            (root / "instances.json").write_text(
                json.dumps({"instances": {"Op@tier_0": {"name": "Op", "tier": "tier_0", "facilities": {"control": {"buff_ids": ["b[000]"]}}}}}),
                encoding="utf-8",
            )
            (root / "skills.json").write_text(
                json.dumps({"skills": [{"id": "b[000]", "facility": "control", "atoms": []}]}),
                encoding="utf-8",
            )
            rows, errors = extract_inventory(root / "registry.csv", root / "instances.json", root / "skills.json")
            self.assertEqual(rows[0]["status"], "unsupported")
            self.assertIn("empty atoms", rows[0]["reason"])
            self.assertTrue(any("empty atoms" in error for error in errors))

    def test_missing_binding_is_error_without_inventing_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "registry.csv").write_text("工作设施,干员,需求精英\n控制中枢,Op,精0\n", encoding="utf-8")
            for name, value in (("instances.json", {"instances": {}}), ("skills.json", {"skills": []})):
                (root / name).write_text(json.dumps(value), encoding="utf-8")
            rows, errors = extract_inventory(root / "registry.csv", root / "instances.json", root / "skills.json")
            self.assertEqual(rows, [])
            self.assertEqual(len(errors), 1)


if __name__ == "__main__":
    unittest.main()
