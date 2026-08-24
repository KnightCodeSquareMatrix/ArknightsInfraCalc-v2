import csv
import json
import tempfile
import unittest
from pathlib import Path

import sync_building_skills


class SyncBuildingSkillsTest(unittest.TestCase):
    def test_extracts_control_skills_from_upstream_shape(self):
        character = {
            "char_demo": {
                "name": "示例干员",
                "phases": [],
            }
        }
        building = {
            "chars": {
                "char_demo": {
                    "charId": "char_demo",
                    "buffChar": [
                        {
                            "buffData": [
                                {"buffId": "control_demo[000]", "cond": {"phase": 0, "level": 1}}
                            ]
                        }
                    ],
                }
            },
            "buffs": {
                "control_demo[000]": {
                    "buffName": "示例技能",
                    "roomType": "CONTROL",
                    "description": "示例描述",
                }
            },
        }

        rows = sync_building_skills.extract_rows(character, building, "v076")

        self.assertEqual(rows[0]["facility"], "control")
        self.assertEqual(rows[0]["tier"], "tier_0")
        self.assertEqual(rows[0]["buff_id"], "control_demo[000]")

    def test_update_skill_table_preserves_atoms_and_adds_empty_only_when_requested(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "skill_table.json"
            path.write_text(
                json.dumps(
                    {
                        "version": 1,
                        "skills": [
                            {
                                "id": "control_existing[000]",
                                "skill_name": "旧名称",
                                "facility": "control",
                                "tier": "tier_0",
                                "atoms": [{"id": "keep"}],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            rows = [
                {
                    "buff_id": "control_existing[000]",
                    "skill_name": "新名称",
                    "facility": "control",
                    "tier": "tier_0",
                },
                {
                    "buff_id": "control_new[000]",
                    "skill_name": "新技能",
                    "facility": "control",
                    "tier": "tier_0",
                },
            ]
            updated, added = sync_building_skills.update_skill_table(path, rows, add_missing=True)
            document = json.loads(path.read_text(encoding="utf-8"))

        self.assertEqual((updated, added), (1, 1))
        existing = next(skill for skill in document["skills"] if skill["id"] == "control_existing[000]")
        self.assertEqual(existing["atoms"], [{"id": "keep"}])
        new = next(skill for skill in document["skills"] if skill["id"] == "control_new[000]")
        self.assertEqual(new["atoms"], [])


if __name__ == "__main__":
    unittest.main()
