#!/usr/bin/env python3
import json
from pathlib import Path

import pandas as pd

root = Path(__file__).resolve().parents[1]
df = pd.read_excel(root / "干员练度表.xlsx", sheet_name=0)
owned = df[df["是否已招募"] == True]
print(f"owned={len(owned)} / {len(df)}")
names = ["但书", "巫恋", "龙舌兰", "可露希尔", "能天使", "德克萨斯", "卡夫卡", "柏喙", "古米", "银灰"]
for n in names:
    row = df[df["干员名称"] == n]
    if len(row):
        r = row.iloc[0]
        print(f"{n}: own={r['是否已招募']} elite={int(r['精英化等级'])} lv={int(r['等级'])}")
    else:
        print(f"{n}: NOT FOUND")
