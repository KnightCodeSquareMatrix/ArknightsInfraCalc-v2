# 第一次完整运行：243 全精二方案

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/接口/CLI与Worker.md；docs/调用链/plan请求.md
> 摘要：用仓库夹具运行一次 `plan`，检查 profile、rotation 和 MAA 产物
> 代码范围：`crates/infra-cli/src/commands/plan.rs`、`plan_compute.rs`
> 代码锚点：`plan_cmd` → `compute_plan` → `build_from_team_rotation`
> 关联链路：[输入输出与兼容](../接口/输入输出与兼容.md)、[debug 与验证](../调用链/debug与验证.md)
> 核验基线：243 layout + full-e2 operbox；exit code、JSON 解析、班次数和成员互斥

## 输入与命令

仓库夹具：

- `data/fixtures/243/layout.json`
- `data/fixtures/243/operbox_full_e2.json`

执行：

```bash
mkdir -p out/243
cargo run -q -p infra-cli -- plan \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/243/profile.json \
  --maa-out out/243/maa.json \
  --rotation-out out/243/rotation.json
```

命令成功要求 exit code 为 0。stdout 是人类可读 profile + rotation；stderr 包含输入标签、阶段日志和写出路径。使用 `--json` 时 stdout 只打印 profile JSON。

## 产物检查

```bash
python3 -m json.tool out/243/profile.json >/dev/null
python3 -m json.tool out/243/rotation.json >/dev/null
python3 -m json.tool out/243/maa.json >/dev/null
```

`profile.json` 是账号画像和分域指标；`rotation.json` 是班次和最终 assignment；`maa.json` 是对同一 rotation 的导出。三者不是三次独立求解。消费者应检查 profile schema version 和 rotation 的班次数，不要逐行匹配历史快照。

## 结果检查

至少核对：

1. rotation profile 与请求一致；
2. 每班房间人数不超过布局容量，同一班没有重复干员；
3. active/resting team、required bind 和中枢容量满足当前 invariant；
4. MAA 工作房成员来自 rotation assignment，宿舍成员是休息安排，不算工作 presence；
5. 若使用 `--assert-invariants`，断言失败即命令失败，应沿[错误与诊断](../接口/错误与诊断.md)定位；
6. 需要固定编制结算时，用 `layout eval --explain-json`，不要修改 MAA 后再把它当作 solver 输出。

## 这次运行不能证明

该命令只证明标准输入在当前 runtime 下可求解并成功写出产物。它不证明全局最优、所有机制均已建模、所有 operbox 均可行、线上旧二进制一致或 full suite 全绿。需要回归时按 `verify --case/--all`，需要生产 bundle 对照时按 `debug rerun`。
