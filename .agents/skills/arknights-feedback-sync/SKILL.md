---
name: arknights-feedback-sync
description: 从生产服务器 114.66.55.78 下载明日方舟基建测试反馈（feedback bundle）到本地并按日期/备注整理，同时导入对应 cli-runs 复现包、捕获无用户反馈的求解失败运行证据。用户要求"下载反馈""同步 feedback""查看服务器反馈""拉取最新反馈"时使用。
---

# Arknights 反馈下载与整理

## 服务器事实（真源：systemd 运行时）

- 生产服务器：`root@114.66.55.78`，SSH 密钥 `~/.ssh/id_rsa`（默认提供，无需 IdentityFile）。
- 反馈目录（按 systemd 服务分离）：
  - beta 前端 `arknights-infra`：`/var/lib/arknights-infra/feedback`
  - dev 前端 `arknights-infra-dev`：`/var/lib/arknights-infra-dev/feedback`
  - quality 前端 `arknights-infra-quality`：`/var/lib/arknights-infra-quality/feedback`（可能不存在，脚本会跳过）
- 旧服务器 61.184.13.127 / 110.42.36.46 的 feedback 目录已废弃，不要再用。

## 快速下载

```bash
python3 scripts/sync_feedback.py
```

一条命令完成三阶段同步（缺省全部开启）：

1. **反馈导入**：rsync 拉取三个服务的 feedback 目录到临时区，按 `feedbackId` 去重，把**新**反馈写入本地 `feedback/<YYYY-MM-DD北京>/<HHMMSS北京>-<备注slug>/`；命名使用北京时区（CST, UTC+8），时间优先取 `meta.json` 的 `savedAt`。
2. **复现包导入**：按每个反馈 `meta.json` 的 `diagnosticId` 到对应服务 `cli-runs/` 目录查找匹配运行目录，把缺失的复现材料（`debug-bundle.json`、`operbox.json`、`layout.json`、`command.txt`、`serve-request.jsonl`、`result.json` 等）补进本地案例目录；找不到或材料不全时打印提示，不阻塞其他同步。
3. **求解失败捕获**：扫描每个服务 `cli-runs/` 中无用户反馈的失败运行（`serve-response.json` 中 `ok=false` 且有错误文本），按错误族归类并去重（运行 UUID）后整目录拷入 `evidence/<YYYY-MM-DD北京>-<错误族slug>/raw-runs/`；同族目录只建一次，后续同步增量追加并刷新 `manifest.tsv`（每行：运行目录 / rotation / top / operbox 前缀 / layout 前缀 / ok / 错误原文）。新建目录时自动生成 `README.md` 与 `replay.sh`（回放 `raw-runs/*/serve-request.jsonl`，断言 `ok=false`）。

脚本结束输出摘要：`New feedback folders synced`、`Reproduction bundles imported`、`Failed solve runs captured into evidence`、`Recent feedback files`。捕获内容含真实练度盒，只放本地 `evidence/`（untracked），不提交公共仓库。

自定义参数：`--target root@1.2.3.4`、`--remote-dir <path>`（可重复）、`--local-dir <path>`（默认仓库根 `feedback/`）、`--evidence-dir <path>`（默认仓库根 `evidence/`）、`--no-failed-runs`（跳过阶段 3）、`--failed-run-hours N`（只捕获 N 小时内的失败，默认 72）。

## 手工核对（未跑脚本时的只读快速查询）

```bash
ssh root@114.66.55.78 'ls -lt /var/lib/arknights-infra-dev/feedback/ | head'
```

每个目录内含 `issue.json`（用户反馈正文）、`meta.json`（feedbackId/savedAt/diagnosticId/contract hash）、可选 `debug-bundle.json` 与 `operbox.json`。

## 同步后

1. 只读处理 bundle：`issue.json`、`meta.json`、`debug-bundle.json`、`operbox.json` 是原始证据，triage 期间不编辑；调查结论写 `feedback/TRACKING.md`。
2. 新反馈行记入 `feedback/TRACKING.md`（状态：intake / reproduced / localized / fixing / regressed / closed / blocked / duplicate / duplicate-covered）。
3. 本地重放命令见 `feedback/README.md`（`infra-cli debug rerun --bundle ...`）。
4. 若反馈指向 solver/机制 bug，按根 `AGENTS.md` 当作普通修复处理；契约 hash 与前端呈现问题转前端。

## 默认丢弃规则（用户裁决 2026-08-31，原话保留）

> "反馈'中枢有小车、没技能/无关的人、12F'的都是傻逼，默认丢掉"

即：反馈"控制中枢里有小车（支援机械）""中枢里有没技能/无关的干员""中枢里有 12F"这些类型，默认丢弃，不调查、不记入 `TRACKING.md`，triage 时直接跳过。反馈目录保留原样作为原始证据，但不再处理。
