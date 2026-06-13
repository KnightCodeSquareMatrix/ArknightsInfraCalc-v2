#!/usr/bin/env python3
"""Fetch PRTS manufacturing station skills table and persist to data/."""

from __future__ import annotations

import csv
import json
import re
from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import Request, urlopen

from lxml import html

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"

SOURCE_PAGE = (
    "https://prts.wiki/w/%E7%BD%97%E5%BE%B7%E5%B2%9B%E5%9F%BA%E5%BB%BA/%E5%88%B6%E9%80%A0%E7%AB%99"
)
SOURCE_PAGE_LABEL = "https://prts.wiki/w/罗德岛基建/制造站"
SOURCE_XPATH = "/html/body/div[3]/div[3]/div[5]/div[1]/table[2]/tbody"
SOURCE_SECTION = "拥有制造站技能的干员"
COLUMNS = ["skill_name", "description", "operators"]


def fetch_page(url: str) -> html.HtmlElement:
    req = Request(url, headers={"User-Agent": "Mozilla/5.0"})
    raw = urlopen(req, timeout=60).read()
    return html.fromstring(raw)


def cell_text(cell: html.HtmlElement) -> str:
    node = deepcopy(cell)
    for hidden in node.xpath('.//*[@style and contains(@style, "display:none")]'):
        parent = hidden.getparent()
        if parent is not None:
            parent.remove(hidden)
    text = node.text_content()
    return re.sub(r"\s+", " ", text).strip()


def operator_names(cell: html.HtmlElement) -> list[str]:
    names: list[str] = []
    seen: set[str] = set()
    for anchor in cell.xpath('.//a[@title]'):
        name = anchor.get("title", "").strip()
        if name and name not in seen:
            seen.add(name)
            names.append(name)
    return names


def parse_table(tbody: html.HtmlElement) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    for tr in tbody.xpath("./tr"):
        cells = tr.xpath("./td")
        if len(cells) < 4:
            continue

        skill_name = cell_text(cells[1])
        if not skill_name:
            continue

        description = cell_text(cells[2])
        operators = operator_names(cells[3])
        rows.append(
            {
                "skill_name": skill_name,
                "description": description,
                "operators": "；".join(operators),
            }
        )
    return rows


def main() -> None:
    page = fetch_page(SOURCE_PAGE)
    matches = page.xpath(SOURCE_XPATH)
    if not matches:
        raise RuntimeError(f"table not found at xpath: {SOURCE_XPATH}")

    tbody = matches[0]
    table = tbody.getparent()
    if table is None:
        raise RuntimeError("table tbody has no parent table")

    rows = parse_table(tbody)
    fetched_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )

    payload = {
        "source_page": SOURCE_PAGE_LABEL,
        "source_xpath": SOURCE_XPATH,
        "source_section": SOURCE_SECTION,
        "fetched_at": fetched_at,
        "row_count": len(rows),
        "columns": COLUMNS,
        "rows": rows,
    }

    json_path = DATA / "prts_manufacturing_skills.json"
    csv_path = DATA / "prts_manufacturing_skills.csv"
    html_path = DATA / "prts_manufacturing_skills_table.html"

    json_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    with csv_path.open("w", encoding="utf-8-sig", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=COLUMNS)
        writer.writeheader()
        writer.writerows(rows)

    html_path.write_text(
        html.tostring(table, encoding="unicode"),
        encoding="utf-8",
    )

    print(f"wrote {json_path} ({len(rows)} rows)")
    print(f"wrote {csv_path}")
    print(f"wrote {html_path} ({html_path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
