import json
import tempfile
import threading
import unittest
from pathlib import Path

from arknights_narrator.adapter import Narrator, NarratorConfig, validate_and_replace
from arknights_narrator.cache import AdmissionStatus, RedisKeys, admit, content_hmac
from arknights_narrator.mapping import CanonicalMapping, MappingError, MappingStore
from arknights_narrator.projection import project_report


ROOT = Path(__file__).parents[2]


def report():
    return {
        "schema_version": 2,
        "context": {},
        "newbie_section_status": "complete",
        "incomplete_newbie": [],
        "combinations": [
            {
                "id": "vermeil",
                "name": "红云组",
                "state": "complete",
                "completion_percent": 100,
                "facilities": ["manufacturing_station"],
                "members": [{"operator": "红云"}],
            },
            {
                "id": "human_fireworks",
                "name": "人间烟火组",
                "state": "needs_training",
                "completion_percent": 66,
                "facilities": ["trade_station", "control_center"],
                "members": [{"operator": "乌有"}],
            },
            {
                "id": "abyssal_hunters",
                "name": "深海猎人组",
                "state": "needs_review",
                "completion_percent": 50,
                "facilities": ["manufacturing_station", "control_center"],
                "members": [{"operator": "歌蕾蒂娅"}],
            },
        ],
        "recommendations": [
            {
                "operator": "乌有",
                "action": "train",
                "target": {"kind": "explicit", "elite": 2},
                "combination_id": "human_fireworks",
                "combination_name": "人间烟火组",
            }
        ],
    }


