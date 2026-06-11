#!/usr/bin/env python3
"""Insert tags/layout fields into TradeOperator / TradeRoomInput literals."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FILES = list((ROOT / "crates/infra-core/src").rglob("*.rs"))


def patch_block(kind: str, text: str, missing_field: str, field_line: str) -> str:
    pattern = re.compile(rf"{kind} \{{", re.MULTILINE)
    out = []
    last = 0
    for m in pattern.finditer(text):
        out.append(text[last : m.start()])
        start = m.end() - 1
        depth = 0
        i = start
        while i < len(text):
            ch = text[i]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    block = text[m.start() : i + 1]
                    if missing_field not in block:
                        inner = block[:-1].rstrip()
                        block = inner + f"\n            {field_line},\n        }}"
                    out.append(block)
                    last = i + 1
                    break
            i += 1
        else:
            out.append(text[m.start() :])
            last = len(text)
            break
    out.append(text[last:])
    return "".join(out)


def main() -> None:
    for path in FILES:
        text = path.read_text(encoding="utf-8")
        orig = text
        text = patch_block("TradeOperator", text, "tags:", "tags: vec![]")
        text = patch_block("TradeRoomInput", text, "layout:", "layout: Default::default()")
        if text != orig:
            path.write_text(text, encoding="utf-8")
            print("fixed", path)


if __name__ == "__main__":
    main()
