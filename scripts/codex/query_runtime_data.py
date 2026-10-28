#!/usr/bin/env python3
"""Query raw runtime catalogs and exact references without interpreting buffs."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Any

if __package__:
    from ._query_output import QueryError, add_output_arguments, emit, pointer, read_json
else:
    from _query_output import QueryError, add_output_arguments, emit, pointer, read_json


CATALOGS = {
    "operator_instances.json": ("instances", dict),
    "skill_table.json": ("skills", list),
    "orchestration_rules.json": ("rules", list),
    "base_systems.json": ("systems", list),
    "producer_rules.json": ("rules", list),
    "trade_shortcuts.json": ("entries", list),
    "trade_segments.json": ("segments", list),
}


def load_catalogs(directory: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    records, sources = [], []
    for filename, (field, kind) in CATALOGS.items():
        path = directory / filename
        obj, source = read_json(path)
        if not isinstance(obj, dict) or type(obj.get(field)) is not kind:
            raise QueryError(f"{path}: /{field} must be {kind.__name__}")
        source["version"] = obj.get("version")
        sources.append(source)
        container = obj[field]
        items = container.items() if isinstance(container, dict) else enumerate(container)
        ids: set[str] = set()
        for key, record in items:
            rp = pointer("/" + field, key)
            if not isinstance(record, dict):
                raise QueryError(f"{path}:{rp} must be an object")
            identifier = key if isinstance(container, dict) else record.get("id")
            if not isinstance(identifier, str) or identifier in ids:
                raise QueryError(f"{path}:{rp} missing/duplicate string record id")
            ids.add(identifier)
            if filename == "operator_instances.json":
                if not isinstance(record.get("name"), str) or not isinstance(record.get("tier"), str):
                    raise QueryError(f"{path}:{rp} requires name and tier strings")
                if not isinstance(record.get("facilities"), dict):
                    raise QueryError(f"{path}:{rp}/facilities must be an object")
                for facility, binding in record["facilities"].items():
                    if (not isinstance(binding, dict) or not isinstance(binding.get("buff_ids"), list)
                            or any(not isinstance(x, str) for x in binding["buff_ids"])):
                        raise QueryError(f"{path}:{rp}/facilities/{facility} requires buff_ids strings")
            records.append({"catalog": filename, "id": identifier, "path": source["path"],
                            "json_pointer": rp, "record": record,
                            "catalog_source": obj.get("source")})
        # roles and injection registries also contain relevant explicit references.
        for field in ("roles", "control_manu_injectors"):
            if field in obj:
                if not isinstance(obj[field], (dict, list)):
                    raise QueryError(f"{path}: /{field} must be an object or array")
                records.append({"catalog": filename, "id": field, "path": source["path"],
                                "json_pointer": "/" + field, "record": obj[field],
                                "catalog_source": obj.get("source")})
    return records, sources


def exact_references(value: Any, target: str, path: str = "") -> list[str]:
    """Locate exact string values/keys; do not infer references from labels."""
    found = []
    if isinstance(value, dict):
        for key, child in value.items():
            at = pointer(path, key)
            if key == target:
                found.append(at)
            found.extend(exact_references(child, target, at))
    elif isinstance(value, list):
        for i, child in enumerate(value):
            found.extend(exact_references(child, target, pointer(path, i)))
    elif isinstance(value, str) and value == target:
        found.append(path or "/")
    return found


def query(records: list[dict[str, Any]], kind: str, target: str) -> tuple[list[dict[str, Any]], list[str]]:
    targets = {"buff": "skill_table.json", "rule": "orchestration_rules.json",
               "system": "base_systems.json", "shortcut": "trade_shortcuts.json"}
    selected, buffs = [], set()
    for record in records:
        value = record["record"]
        primary = (record["catalog"] == "operator_instances.json" and
                   isinstance(value, dict) and value.get("name") == target) if kind == "operator" else (
                   record["catalog"] == targets[kind] and record["id"] == target)
        refs = exact_references(value, target, record["json_pointer"])
        if primary or refs:
            selected.append({**record, "match": "primary" if primary else "exact_reference",
                             "matched_paths": refs})
            if kind == "operator" and primary:
                for binding in value["facilities"].values():
                    buffs.update(binding["buff_ids"])
    missing = []
    if kind == "operator":
        skills = {x["id"]: x for x in records if x["catalog"] == "skill_table.json"}
        for buff in sorted(buffs):
            if buff not in skills:
                missing.append(buff)
            else:
                selected.append({**skills[buff], "match": "raw_binding_buff",
                                 "matched_paths": [skills[buff]["json_pointer"] + "/id"]})
    return selected, missing


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", type=Path, default=Path(__file__).resolve().parents[2] / "data")
    choice = parser.add_mutually_exclusive_group(required=True)
    for option in ("operator", "buff-id", "rule", "system", "shortcut"):
        choice.add_argument("--" + option)
    add_output_arguments(parser)
    args = parser.parse_args()
    kind, target = next((kind, value) for kind, value in
                        [("operator", args.operator), ("buff", args.buff_id), ("rule", args.rule),
                         ("system", args.system), ("shortcut", args.shortcut)] if value is not None)
    try:
        records, sources = load_catalogs(args.data_dir)
        matches, missing = query(records, kind, target)
        payload = {"schema": "runtime-query-v1", "status": "ok" if matches else "no_match",
                   "selection": {"kind": kind, "value": target, "data_dir": str(args.data_dir.resolve())},
                   "sources": sources, "match_count": len(matches), "matches": matches,
                   "buff_ids_not_in_skill_table": missing,
                   "scope": "raw bindings and exact string references; no tier/stepwise merge or mechanic judgment"}
        emit(payload, args)
        return int(not matches)
    except (QueryError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
