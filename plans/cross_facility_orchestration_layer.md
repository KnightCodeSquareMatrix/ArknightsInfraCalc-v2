# 跨设施编排层设计报告

> 设计目标：为 `ArknightsInfraCalc-v2` 引入正式的跨设施编排层，消除 AI 遇到跨房 buff 时打补丁的现状。

---

## 一、问题诊断

当前系统中，跨设施效果（中枢→贸易/制造全局注入、感知信息链、人间烟火链、魔物料理链等）的处理方式是**散点式**的：

### 1.1 已有良好接口但待强化的

| 路径 | 机制 | 现状评级 |
|------|------|---------|
| `control/interpreter.rs` → `layout.global_inject` | 中枢 → 全局效率% | ✅ 接口清晰，`EffectAtom` 驱动 |
| `control/interpreter.rs` → `layout.global` | 中枢 → 全局资源池（木天蓼等） | ✅ 接口清晰 |
| `global_resource/pool.rs` | 全局资源池管理 | ✅ 泛型良好 |
| `global_resource/inject.rs` | 全局注入清单 + 族取最大策略 | ✅ 设计合理 |
| `WorkforceIndex::apply_cross_facility_stats` | tag 跨设施统计 | ✅ 系统化 |

### 1.2 散点补丁（问题核心）

以下效果**不是**通过 `EffectAtom` 系统表达的，而是通过**按干员名硬编码**写在 `layout/resolve.rs` 中：

| 函数 | 硬编码 | 问题 |
|------|--------|------|
| `apply_wuyou_human_fireworks_baseline` | `"乌有"` 名检查 | 不在 `skill_table`，不参与 Phase |
| `apply_senxi_monster_cuisine_from_assignment` | `"森西"` + buff 名 | 类似硬编码 |
| `apply_perception_producers` | `"爱丽丝"`、`"车尔尼"`、`"絮雨"` | 硬编码名 + 等级公式 |
| `room_layout_for_trade` 中的扣回逻辑 | `BLACKKEY_PERCEPTION_BUFF` | 需要手动扣重 |
| `room_layout_for_manu` 中的扣回逻辑 | `ROSEMARY_PERCEPTION_BUFF` | 同上 |

这些函数共约 **120 行**硬编码逻辑，分布在 `resolve.rs`（~1100 行），且每次新增跨设施干员都需要在这里加补丁。

### 1.3 根本原因

```
EffectAtom 系统只覆盖了「同房」效果。
跨设施效果因为没有「跨房上下文」，所以退化为按名硬编码。
```

---

## 二、方案对比

### 方案 A（你的思路）：新增跨设施编排层

```
cross_facility/ 模块
  ├── 收集全基建所有设施的 atoms
  ├── 按 Phase 排序执行（StateWrite → conversions）
  ├── 产出全局资源池 + 注入清单
  └── 喂入 per-room 求解
```

### 方案 B：在 resolve.rs 内形式化流水线

```
resolve_base 内部形式化为 7 步：
  1. WorkforceIndex 建索引
  2. 发电站求解（影响 effective_power_station_count）
  3. 中枢求解（global_inject + global pool）
  4. 跨设施资源求解（宿舍/办公室/自producer）
  5. 全局资源转化 run_conversions
  6. 各房间求解
```

### 方案 C（推荐）：继承方案 A 的设计，但复用 EffectAtom 系统

**核心思路**：让跨设施效果也走 `EffectAtom` + `SkillTable` 路径，消除按名硬编码。

| 维度 | 方案 A | 方案 B | **方案 C** |
|------|--------|--------|-----------|
| 消除硬编码 | ✅ 可消除 | ❌ 保留硬编码 | ✅ 完全消除 |
| 新增干员成本 | 低 | 中 | **最低**（只需在 `skill_table.json` 加 atom） |
| 动代码量 | 大（新模块） | 中（重构现有） | **中**（新模块 + migrate atoms） |
| 与现有架构兼容 | ⚠️ 新入口 | ✅ 兼容 | ✅ 兼容 |
| 是否改数据结构 | ❌ 不必须 | ❌ 不必须 | **需要扩展 facility 域** |

---

## 三、方案 C 详细设计

### 3.1 新模块结构

```
crates/infra-core/src/cross_facility/
├── mod.rs          # 主入口 orchestrate_cross_facility()
├── interpreter.rs  # 跨设施 Phase 执行
├── collector.rs    # 从全基建收集 atoms
└── resolver.rs     # 与 resolve_base 的集成点
```

