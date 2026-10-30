#!/usr/bin/env python3
"""Compatibility entry: run the Rust final-assignment invariant gate.

Karlan admission is optional. The former Python registry-claim checks are
superseded by karlan_precision_selected_team and rotation_bind in Rust.
"""

from pathlib import Path
import runpy
import sys

if __name__ == "__main__":
    # The common runner always enables this gate; keep accepting the old flag.
    sys.argv = [arg for arg in sys.argv if arg != "--assert-invariants"]
    runpy.run_path(str(Path(__file__).with_name("assert_rotation_invariants.py")), run_name="__main__")
