#!/usr/bin/env python3
"""Run a read-only, harness-neutral Phase 0B compatibility preflight."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


PROTOCOL_VERSION = "agent-harness-v0"
REQUIRED_READONLY_PERMISSIONS = {"edit": "deny", "task": "deny"}
ALLOWED_READONLY_BASH_PERMISSIONS = {"allow", "deny"}


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object: {path}")
    return value


def parse_agent_frontmatter(path: Path) -> dict[str, str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0].strip() != "---":
        raise ValueError(f"missing frontmatter: {path}")

    values: dict[str, str] = {}
    in_permissions = False
    for line in lines[1:]:
        if line.strip() == "---":
            break
        if not line.strip():
            continue
        if line.startswith("  ") and in_permissions:
            key, separator, value = line.strip().partition(":")
            if separator:
                values[f"permission.{key}"] = value.strip()
            continue
        key, separator, value = line.partition(":")
        if not separator:
            continue
        key = key.strip()
        values[key] = value.strip()
        in_permissions = key == "permission"
    else:
        raise ValueError(f"unterminated frontmatter: {path}")
    return values


def discover_profiles(root: Path) -> dict[str, dict[str, str]]:
    profiles: dict[str, dict[str, str]] = {}
    for path in sorted((root / ".opencode" / "agents").glob("*.md")):
        profiles[path.stem] = parse_agent_frontmatter(path)
    return profiles


def preflight(root: Path) -> dict[str, Any]:
    schema_path = root / "contracts" / "agent-harness-v0.schema.json"
    matrix_path = root / "contracts" / "agent-harness-capability-matrix-v0.json"
    schema = load_json(schema_path)
    matrix = load_json(matrix_path)
    errors: list[str] = []
    warnings: list[str] = []

    if schema.get("$id") is None:
        errors.append("protocol schema is missing $id")
    if schema.get("$defs", {}).get("taskRecord") is None:
        errors.append("protocol schema is missing taskRecord definition")
    if matrix.get("protocol_version") != PROTOCOL_VERSION:
        errors.append("capability matrix protocol_version does not match protocol schema")

    policy = matrix.get("default_policy", {})
    for key, expected in {
        "writer_count": 1,
        "benchmark_concurrency": 1,
        "private_inputs_allowed": False,
        "remote_provider_private_inputs": "blocked",
    }.items():
        if policy.get(key) != expected:
            errors.append(f"default_policy.{key} must be {expected!r}")

    profiles = discover_profiles(root)
    configured_profiles = {
        profile
        for runtime in matrix.get("runtimes", [])
        if runtime.get("id") == "opencode-native"
        for profile in runtime.get("actual_profiles", [])
    }
    for profile in sorted(configured_profiles):
        if profile not in profiles:
            errors.append(f"configured OpenCode profile is missing: {profile}")
            continue
        frontmatter = profiles[profile]
        if frontmatter.get("mode") != "subagent":
            errors.append(f"Phase 0B profile is not a subagent: {profile}")
        for permission, expected in REQUIRED_READONLY_PERMISSIONS.items():
            actual = frontmatter.get(f"permission.{permission}")
            if actual != expected:
                errors.append(
                    f"Phase 0B profile {profile} permission.{permission}="
                    f"{actual!r}, expected {expected!r}"
                )
        bash_permission = frontmatter.get("permission.bash")
        if bash_permission not in ALLOWED_READONLY_BASH_PERMISSIONS:
            errors.append(
                f"Phase 0B profile {profile} permission.bash={bash_permission!r}, "
                "expected 'allow' or 'deny'"
            )

    for runtime in matrix.get("runtimes", []):
        runtime_id = runtime.get("id", "unknown")
        status = runtime.get("status")
        if status == "unverified":
            warnings.append(f"runtime capability remains unverified: {runtime_id}")
        if status == "available" and runtime_id != "opencode-native":
            errors.append(f"external runtime cannot be marked available in Phase 0B: {runtime_id}")
        for capability, value in runtime.get("capabilities", {}).items():
            if value == "available" and status == "unverified":
                errors.append(f"unverified runtime claims available capability: {runtime_id}.{capability}")

    return {
        "protocol_version": PROTOCOL_VERSION,
        "phase": "0B",
        "status": "blocked" if errors else "preflight_passed",
        "scope": {
            "writes": False,
            "private_inputs": False,
            "business_commands": False,
            "runtime_start": False,
        },
        "profiles_checked": sorted(configured_profiles),
        "errors": errors,
        "warnings": warnings,
        "checks": {
            "protocol_schema": not any("protocol schema" in error for error in errors),
            "capability_matrix": not any("capability matrix" in error for error in errors),
            "readonly_profiles": not any("profile" in error for error in errors),
            "fail_closed_policy": not any("default_policy" in error for error in errors),
            "unverified_external_runtimes": not any("external runtime" in error for error in errors),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    try:
        report = preflight(args.root.resolve())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"preflight error: {error}", file=sys.stderr)
        return 2

    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    return 0 if report["status"] == "preflight_passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