`orchestrate_cross_facility()` 接受全基建蓝图的完整编制，输出 `GlobalResourceSnapshot`：

```rust
pub struct GlobalResourceSnapshot {
    pub global: GlobalResourcePool,
    pub inject: GlobalInjectManifest,
    pub layout_derived: LayoutContext,  // 不含 global/inject，仅布局字段
}
```

### 3.2 收集范围（谁参与跨设施编排）

当前需要纳入的设施/干员类型：

| 设施 | 跨设施效果 | 当前处理方式 | 迁移后 |
|------|-----------|-------------|--------|
| 控制中枢 | `global_inject`、全局资源生产 | `control/interpreter.rs` | 保留，纳入编排 |
| 宿舍（爱丽丝/车尔尼） | 感知信息 | `resolve.rs` 硬编码 | 改为 `dorm` 域 `StateWrite` atom |
| 办公室（絮雨） | 感知信息 | `resolve.rs` 硬编码 | 同上 |
| 发电站 | 虚拟发电量、平台统计 | `resolve.rs` + `workforce.rs` | 保留，纳入编排 |
| 贸易站（黑键） | 感知信息（自产+扣回） | `resolve.rs` 硬编码 | 改为 `StateWrite` + `room_layout_for_trade` 自动 |
| 制造站（迷迭香） | 感知信息（自产+扣回） | `resolve.rs` 硬编码 | 同上 |
| 宿舍（森西） | 魔物料理 | `resolve.rs` 硬编码 | 改为 `dorm` 域 `StateWrite` atom |
| 贸易站（乌有） | 人间烟火 | `resolve.rs` 硬编码 | 改为 `trade` 域 `StateWrite` atom |
| 中枢+贸易站（灵知） | 精密计算 | `inject.rs` 接口 | 保留，纳入编排 |

### 3.3 Phase 扩展

需要新增或正式化以下 Phase 用于跨设施场景：

| Phase | 用途 | 示例 |
|-------|------|------|
| `StateWrite` | ✅ 已有 | 中枢产木天蓼、宿舍产感知 |
| `GlobalInject` | ✅ 已有 | 中枢注入全贸易/全制造% |
| **`CrossFacilityCollect`** | **新增** | 非中枢设施向全局产资源（黑键感知、乌有烟火） |
| `Energy`（原 Mood 范围） | 待定 | 发电站虚拟发电 |

同时需要在 `skill_table.json` 的 `facility` 字段基础上，在处理跨设施 atom 时按 `facility` + `room_kind` 过滤。

**核心问题**：现有 `skill_table.json` 的 `facility` 字段标注了技能所属设施，但跨设施 atom 需要标注「效果影响范围」。建议新增 `scope` 字段：

```json
{
  "id": "trade_ord_spd_bd_n1[000]",
  "facility": "trade",
  "atoms": [{
    "phase": "state_write",
    "scope": "global",
    "selector": "DormOccupantCount",
    "action": { "StateProduce": { "key": "perception", "amount": 1.0 } }
  }]
}
```

`scope` 取值范围：
- `"room"`（默认）— 同房效果，现有行为
- `"global"` — 跨设施效果，写入全局资源池

### 3.4 编排流水线

```
resolve_base 调用或独立测试：
  1. WorkforceIndex 建索引 + layout stats 快照
  2. cross_facility::collect_cross_facility_atoms
     - 遍历全基建所有设施的 operator
     - 收集 scope=global 的 atom
  3. cross_facility::orchestrate
     - 按 Phase 排序执行 atom
     - StateWrite → GlobalInject → 资源转化
     - 产出 GlobalResourceSnapshot
  4. Per-room 求解（不变）
     - 每个房间 = base_snapshot + 扣回自产 + 房内 atoms 执行
```

### 3.5 迁移路径（不破坏现有逻辑）

**迁移策略：双轨并行 + 逐步淘汰**

| 阶段 | 内容 | 测试 |
|------|------|------|
| **Phase 0** | 新建 `cross_facility/` 模块骨架，`orchestrate` 空实现 | 不影响现有逻辑 |
| **Phase 1** | 将 `control/interpreter.rs` 集成到编排层 | 输出应与现有完全一致 |
| **Phase 2** | 将 `apply_wuyou_human_fireworks_baseline` 改为 atom | 回归夹具验证 |
| **Phase 3** | 将 `apply_senxi_monster_cuisine_from_assignment` 改为 atom | 回归夹具验证 |
| **Phase 4** | 将 `apply_perception_producers` 拆分为各设施的 scope=global atoms | 感知链测试验证 |
| **Phase 5** | 删除 `resolve.rs` 中的对应硬编码函数 + `room_layout_for_*` 扣回逻辑 | 全回归通过 |
| **Phase 6** | 新增 CLI `cross-facility` 子命令用于独立调试/验证 | 文档更新 |

