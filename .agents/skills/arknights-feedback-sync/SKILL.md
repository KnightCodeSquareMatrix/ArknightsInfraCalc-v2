---
name: arknights-feedback-sync
description: 从生产服务器 114.66.55.78 下载明日方舟基建测试反馈（feedback bundle）到本地并按日期/备注整理。用户要求"下载反馈""同步 feedback""查看服务器反馈""拉取最新反馈"时使用。
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

脚本行为：
- rsync 拉取三个服务的 feedback 目录到临时区，按 `feedbackId` 去重，把**新**反馈写入本地 `feedback/<YYYY-MM-DD北京>/<HHMMSS北京>-<备注slug>/`。
- 命名使用北京时区（CST, UTC+8），与既有本地目录一致；时间优先取 `meta.json` 的 `savedAt`。
- 只输出新同步的目录；已存在的重复项不覆盖输出。
- 自定义参数：`--target root@1.2.3.4`、`--remote-dir <path>`（可重复）、`--local-dir <path>`（默认仓库根 `feedback/`）。

## 手工核对（未跑脚本时的只读快速查询）

```bash
ssh root@114.66.55.78 'ls -lt /var/lib/arknights-infra-dev/feedback/ | head'
```

每个目录内含 `issue.json`（用户反馈正文）、`meta.json`（feedbackId/savedAt/diagnosticId/contract hash）、可选 `debug-bundle.json` 与 `operbox.json`。

## 同步后

1. 只读处理 bundle：`issue.json`、`meta.json`、`debug-bundle.json`、`operbox.json` 是原始证据，triage 期间不编辑；调查结论写 `feedback/TRACKING.md`。
2. 新反馈行记入 `feedback/TRACKING.md`（状态：intake / reproduced / localized / fixing / regressed / closed / blocked / duplicate / duplicate-covered）。
3. 本地重放命令见 `feedback/README.md`（`infra-cli debug rerun --bundle ...`）。
4. 若反馈指向 solver/机制 bug，转 `arknights-maintenance`；契约 hash 与前端呈现问题转前端/`arknights-feature`。
