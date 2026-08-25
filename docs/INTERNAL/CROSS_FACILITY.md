# cross_facility/ 编排层内部地图

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/ORCHESTRATION_LAYER.md；docs/CONTROL_CENTER_ASSIGNMENT.md
> 摘要：定位跨设施作用域的当前实现 owner

> 文件：`crates/infra-core/src/cross_facility/`
> 对外 API：`orchestrate_global_atoms`、`collect_global_atoms`、`GlobalAtomEntry`

## 职责

同房 `EffectAtom` 只影响本房间。但跨房效果（黑键自产感知、乌有自产人间烟火、迷迭香自产感知、森西产魔物料理等）需要汇总到全局池供全基建共享。

本模块是这些跨房效果的**统一执行器**，取代 `layout/resolve.rs` 中按名硬编码的散点注入。

## API

```rust
// 1. 收集全基建 scope=Global 的 atom
let atoms = cross_facility::collect_global_atoms(
    blueprint, assignment, instances, table, &layout,
);

// 2. 执行编排，产出全局资源快照
let snapshot = cross_facility::orchestrate_global_atoms(
    &atoms, &layout, layout.global.clone(),
);
layout.global = snapshot.global;
```

## 代码导航

| 文件 | 函数 | 作用 |
|------|------|------|
| `mod.rs` | `GlobalResourceSnapshot` | 编排输出结构体 |
| `collector.rs` | `collect_global_atoms` | 遍历全基建编制，收集 scope=Global atom，按 Phase 排序 |
| `collector.rs` | `resolve_facility_buff_ids` | 按设施类型解析干员 buff_ids（支持 dorm/office 等非求解设施） |
| `interpreter.rs` | `orchestrate_global_atoms` | 主编排函数，依次执行每个 atom |
| `interpreter.rs` | `apply_state_write` | 执行 StateProduce / StateConvert |
| `interpreter.rs` | `resolve_selector_value` | 跨设施上下文的 Selector 求值 |

## 与现有架构的关系

```
resolve_base()
  ├─ WorkforceIndex 建索引
  ├─ 中枢求解 (control/interpreter.rs)
  ├─ 发电站求解
  ├─ 办公室求解
  ├─ cross_facility 编排 ← 新增
  │   ├─ collect_global_atoms
  │   └─ orchestrate_global_atoms
  ├─ run_conversions
  └─ per-room 求解 (trade/manufacture/power)
```

`run_conversions` 只执行确实会消耗 producer 状态、且 provider/converter buff 均由当班
实际干员携带的全局转换。感知信息与人间烟火是
全基建共享读取状态，不在此扣减；无声共鸣、思维链环、巫术结晶等换算由对应
consumer 的房间 interpreter 局部完成，因此多个 consumer 可读取同一份全量状态。

**用户裁决（2026-08-25）**：`scope=Global` 的 atom 由 `cross_facility` 全局执行一次；
per-room interpreter 不再执行 Global atom，也不再通过 `room_layout_for_*` 扣回全局贡献。
Global `StateConvert` 的具体允许范围和是否进入消耗式转换仍待单独裁决；当前数据未发现
Global `StateConvert` 实例。

- `resolve.rs` 中 `apply_perception_producers` 已删除；其余硬编码迁移进行中。
- per-room 求解只执行 `scope=Room` 的 atom；Global atom 不进入房间解释器。

## 迁移步骤（按干员）

1. 在 `skill_table.json` 中的对应 atom 加 `"scope": "global"`（仅 `StateWrite` atom）
2. 从 `resolve.rs` 删除对应的硬编码函数（`apply_wuyou_human_fireworks_baseline` 等）
3. 删除或改写 `room_layout_for_trade` / `room_layout_for_manu` 中针对 Global atom 的扣回路径
4. 在 trade/manufacture interpreter 中跳过 `scope=Global` atom
5. `cargo test -p infra-core` + `cargo run -p infra-cli -- verify --all`

## 当前已标记 scope=Global 的干员

| 干员 / 技能 | 设施 | 产出 |
|-------------|------|------|
| 黑键·乐感 | 贸易 | 感知（宿舍人数） |
| 迷迭香·超感 | 制造 | 感知（宿舍人数） |
| 乌有·愿者上钩 | 贸易 | 人间烟火（宿舍人数） |
| 桑葚·灾后普查 | 办公室 | 人间烟火（办公室额外招募位×10，精2） |
| 森西·大食堂 | 宿舍 | 魔物料理（宿舍等级） |
| 爱丽丝·梦境呓语 | 宿舍 | 梦境（宿舍等级）→ 同 buff 转感知 |
| 车尔尼·琴键漫步 | 宿舍 | 小节（宿舍等级）→ 同 buff 转感知 |
| 絮雨·巡游/追忆 | 办公室 | 记忆碎片（(Lv-1)×10）→ 同 buff 转感知 |