各阶段保持 `cargo test -p infra-core` + `verify --all` 全绿。

---

## 四、核心数据结构变更

### 4.1 `types.rs` 新增

```rust
/// atom 影响范围：同房（默认）或全基建。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AtomScope {
    #[serde(alias = "room")]
    Room,
    #[serde(alias = "global")]
    Global,
}

impl Default for AtomScope {
    fn default() -> Self { Self::Room }
}
```

### 4.2 `EffectAtom` 新增字段

```rust
pub struct EffectAtom {
    pub selector: Option<Selector>,
    pub action: Action,
    pub condition: Option<Condition>,
    pub tag: Option<String>,
    pub phase: Phase,
    pub phase_order: i32,
    #[serde(default)]
    pub scope: AtomScope,  // 新增
}
```

### 4.3 `cross_facility` 模块新增类型

```rust
/// 跨设施编排的输出：一个全基建快照，供后续 per-room 求解使用。
#[derive(Debug, Clone)]
pub struct GlobalResourceSnapshot {
    /// 全局资源池（生产完毕，所有转化已完成）。
    pub global: GlobalResourcePool,
    /// 中枢注入清单。
    pub inject: GlobalInjectManifest,
    /// 全基建布局衍生统计（trade/manu/power count 等；不含 global/inject）。
    pub layout: LayoutContext,
}
```

### 4.4 与现有 `resolve_base` 的集成

```rust
// 在 resolve_base 中替换现有散点逻辑:

// 旧:
// apply_wuyou_human_fireworks_baseline(&workforce, blueprint, &mut layout);
// apply_senxi_monster_cuisine_from_assignment(blueprint, assignment, instances, &mut layout);
// apply_perception_producers(blueprint, assignment, instances, &mut layout);

// 新:
let snapshot = cross_facility::orchestrate(
    &workforce, blueprint, assignment, instances, table, mood
);
layout.global = snapshot.global;
layout.global_inject = snapshot.inject;
```

---

## 五、skill_table.json 迁移示例

### 乌有·人间烟火（当前硬编码 → 改为 atom）

```json
// 当前: resolve.rs 中按名检查
if op.name == "乌有" && op.elite >= 2 {
    layout.global.add(GlobalResourceKey::HumanFireworks, dorm_occupant_count);
}

// 迁移后: skill_table.json
{
  "id": "trade_ord_spd_bd_n2[000]",
  "facility": "trade",
  "name": "人间烟火·乌有",
  "atoms": [{
    "phase": "state_write",
    "scope": "global",
    "phase_order": 0,
    "selector": "DormOccupantCount",
    "action": {
      "StateProduce": {
        "key": "human_fireworks",
        "amount": 1.0
      }
    }
  }]
}
```

### 森西·魔物料理（当前硬编码 → 改为 atom）

```json
{
  "id": "dorm_rec_bd_dungeon[000]",
  "facility": "dorm",
  "name": "森西大食堂",
  "atoms": [{
    "phase": "state_write",
    "scope": "global",
    "phase_order": 0,
    "selector": "FacilityLevel",
    "action": {
      "StateProduce": {
        "key": "monster_cuisine",
        "amount": 1.0
      }
    }
  }]
}
```

### 爱丽丝/车尔尼·感知（当前硬编码 → 改为 atom）

```json
{
  "id": "dorm_rec_spd_n2_percept[000]",
  "facility": "dorm",
  "atoms": [{
    "phase": "state_write",
    "scope": "global",
    "phase_order": 0,
    "condition": { "MoodAbove": { "n": 12 } },
    "selector": "FacilityLevel",
    "action": {
      "StateProduce": {
        "key": "perception",
        "amount": 1.0
      }
    }
  }]
}
```

### 黑键·乐感（当前 resolve.rs 双计数+扣回 → 改为 atom）

原子自带 `scope: global`，跨设施编排层累加全量；room_layout_for_trade 自动扣回本房自产（通过 operator_id 匹配）。

