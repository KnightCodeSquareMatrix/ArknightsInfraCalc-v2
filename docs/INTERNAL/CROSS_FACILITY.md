# cross_facility/ 编排层内部地图

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

- `resolve.rs` 中 `apply_perception_producers` 已删除；其余硬编码迁移进行中。
- per-room 求解仍会执行 scope=Room 的 atom（含 scope=Global 的 StateProduce 副本），`room_layout_for_*` 函数扣回全局已计数的部分。

## 迁移步骤（按干员）

1. 在 `skill_table.json` 中的对应 atom 加 `"scope": "global"`（仅 `StateWrite` atom）
2. 从 `resolve.rs` 删除对应的硬编码函数（`apply_wuyou_human_fireworks_baseline` 等）
3. 更新 `room_layout_for_trade` / `room_layout_for_manu` 扣回逻辑（从按名扣回改为 scope=Global atom 检测）
4. `cargo test -p infra-core` + `cargo run -p infra-cli -- verify --all`

## 当前已标记 scope=Global 的干员

| 干员 / 技能 | 设施 | 产出 |
|-------------|------|------|
| 黑键·乐感 | 贸易 | 感知（宿舍人数） |
| 迷迭香·超感 | 制造 | 感知（宿舍人数） |
| 乌有·愿者上钩 | 贸易 | 人间烟火（宿舍人数） |
| 森西·大食堂 | 宿舍 | 魔物料理（宿舍等级） |
| 爱丽丝·梦境呓语 | 宿舍 | 感知（宿舍等级，精2） |
| 车尔尼·琴键漫步 | 宿舍 | 感知（宿舍等级，精2） |
| 絮雨·语言学 | 办公室 | 感知（(Lv-1)×10，精2） |
