# 前端双 JSON 输出契约

> 状态：done
> 来源：用户需求：后端输出账户画像和 MAA 排班表两个 JSON，前端单独仓库读取，不改成本项目 monorepo

## 目标

- `infra-cli plan` 作为前端主入口，同时写出账户画像 JSON 和 MAA 排班表 JSON。
- 前端通过 `--profile-out <account_profile.json>` 读取账户画像，通过 `--maa-out <maa_schedule.json>` 读取 MAA 排班表。
- `--json` 模式输出纯账户画像 JSON 到 stdout，避免混入人类可读报告。
- 文档明确前端不要解析 stdout 文本作为结构化数据。

## 非目标

- 不把前端仓库移入本项目。
- 不把后端改成 Web 服务。
- 不改变求解、评分、排班逻辑。
- 不新增跨贸易 / 制造综合权重。

## 改动范围

| 文件/目录 | 动作 |
|-----------|------|
| `crates/infra-cli/src/commands/plan.rs` | 修正 `--json` stdout 为纯 profile JSON |
| `crates/infra-cli/src/commands/layout.rs` | 同步修正 `layout analyze --json` stdout 为纯 profile JSON |
| `docs/FRONTEND_CLI.md` | 固化前端双 JSON 文件读取契约 |
| `docs/TODO/README.md` | 登记当前 TODO |

## 验收

- [x] `cargo build -p infra-cli`
- [x] `cargo run -q -p infra-cli -- plan --operbox data/fixtures/243/operbox_full_e2.json --profile-out out/frontend_profile.json --maa-out out/frontend_maa.json --json`
- [x] `out/frontend_profile.json` 是账户画像 JSON
- [x] `out/frontend_maa.json` 是 MAA 排班 JSON
- [x] `plan --json` stdout 可直接被 JSON parser 解析

## 完成后归档

完成后移动到 `docs/ARCHIVE/done/`，并更新 `docs/FRONTEND_CLI.md`。
