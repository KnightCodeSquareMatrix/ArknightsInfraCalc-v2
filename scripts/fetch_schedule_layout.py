"""Fetch yituliu scheduleV2 layout UI fragment via requests + BeautifulSoup."""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

import requests
from bs4 import BeautifulSoup, Tag

URL = "https://ark.yituliu.cn/tools/scheduleV2"
# /html/body/div[1]/div/div/div[2]/div/div/main/div[1]/div/div/div[3]/div[3]
XPATH_INDICES = [1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 3, 3]
OUT_DIR = Path(__file__).resolve().parent.parent / "out" / "layout_fetch"
SCHEDULE_V2_CHUNK = "schedule.v2-Cc3GIZrm.js"
SCHEDULE_CSS = "schedule-DCmH-EbA.css"


def element_children(el: Tag) -> list[Tag]:
    return [c for c in el.children if isinstance(c, Tag)]


def follow_path(root: Tag, indices: list[int]) -> Tag | None:
    el: Tag | None = root
    for idx in indices:
        if el is None:
            return None
        kids = element_children(el)
        if idx < 1 or idx > len(kids):
            print(
                f"child {idx} not found at <{el.name}>, have {len(kids)}",
                file=sys.stderr,
            )
            for i, k in enumerate(kids, 1):
                print(
                    f"  {i}: <{k.name}> class={k.get('class')} id={k.get('id')}",
                    file=sys.stderr,
                )
            return None
        el = kids[idx - 1]
    return el


def summarize_structure(el: Tag, depth: int = 0, max_depth: int = 4) -> list[dict]:
    rows: list[dict] = []
    cls = " ".join(el.get("class") or [])
    text = el.get_text(strip=True)[:80]
    rows.append(
        {
            "depth": depth,
            "tag": el.name,
            "class": cls,
            "id": el.get("id"),
            "text": text,
        }
    )
    if depth >= max_depth:
        return rows
    for child in element_children(el):
        rows.extend(summarize_structure(child, depth + 1, max_depth))
    return rows


def extract_scripts(html: str) -> list[str]:
    return re.findall(r"<script[^>]*>([\s\S]*?)</script>", html, flags=re.I)


def main() -> int:
    for key in (
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ):
        os.environ.pop(key, None)

    headers = {
        "User-Agent": (
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
            "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        )
    }
    resp = requests.get(URL, headers=headers, timeout=30, proxies={"http": None, "https": None})
    resp.raise_for_status()
    print(f"status={resp.status_code} len={len(resp.text)}")

    soup = BeautifulSoup(resp.text, "html.parser")
    body = soup.body
    if body is None:
        print("no <body>", file=sys.stderr)
        return 1

    target = follow_path(body, XPATH_INDICES)
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    if target is None:
        (OUT_DIR / "body_snippet.html").write_text(
            body.prettify()[:20000], encoding="utf-8"
        )
        print(
            "xpath target not in static HTML (Vue SPA shell); "
            f"wrote {OUT_DIR / 'body_snippet.html'}",
            file=sys.stderr,
        )
    else:
        target_html = target.prettify()
        (OUT_DIR / "target.html").write_text(target_html, encoding="utf-8")
        structure = summarize_structure(target, max_depth=5)
        (OUT_DIR / "structure.json").write_text(
            json.dumps(structure, ensure_ascii=False, indent=2), encoding="utf-8"
        )
        print("=== TARGET ===")
        print("tag:", target.name)
        print("class:", target.get("class"))
        print("text:", target.get_text(separator=" | ", strip=True)[:1500])

    # scheduleV2 layout lives in lazy-loaded Vue chunk, not server HTML.
    base = "https://ark.yituliu.cn"
    for asset in (SCHEDULE_V2_CHUNK, SCHEDULE_CSS, "schedule_menu-cgBhe1WV.js"):
        r2 = requests.get(
            f"{base}/assets/{asset}",
            headers=headers,
            timeout=60,
            proxies={"http": None, "https": None},
        )
        r2.raise_for_status()
        (OUT_DIR / asset).write_bytes(r2.content)
        print(f"saved asset {asset} ({len(r2.content)} bytes)")

    chunk = (OUT_DIR / SCHEDULE_V2_CHUNK).read_text(encoding="utf-8", errors="replace")
    if "room-wrap" in chunk:
        print("schedule.v2 chunk contains room-wrap layout (target UI region)")

    print(f"artifacts -> {OUT_DIR}")
    print("generator -> tools/layout-gen/index.html")
    return 0 if (OUT_DIR / SCHEDULE_V2_CHUNK).exists() else 1


if __name__ == "__main__":
    raise SystemExit(main())
