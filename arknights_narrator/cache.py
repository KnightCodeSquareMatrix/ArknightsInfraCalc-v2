"""BFF-facing Redis key and admission primitives.

This module deliberately depends only on the small Redis operations that a BFF
already provides. It does not create a Redis client or own HTTP/task workers.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import secrets
import uuid
from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Protocol


NAMESPACE = "arknights:training_advice:narrative:v1"


class RedisUnavailable(Exception):
    """Raised by a BFF Redis adapter when Redis cannot be reached."""


class RedisLike(Protocol):
    def get(self, key: str) -> str | bytes | None:
        ...

    def set(self, key: str, value: str, *, nx: bool = False, ex: int | None = None) -> bool:
        ...


class AdmissionStatus(StrEnum):
    READY = "ready"
    PROCESSING = "processing"
    ACCEPTED = "accepted"
    FALLBACK = "fallback"


@dataclass(frozen=True)
class Admission:
    status: AdmissionStatus
    content_hmac: str
    task_id: str | None = None
    lock_token: str | None = None
    retry_after_ms: int | None = None
    cached_value: str | None = None
    fallback_reason: str | None = None


class RedisKeys:
    """Separate key families prevent task state and content cache collisions."""

    @staticmethod
    def ready(content_hmac: str) -> str:
        return f"{NAMESPACE}:ready:{content_hmac}"

    @staticmethod
    def fallback(content_hmac: str) -> str:
        return f"{NAMESPACE}:fallback:{content_hmac}"

    @staticmethod
    def lock(content_hmac: str) -> str:
        return f"{NAMESPACE}:lock:{content_hmac}"

    @staticmethod
    def task(task_id: str) -> str:
        return f"{NAMESPACE}:task:{task_id}"


def content_hmac(
    report: dict[str, Any],
    secret: str,
    *,
    locale: str,
    skill_version: str,
    prompt_version: str,
    mapping_hash: str,
    model: str,
) -> str:
    """Create a server-owned digest; callers cannot choose the cache identity."""

    canonical = json.dumps(
        {
            "report": report,
            "locale": locale,
            "skill_version": skill_version,
            "prompt_version": prompt_version,
            "mapping_hash": mapping_hash,
            "model": model,
        },
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hmac.new(secret.encode("utf-8"), canonical, hashlib.sha256).hexdigest()


def admit(
    redis: RedisLike,
    report: dict[str, Any],
    principal: str,
    secret: str,
    *,
    locale: str,
    skill_version: str,
    prompt_version: str,
    mapping_hash: str,
    model: str,
    lock_ttl_seconds: int = 30,
    retry_after_ms: int = 500,
) -> Admission:
    """Check ready first, then merge duplicate work with a fencing lock.

    The BFF must execute lock acquisition, task-state creation and queue enqueue
    in one same-slot Lua transaction. This pure primitive exposes the decision
    and token; it does not pretend to implement the transaction itself.
    """

    if not principal:
        raise ValueError("principal is required")
    digest = content_hmac(
        report,
        secret,
        locale=locale,
        skill_version=skill_version,
        prompt_version=prompt_version,
        mapping_hash=mapping_hash,
        model=model,
    )
    try:
        ready = redis.get(RedisKeys.ready(digest))
        if ready is not None:
            return Admission(AdmissionStatus.READY, digest, cached_value=_decode(ready))
        token = f"{uuid.uuid4()}:{secrets.token_urlsafe(18)}"
        acquired = redis.set(RedisKeys.lock(digest), token, nx=True, ex=lock_ttl_seconds)
    except (RedisUnavailable, ConnectionError, TimeoutError, OSError):
        return Admission(AdmissionStatus.FALLBACK, digest, fallback_reason="redis_unavailable")
    if acquired is not True:
        return Admission(AdmissionStatus.PROCESSING, digest, retry_after_ms=retry_after_ms)
    return Admission(
        AdmissionStatus.ACCEPTED,
        digest,
        task_id=str(uuid.uuid4()),
        lock_token=token,
    )


def _decode(value: str | bytes) -> str:
    return value.decode("utf-8") if isinstance(value, bytes) else value


RELEASE_LOCK_LUA = """
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('DEL', KEYS[1])
end
return 0
""".strip()


PUBLISH_RESULT_LUA = """
if redis.call('GET', KEYS[1]) ~= ARGV[1] then
  return 0
end
redis.call('SET', KEYS[2], ARGV[2], 'EX', ARGV[3])
redis.call('DEL', KEYS[1])
return 1
""".strip()
