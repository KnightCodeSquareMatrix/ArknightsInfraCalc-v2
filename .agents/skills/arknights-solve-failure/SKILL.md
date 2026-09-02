---
name: arknights-solve-failure
description: 同步、管理并检索无用户反馈的求解失败证据族（evidence/ 目录与 evidence/TRACKING.md 台账），支持按 CLI 指纹（solver_executable_sha256）检索本地 evidence/ 与 feedback/ 的历史案例。用户要求"查看求解失败""同步失败案例""失败证据""按指纹查""哪个构建产生的失败"时使用。与 arknights-feedback-sync 互补：feedback-sync 管用户反馈，本 skill 管 solver 自身失败族。
---

# 求解失败证据同步与检索

## 本地形态（与 feedback/ 同构）

| 反馈侧（用户反馈） | 失败侧（无反馈的 solve 失败） |
|---|---|
| `feedback/<日期>/<HHMMSS>-<备注>/` | `evidence/<日期>-<错误族>/raw-runs/<运行id>/` |
| `feedback/TRACKING.md` | `evidence/TRACKING.md` |
| `scripts/sync_feedback.py` | `scripts/sync_solve_failures.py` |
| `.agents/skills/arknights-feedback-sync/` | `.agents/skills/arknights-solve-failure/`（本文件） |

错误族目录内含：`raw-runs/`（生产原样运行：serve-request.jsonl / serve-response.json / layout / operbox / debug-bundle.json / stdout / stderr）、`manifest.tsv`（运行 / rotation / top / operbox / layout / ok / 错误原文）、`replay.sh`（本地回放验收）、`README.md`。`debug-bundle.json` 的 `solver_executable_sha256` 记录产生该运行的生产 CLI 指纹。

## 服务器事实（真源：systemd 运行时）

- 生产服务器：`root@114.66.55.78`，SSH 密钥 `~/.ssh/id_rsa`。
- 失败运行源目录（按服务）：
  - beta 前端 `arknights-infra`：`/var/lib/arknights-infra/cli-runs`
  - dev 前端 `arknights-infra-dev`：`/var/lib/arknights-infra-dev/cli-runs`
  - quality：无 cli-runs，自动跳过
- 旧服务器 61.184.13.127 / 110.42.36.46 已废弃，不要再用。

## 同步

```bash
python3 scripts/sync_solve_failures.py
```

行为：
- 扫描各服务 `cli-runs/` 的失败运行（`serve-response.json` 中 `ok=false` 且有错误文本、`serve-request.jsonl` 存在），按错误族 + 北京日期归入 `evidence/<日期>-<错误族>/raw-runs/`，按运行 UUID 去重，只下载新增。
- 刷新族目录 `manifest.tsv`；新建族时自动生成 `README.md` 与 `replay.sh`。
- 重写 `evidence/TRACKING.md` 的自动索引块（族 / 运行数 / 首错摘要 / CLI 指纹 8hex），人工维护的「族清单」保持不动。
- `sync_feedback.py` 的失败捕获阶段与本脚本共用同一逻辑；跑哪个入口都能保持 evidence/ 最新。

参数：`--target`、`--remote-dir <path>`（可重复）、`--evidence-dir <path>`、`--failed-run-hours N`（默认 72）、`--no-sync`。

## 按 CLI 指纹检索

指纹 = 运行 `debug-bundle.json` 顶层 `solver_executable_sha256`（产生该运行的生产 CLI）。只查本地已抓材料（evidence/ + feedback/ 全部 debug-bundle）：

```bash
# 直接给指纹前缀（至少 8 位十六进制）
python3 scripts/sync_solve_failures.py --no-sync --cli-fingerprint 9dd38385
python3 scripts/sync_solve_failures.py --no-sync --cli-fingerprint 509f8ac8924fca7659c85a3773cab5169032f03886156077cf9262d708b27fe1

# 按本地某个构建文件算 SHA 再查（判断"这个构建对应哪些失败案例"）
python3 scripts/sync_solve_failures.py --no-sync --match-build target/release/infra-cli
```

输出按族/案例分组列出命中运行、指纹 8hex 与错误摘要，并报告 `matched=N unknown-fingerprint-bundles=M`（M 是本地无法提取指纹的 bundle 数）。指纹检索本身不触发远程访问。

## 台账处置

1. 新族出现时在 `evidence/TRACKING.md`「族清单」加一行标 `intake`；先本地回放 `evidence/<族>/replay.sh` 确认复现。
2. 状态与反馈台账同词表：`intake` / `reproduced` / `localized` / `fixing` / `closed` / `regressed` / `blocked`。
3. 定位到最小层（CLI / 排班 / 候选 / 机制 / 数据）后把复现命令与结论写进行；指向 solver bug 时转 `arknights-maintenance`，大规模求解/排班改动按 `AGENTS.md` 需要 `arknights-invariant-gate`。
4. `evidence/` 含真实练度盒与运行日志：保持 untracked 不提交公共仓库；`raw-runs` 材料原始证据不改写。`evidence/TRACKING.md` 本身可提交（与 `feedback/TRACKING.md` 同级）。
5. 同步只读处理：不编辑 raw 文件；triage 中遇到的异常输入族记录到台账，不直接改代码。