```json
{
  "id": "trade_ord_spd_bd_n1[000]",
  "facility": "trade",
  "atoms": [{
    "phase": "state_write",
    "scope": "global",
    "phase_order": 0,
    "selector": "DormOccupantCount",
    "action": {
      "StateProduce": {
        "key": "perception",
        "amount": 1.0
      }
    }
  }]
}
```

---

## 六、取消 `room_layout_for_trade` 和 `room_layout_for_manu` 中的自产扣回

当前逻辑：

```rust
// resolve.rs
fn room_layout_for_trade(layout, operators) -> LayoutContext {
    let mut room_layout = layout.clone();
    if operators_include_wuyou_producer(operators) {
        // 扣回乌有自产人间烟火（全局已计数，本房 atom 再产 = 重复）
        room_layout.global.add(HumanFireworks, -dorm_count);
    }
    if operators include blackkey {
        // 扣回黑键自产感知
        room_layout.global.add(Perception, -dorm_count);
    }
    room_layout
}
```

迁移后，跨设施编排层已经执行了 scope=global atoms 并累加到 `GlobalResourceSnapshot`。Per-room 求解时：

1. 取得编排层产出的 `GlobalResourcePool`（已含黑键/乌有/迷迭香/爱丽丝/车尔尼等全量）
2. room_layout 检查本房 operator 是否执行了 scope=global atom
3. 如果是，扣除该 atom 的贡献（避免本房 atom 再次计入重复）

这种「先全局累加，再在 room_layout 中按名扣回」的模式需要保留，但应改为**声明式**：在 atom 中加入 `scope: "global"` 标记，编排基础设施自动处理扣回。

```rust
// 跨设施编排产出后，room_layout_for_* 改为通用逻辑：
fn deduct_room_self_global(layout: &mut LayoutContext, operators, table) {
    for op in operators {
        for bid in &op.buff_ids {
            let skill = table.get(bid);
            for atom in &skill.atoms {
                if atom.scope == AtomScope::Global {
                    // 计算该 atom 本应贡献的值
                    let value = evaluate_atom_global_contribution(atom, layout);
                    // 扣除，避免 room atom 再产重复
                    deduct_from_global_pool(layout, atom, value);
                }
            }
        }
    }
}
```

---

## 七、执行顺序（跨设施 Phase 排程）

```
跨设施 Phase 执行顺序（Phase::sort_key 基础上编排）:

顺序  Phase              设施            说明
────  ─────────────────  ──────────────  ───────────────────────
  1   StateWrite         宿舍/办公室      森西产魔物料理、爱丽丝/车尔尼产感知
  2   StateWrite         发电站           虚拟发电、平台状态
  3   StateWrite         中枢            木天蓼、高塔、烟火、共鸣
  4   StateWrite         贸易站           黑键自产感知、乌有自产烟火
  5   StateWrite         制造站           迷迭香自产感知
  6   GlobalInject       中枢            全贸易/全制造global %
  7   [资源转化]          —               run_conversions
  8   [Per-room solve]   各设施           trade/manu/power 各自求解
```

关键约束：
- **宿舍/办公室先于贸易/制造**：黑键/迷迭香的前置感知来自宿舍
- **resource conversions 最后**：确保所有 producer 写入后才转化
- **per-room solve 最后**：确保全局资源已冻结

---

## 八、影响范围与兼容性

### 8.1 影响文件

| 文件 | 改动类型 | 预期行数 |
|------|---------|---------|
| `types.rs` | 新增 `AtomScope` enum、`EffectAtom.scope` | +20 行 |
| `cross_facility/mod.rs` | **新建** | +80 行 |
| `cross_facility/collector.rs` | **新建** | +100 行 |
| `cross_facility/interpreter.rs` | **新建** | +150 行 |
| `cross_facility/resolver.rs` | **新建** | +80 行 |
| `layout/resolve.rs` | 删除散点函数，集成编排层 | -120 行 + 60 行 |
| `control/interpreter.rs` | 增加 `scope: global` 感知 | +5 行 |
| `skill_table.json` | 迁移 per-facility 资源 atom | +~15 条目 |
| `operator_instances.json` | 可能更新 facility 绑定 | 按需 |

### 8.2 不影响的文件

- `trade/interpreter.rs` — 保持同房求解不感知跨设施
- `manufacture/interpreter.rs` — 同上
- `search/trade.rs` / `search/manufacture.rs` — 编排结果直接喂入 layout
- `pool/trade.rs` / `pool/manufacture.rs` — 不变
- `shortcut.rs` / `trade_shortcuts.json` — 不变
- CLI 命令和输出格式 — 不变

