import unittest

import audit_building_skill_unlocks as audit


def source_fixture():
    character = {
        "char_low": {"name": "低星示例", "rarity": 3},
        "char_high": {"name": "高星示例", "rarity": 5},
    }
    building = {
        "chars": {
            "char_low": {
                "buffChar": [
                    {"buffData": [{"buffId": "manu_low", "cond": {"phase": 0, "level": 1}}]},
                    {"buffData": [{"buffId": "trade_low", "cond": {"phase": 1, "level": 1}}]},
                ]
            },
            "char_high": {
                "buffChar": [
                    {"buffData": [{"buffId": "manu_high", "cond": {"phase": 0, "level": 1}}]},
                    {"buffData": [{"buffId": "trade_high", "cond": {"phase": 2, "level": 1}}]},
                ]
            },
        },
        "buffs": {
            "manu_low": {"roomType": "MANUFACT"},
            "trade_low": {"roomType": "TRADE"},
            "manu_high": {"roomType": "MANUFACT"},
            "trade_high": {"roomType": "TRADE"},
        },
    }
    return character, building


class AuditBuildingSkillUnlocksTest(unittest.TestCase):
    def test_low_star_tier_up_is_phase_one(self):
        character, building = source_fixture()
        instances = {
            "低星示例@tier_0": {"name": "低星示例", "tier": "tier_0", "facilities": {"manufacture": {"buff_ids": ["manu_low"]}}},
            "低星示例@tier_up": {"name": "低星示例", "tier": "tier_up", "facilities": {"manufacture": {"buff_ids": ["manu_low"]}, "trade": {"buff_ids": ["trade_low"]}}},
            "高星示例@tier_0": {"name": "高星示例", "tier": "tier_0", "facilities": {"manufacture": {"buff_ids": ["manu_high"]}}},
            "高星示例@tier_up": {"name": "高星示例", "tier": "tier_up", "facilities": {"manufacture": {"buff_ids": ["manu_high"]}, "trade": {"buff_ids": ["trade_high"]}}},
        }
        self.assertEqual(audit.audit_document(character, building, instances), [])

    def test_future_unlock_in_tier_zero_is_reported(self):
        character, building = source_fixture()
        instances = {
            "低星示例@tier_0": {"name": "低星示例", "tier": "tier_0", "facilities": {"trade": {"buff_ids": ["trade_low"]}}},
            "低星示例@tier_up": {"name": "低星示例", "tier": "tier_up", "facilities": {}},
        }
        findings = audit.audit_document(character, building, instances)
        kinds = {finding["kind"] for finding in findings}
        self.assertIn("future_unlock_in_tier_0", kinds)
        self.assertIn("missing_phase_0", kinds)
        self.assertIn("missing_tier_up", kinds)

    def test_unmapped_local_id_is_not_reported_as_phase_error(self):
        character, building = source_fixture()
        building["chars"]["char_low"]["buffChar"][0]["buffData"][0]["buffId"] = "source_manu"
        building["buffs"]["source_manu"] = {"roomType": "MANUFACT"}
        instances = {
            "低星示例@tier_0": {
                "name": "低星示例",
                "tier": "tier_0",
                "facilities": {"manufacture": {"buff_ids": ["local_manu"]}},
            }
        }
        findings = audit.audit_document(character, building, instances)
        self.assertIn("unmapped_local_binding", {finding["kind"] for finding in findings})
        self.assertNotIn("future_unlock_in_tier_0", {finding["kind"] for finding in findings})


if __name__ == "__main__":
    unittest.main()
