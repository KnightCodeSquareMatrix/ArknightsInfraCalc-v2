"""Versioned, reloadable canonical-name mapping."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from threading import RLock
from typing import Any


class MappingError(ValueError):
    pass


@dataclass(frozen=True)
class CanonicalMapping:
    version: str
    locale: str
    operators: dict[str, dict[str, Any]]
    combinations: dict[str, dict[str, Any]]
    operator_ids_by_name: dict[str, str]
    content_hash: str

    @classmethod
    def load(cls, path: str | Path) -> "CanonicalMapping":
        source = Path(path).read_bytes()
        try:
            data = json.loads(source)
        except json.JSONDecodeError as exc:
            raise MappingError(f"invalid mapping JSON: {exc}") from exc
        if not isinstance(data, dict):
            raise MappingError("mapping must be an object")
        version = data.get("mapping_version")
        locale = data.get("locale")
        if not isinstance(version, str) or not version.strip():
            raise MappingError("mapping_version must be a non-empty string")
        _version_key(version)
        if locale != "zh-CN":
            raise MappingError("only zh-CN mapping is supported")
        operators = _validate_entries(data.get("operators"), "operators")
        combinations = _validate_entries(data.get("combinations"), "combinations")
        _validate_aliases(operators, "operators")
        _validate_aliases(combinations, "combinations")
        return cls(
            version=version,
            locale=locale,
            operators=operators,
            combinations=combinations,
            operator_ids_by_name={entry["name"]: identifier for identifier, entry in operators.items()},
            content_hash=hashlib.sha256(source).hexdigest(),
        )

    def combination_name(self, identifier: str) -> str:
        try:
            return self.combinations[identifier]["name"]
        except KeyError as exc:
            raise MappingError(f"unknown combination mapping: {identifier}") from exc

    def operator_name(self, identifier: str) -> str:
        try:
            return self.operators[identifier]["name"]
        except KeyError as exc:
            raise MappingError(f"unknown operator mapping: {identifier}") from exc


class MappingStore:
    """Keeps the last valid immutable mapping snapshot across reload failures."""

    def __init__(self, path: str | Path):
        self.path = Path(path)
        self._lock = RLock()
        self._current: CanonicalMapping | None = None

    def load(self) -> CanonicalMapping:
        with self._lock:
            candidate = CanonicalMapping.load(self.path)
            if self._current and _version_key(candidate.version) <= _version_key(self._current.version):
                raise MappingError(
                    f"mapping version must increase: {candidate.version} <= {self._current.version}"
                )
            self._current = candidate
            return candidate

    def reload(self) -> CanonicalMapping:
        with self._lock:
            previous = self._current
            try:
                candidate = CanonicalMapping.load(self.path)
                if previous and _version_key(candidate.version) <= _version_key(previous.version):
                    raise MappingError(
                        f"mapping version must increase: {candidate.version} <= {previous.version}"
                    )
            except (OSError, MappingError):
                if previous is not None:
                    return previous
                raise
            self._current = candidate
            return candidate

    @property
    def current(self) -> CanonicalMapping | None:
        with self._lock:
            return self._current


def _validate_entries(value: Any, field: str) -> dict[str, dict[str, Any]]:
    if not isinstance(value, dict):
        raise MappingError(f"{field} must be an object")
    result: dict[str, dict[str, Any]] = {}
    for identifier, entry in value.items():
        if not isinstance(identifier, str) or not identifier.strip():
            raise MappingError(f"{field} contains an empty identifier")
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            raise MappingError(f"{field}.{identifier} must contain a name")
        aliases = entry.get("aliases", [])
        if not isinstance(aliases, list) or not all(isinstance(alias, str) for alias in aliases):
            raise MappingError(f"{field}.{identifier}.aliases must be strings")
        result[identifier] = {"name": entry["name"], "aliases": aliases}
    return result


def _validate_aliases(entries: dict[str, dict[str, Any]], field: str) -> None:
    seen: dict[str, str] = {}
    for identifier, entry in entries.items():
        for value in [entry["name"], *entry["aliases"]]:
            if value in seen and seen[value] != identifier:
                raise MappingError(f"duplicate {field} alias: {value}")
            seen[value] = identifier


def _version_key(value: str) -> tuple[int, ...]:
    try:
        parts = tuple(int(part) for part in value.split("."))
    except ValueError as exc:
        raise MappingError(f"mapping_version must be numeric dotted version: {value}") from exc
    if not parts or any(part < 0 for part in parts):
        raise MappingError(f"mapping_version must be non-negative: {value}")
    return parts