### 8.3 兼容性保证

1. **回归夹具**：所有 `verify --all` 用例的输出数值不变
2. **搜索排序**：`search trade` Top-K 排序不变
3. **排班输出**：`layout test / rotation / team-rotation` 输出不变
4. **公有 API**：`solve_trade` / `solve_manufacture` 签名不变

---

## 九、风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| scope=global atom 扣回漏算 | 中 | 算分翻倍 | 自动化测试：黑键/乌有/迷迭香的夹具验证 |
| Phase 顺序与游戏不一致 | 低 | 感知/烟火链少算 | 对照 PRTS + 游戏实测 |
| skill_table 迁移遗漏 | 中 | 跨设施效果为0 | `verify --all` 检测预期值，匹配旧版输出 |
| 排班集成出错 | 低 | 排班输出异常 | `layout test --rotation` 回归夹具 |
| 制造站误用贸易跨设施逻辑 | 低 | 制造站算分偏差 | 制造站侧单独验证，不做跨设施误触 |

---

## 十、实施建议（TODO）

1. **先读**：本报告 + `types.rs` + `layout/resolve.rs` + `global_resource/mod.rs`
2. **Phase 0-1**：`cross_facility/mod.rs` 骨架 + 集成控制中枢 → 验证输出不变
3. **Phase 2-4**：逐个迁移 resolve.rs 中的硬编码函数 → 每步跑 `verify --all`
4. **Phase 5**：删除旧代码 + 清理扣回逻辑
5. **Phase 6**：文档 + CLI cross-facility 子命令（可选）
6. **验证**：`cargo test -p infra-core` + `cargo run -p infra-cli -- verify --all`

---

## 附录：跨设施效果完整清单

| 干员 | 设施 | 产/消 | 资源/效果 | 当前实现位置 | 迁移优先级 |
|------|------|-------|----------|-------------|-----------|
| 爱丽丝 | 宿舍 | 产 | 感知 (每级+1) | `resolve.rs:apply_perception_producers` | P1 |
| 车尔尼 | 宿舍 | 产 | 感知 (每级+1) | 同上 | P1 |
| 森西 | 宿舍 | 产 | 魔物料理 (每级+1) | `resolve.rs:apply_senxi_monster_cuisine` | P1 |
| 絮雨 | 办公室 | 产 | 感知 ((Lv-1)*10) | `resolve.rs:apply_perception_producers` | P1 |
| 黑键 | 贸易站 | 产 | 感知 (=dorm) | `resolve.rs` + `room_layout_for_trade` 扣回 | P1 |
| 迷迭香 | 制造站 | 产 | 感知 (=dorm) | `resolve.rs` + `room_layout_for_manu` 扣回 | P1 |
| 乌有 | 贸易站 | 产 | 人间烟火 (=dorm) | `resolve.rs:apply_wuyou_human_fireworks` + 扣回 | P1 |
| 令 | 中枢 | 产 | 人间烟火 15 | `control/interpreter.rs` | 已走 atom |
| 夕 | 中枢 | 产 | 感知 10 | `control/interpreter.rs` | 已走 atom |
| 重岳 | 中枢 | 产 | 人间烟火 (=岁) | `control/interpreter.rs` | 已走 atom |
| 火龙S黑角 | 中枢 | 产 | 木天蓼 | `control/interpreter.rs` | 已走 atom |
| 麒麟R夜刀 | 中枢 | 产 | 木天蓼 | `control/interpreter.rs` | 已走 atom |
| 战车 | 中枢 | 产 | 乌萨斯特饮 | `control/interpreter.rs` | 已走 atom |
| 灰烬 | 中枢 | 产 | 情报储备 | `control/interpreter.rs` | 已走 atom |
| 森蚺 | 中枢 | 产 | 虚拟发电 | `control/interpreter.rs` | 已走 atom |
| 阿米娅 | 中枢 | 注入 | 全局贸易+7% | `control/interpreter.rs` | 已走 atom |
| 灵知 | 中枢 | 注入 | 精密计算 | `control/interpreter.rs` | 已走 atom |
| 若叶睦 | 中枢 | 注入 | 热情→贸易+制造 | `control/interpreter.rs` | 已走 atom |
| 杜林/鸿雪 | 全基建 | 统计 | 虚拟赤金线 | `workforce.rs` + `context.rs` | 已系统化 |