class NarratorTests(unittest.TestCase):
    def setUp(self):
        self.mapping = CanonicalMapping.load(ROOT / "data/canonical_mapping.json")
        self.value = project_report(report(), self.mapping)

    def test_projection_sorts_caps_and_excludes_needs_review(self):
        self.assertEqual([item["id"] for item in self.value.completed_combinations], ["vermeil"])
        self.assertEqual([item["id"] for item in self.value.incomplete_combinations], ["human_fireworks"])
        self.assertEqual(self.value.references["facts"]["completion:human_fireworks"]["value"], "66%")
        self.assertEqual(self.value.references["facts"]["facility:0"]["value"], "贸易站、控制中枢")

    def test_placeholder_replacement_uses_backend_facts(self):
        data = {
            "schema_version": 1,
            "sections": [
                {"id": "account_overview", "heading": "账号概况", "paragraphs": ["概况"]},
                {"id": "completed_combinations", "heading": "已成型组合", "paragraphs": ["{{combination:vermeil}}已经成型"]},
                {"id": "newbie_requirements", "heading": "基础必练、必收集", "paragraphs": ["基础已完成"]},
                {"id": "incomplete_combinations", "heading": "待完成组合", "paragraphs": ["{{combination:human_fireworks}}（{{fact:completion:human_fireworks}}）尚未完成"]},
                {"id": "training_recommendations", "heading": "练卡建议与高效率散件", "items": ["建议{{operator:乌有}}至{{fact:target:0}}，进入{{fact:facility:0}}。"]},
                {"id": "next_base_plan", "heading": "再次使用排班工具的提示", "paragraphs": ["完成后再排班"]},
            ],
        }
        rendered = validate_and_replace(json.dumps(data, ensure_ascii=False), self.value)
        text = json.dumps(rendered, ensure_ascii=False)
        self.assertIn("人间烟火组（66%）", text)
        self.assertIn("乌有至精二，进入贸易站、控制中枢", text)

    def test_direct_numeric_fact_is_rejected(self):
        data = {"schema_version": 1, "sections": [{"id": section, "heading": section, "paragraphs": ["精二"]} for section in [
            "account_overview", "completed_combinations", "newbie_requirements", "incomplete_combinations", "training_recommendations", "next_base_plan"
        ]]}
        with self.assertRaises(ValueError):
            validate_and_replace(data, self.value)

    def test_section_heading_is_canonical(self):
        data = {
            "schema_version": 1,
            "sections": [
                {"id": section, "heading": "模型自定义标题", "paragraphs": []}
                for section in [
                    "account_overview", "completed_combinations", "newbie_requirements",
                    "incomplete_combinations", "training_recommendations", "next_base_plan",
                ]
            ],
        }
        with self.assertRaises(ValueError):
            validate_and_replace(data, self.value)

    def test_unconfigured_llm_returns_fallback(self):
        result = Narrator(NarratorConfig(model="", api_key="")).generate(self.value)
        self.assertEqual(result.status, "fallback")
        self.assertEqual(result.fallback_reason, "llm_not_configured")

    def test_mapping_reload_failure_keeps_previous_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "mapping.json"
            path.write_text((ROOT / "data/canonical_mapping.json").read_text(encoding="utf-8"), encoding="utf-8")
            store = MappingStore(path)
            current = store.load()
            path.write_text("{}", encoding="utf-8")
            self.assertIs(store.reload(), current)

    def test_mapping_mismatch_rejects_report(self):
        broken = dict(report())
        broken["combinations"] = [dict(broken["combinations"][0], name="错误名称")]
        with self.assertRaises(MappingError):
            project_report(broken, self.mapping)

    def test_recommendation_combination_name_cannot_override_mapping(self):
        broken = json.loads(json.dumps(report()))
        broken["recommendations"][0]["combination_name"] = "伪造组合"
        with self.assertRaises(MappingError):
            project_report(broken, self.mapping)

    def test_content_hmac_is_stable_and_server_owned(self):
        kwargs = dict(
            locale="zh-CN", skill_version="skill.v1", prompt_version="prompt.v1",
            mapping_hash="mapping", model="model",
        )
        self.assertEqual(content_hmac(report(), "secret", **kwargs), content_hmac(report(), "secret", **kwargs))
        self.assertNotEqual(content_hmac(report(), "secret", **kwargs), content_hmac(report(), "other", **kwargs))

    def test_redis_admission_merges_one_hundred_duplicate_requests(self):
        redis = FakeRedis()
        results = []
        barrier = threading.Barrier(100)

        def request():
            barrier.wait()
            results.append(admit(redis, report(), "user-1", "secret", **self.cache_kwargs()))

        threads = [threading.Thread(target=request) for _ in range(100)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertEqual(sum(result.status == AdmissionStatus.ACCEPTED for result in results), 1)
        self.assertEqual(sum(result.status == AdmissionStatus.PROCESSING for result in results), 99)

    def test_ready_cache_is_checked_before_lock(self):
        redis = FakeRedis()
        first = admit(redis, report(), "user-1", "secret", **self.cache_kwargs())
        digest = first.content_hmac
        redis.values[RedisKeys.ready(digest)] = "cached"
        second = admit(redis, report(), "user-1", "secret", **self.cache_kwargs())
        self.assertEqual(second.status, AdmissionStatus.READY)
        self.assertEqual(second.cached_value, "cached")

    def test_redis_failure_returns_fallback_without_locking(self):
        result = admit(FailingRedis(), report(), "user-1", "secret", **self.cache_kwargs())
        self.assertEqual(result.status, AdmissionStatus.FALLBACK)
        self.assertEqual(result.fallback_reason, "redis_unavailable")

    @staticmethod
    def cache_kwargs():
        return dict(
            locale="zh-CN", skill_version="skill.v1", prompt_version="prompt.v1",
            mapping_hash="mapping", model="model",
        )


if __name__ == "__main__":
    unittest.main()


class FakeRedis:
    def __init__(self):
        self.values = {}
        self.lock = threading.Lock()

    def get(self, key):
        with self.lock:
            return self.values.get(key)

    def set(self, key, value, *, nx=False, ex=None):
        del ex
        with self.lock:
            if nx and key in self.values:
                return False
            self.values[key] = value
            return True


class FailingRedis:
    def get(self, key):
        del key
        raise ConnectionError("Connection Refused")

    def set(self, key, value, *, nx=False, ex=None):
        del key, value, nx, ex
        raise ConnectionError("Connection Refused")
