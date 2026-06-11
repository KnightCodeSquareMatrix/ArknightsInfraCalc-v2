#!/usr/bin/env python3
"""调用 infra-cli verify（需已 cargo build）。"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", default="reg_gsl_closure_tier90")
    parser.add_argument("--all", action="store_true")
    args = parser.parse_args()

    cmd = ["cargo", "run", "-p", "infra-cli", "--"]
    if args.all:
        cmd.append("verify")
        cmd.append("--all")
    else:
        cmd.extend(["verify", "--case", args.case])

    return subprocess.call(cmd, cwd=ROOT)


if __name__ == "__main__":
    sys.exit(main())
