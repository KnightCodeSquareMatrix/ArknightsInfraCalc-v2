#!/usr/bin/env python3
"""Probe local runtime adapter prerequisites without launching a runtime."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


def command_version(
    command: str,
    version_args: tuple[str, ...] = ("--version",),
    explicit_path: str | None = None,
) -> str | None:
    executable = explicit_path or shutil.which(command)
    if executable is None:
        return None
    result = subprocess.run(
        [executable, *version_args],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    output = (result.stdout or result.stderr).strip().splitlines()
    return output[0] if result.returncode == 0 and output else f"exit:{result.returncode}"


def node_module_available(root: Path, module: str) -> bool:
    node = shutil.which("node")
    if node is None:
        return False
    result = subprocess.run(
        [node, "-e", f"import({module!r}).then(() => process.exit(0)).catch(() => process.exit(1))"],
        cwd=root / ".opencode",
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    return result.returncode == 0


def probe(root: Path) -> dict[str, Any]:
    opencode_path = os.environ.get("HARNESS_OPENCODE_BIN")
    mimo_path = os.environ.get("HARNESS_MIMO_BIN")
    opencode_version = command_version("opencode", explicit_path=opencode_path)
    mimo_version = command_version("mimo", explicit_path=mimo_path)
    claude_version = command_version("claude")
    opencode_sdk = node_module_available(root, "@opencode-ai/sdk")
    claude_sdk = node_module_available(root, "@anthropic-ai/claude-agent-sdk")
    openai_sdk = node_module_available(root, "openai")

    runtimes = [
        {
            "id": "opencode-native",
            "status": "available" if opencode_version else "not_on_probe_path",
            "launch": False,
            "facts": {
                "cli_version": opencode_version,
                "explicit_path": bool(opencode_path),
                "project_sdk_importable": opencode_sdk,
                "project_config": (root / "opencode.json").is_file(),
            },
            "unknowns": [
                "Task parallelism, child session resume and cancellation were not exercised.",
                "A missing PATH entry is not evidence that the user's interactive shell lacks the CLI.",
            ],
        },
        {
            "id": "mimo-code",
            "status": "not_on_probe_path" if not mimo_version else "unverified",
            "launch": False,
            "facts": {"cli_version": mimo_version},
            "unknowns": [
                "MiMo project/workflow compatibility was not exercised.",
                "A missing PATH entry is not evidence that the user's interactive shell lacks the CLI.",
            ],
        },
        {
            "id": "claude-agent-sdk",
            "status": "partial" if claude_version and not claude_sdk else "unverified",
            "launch": False,
            "facts": {
                "cli_version": claude_version,
                "project_sdk_importable": claude_sdk,
            },
            "unknowns": ["Claude Agent SDK adapter and worktree/session semantics were not exercised."],
        },
        {
            "id": "openai-responses-multi-agent",
            "status": "unverified",
            "launch": False,
            "facts": {"project_sdk_importable": openai_sdk},
            "unknowns": [
                "Responses Multi-agent beta and provider gateway compatibility were not exercised.",
                "No network request or private input was used by this probe.",
            ],
        },
    ]
    return {
        "protocol_version": "agent-harness-v0",
        "task": {
            "id": "runtime-adapter-probe",
            "protocol_version": "agent-harness-v0",
            "project_profile": "arknights-infra-calc-solver",
            "state": "succeeded",
            "write_policy": "readonly",
            "data_classification": "public",
            "dirty_worktree": False,
            "children": [],
            "proof_obligations": [],
            "adapter_probe": {
                "phase": "0C",
                "probe_scope": {
                    "launch_runtime": False,
                    "network": False,
                    "private_inputs": False,
                    "writes": False,
                },
                "runtimes": runtimes,
            },
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    report = probe(args.root.resolve())
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
