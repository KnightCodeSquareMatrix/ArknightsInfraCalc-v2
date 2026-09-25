# Debug 与一致性修复指南

> 文档角色：canonical
> 生命周期状态：current
> 领域键：workflow.maintenance
> 当前真源：self
> 摘要：按症状、调用链和证据阶段修复既有结果错误
> 代码范围：`crates/infra-cli/src/commands/`、`crates/infra-core/src/`
> 代码锚点：见[plan 请求调用链](调用链/plan请求.md)和[debug 与验证](调用链/debug与验证.md)
> 关联链路：[错误与诊断](接口/错误与诊断.md)、[项目地图](PROJECT_MAP.md)
> 核验基线：最小真实入口；固定编制 explain；适用时 regression/invariant

## 先判断错误属于哪一层

```text
输入/协议 → adapter → shared plan → rotation/Plan → resolve/room solver → export/output
```

| 症状 | 第一检查点 | 不要先改 |
|---|---|---|
| JSON、路径、字段被拒绝 | schema、`BaseBlueprint::validate`、`inline_operbox` | solver |
| 干员未入池 | `OperBox`、instances tier/facility、pool builder | 输出层 |
| 体系核心缺失或容量冲突 | `build_plan`、`execute_plan`、`used`、rotation admission | 单房 solver |
| 单房效率错误 | `resolve_base` → 对应 trade/manufacture/power solver | CLI 参数 |
| 跨设施资源/中枢错误 | `resolve_base`、control/global_resource/cross_facility | 贸易房局部加法 |
| 轮换、休息、bind 错误 | `schedule_timed_rotation`、shift bind、final invariant | MAA exporter |
| assignment 正确、MAA 字段错误 | `export/maa.rs` | 上游重求解 |
| profile 错误 | `box_profile` 投影与 baseline | rotation 选择 |

机制事实从游戏知识库引用；solver 行为从本仓 canonical、代码和 fresh command 证明，三者不能互相替代。

## 最小复现

优先选择与用户相同的入口：

```bash
cargo run -q -p infra-cli -- plan --layout <layout.json> --operbox <operbox.json> --profile-out <private>/profile.json --maa-out <private>/maa.json
cargo run -q -p infra-cli -- layout team-rotation --layout <layout.json> --operbox <operbox.json> --rotation <profile> --maa-out <private>/maa.json
cargo run -q -p infra-cli -- layout eval --layout <layout.json> --operbox <operbox.json> --assignment <assignment.json> --explain-json > <private>/explain.json
cargo run -q -p infra-cli -- debug rerun --bundle <debug-bundle.json> --output-dir <new-private-dir> --pretty
cargo run -q -p infra-cli -- verify --case <case-id>
```

失败记录必须包括输入 hash、layout、operbox、rotation、top、数据目录、commit、完整错误原文和首个失败阶段。不要执行 bundle 中记录的旧 command 或绝对路径。

## 调查顺序

1. **确认输入**：文件可解析、房间唯一、产品匹配、operbox id/name 唯一且练度可达。
2. **确认共享入口**：`plan`、Worker `plan.compute` 和 legacy `plan` 是否汇合 `plan_compute::compute_plan`；若不一致，先查 adapter 默认值。
3. **确认最终状态**：查看 rotation assignment、active/resting team、bind、used、容量和 partial result。
4. **确认结算**：固定 assignment 用 `layout eval --explain-json`；不要从 top hit 推导未选候选。
5. **确认导出**：只有 assignment 已正确时才查 MAA/profile 字段映射。
6. **回归并分类**：区分 input/solver/timeout/invariant；修复后重跑原入口和适用的 verify/invariant。

## 证据与保证

一次命令成功只证明该输入和当前 runtime 成功。它不能证明全局最优、全量机制覆盖或线上旧制品一致。搜索空间、剪枝、Bake、rotation 或跨设施行为变化时，追加项目规定的 invariant/batch/evidence 门禁。

历史错误报告只保存调查事实，不裁决当前机制；当前案例模板见[求解错误分析报告](求解错误分析报告.md)。

## 4. 回归策略

先重跑用户原入口，再按失败层选择固定编制 explain、debug rerun、verify 或最终 invariant。每次报告同时记录通过项和未运行项，不把单次 smoke 结果扩展为全局保证。

### 2.2 复现入口

优先使用与用户相同的 `plan`、`serve plan.compute`、`layout team-rotation` 或 `layout eval`；生产 bundle 使用 `debug rerun`。命令模板和输出边界见[debug 与验证](调用链/debug与验证.md)。

### 1.1 任务与改动形态

- `local repair`：责任 owner 已存在，只修该层。
- `conformance repair`：确认不变量无法表达，重建单一 owner 并删除冲突兜底。
- 体系/跨设施/轮换一致性问题沿项目对应领域审计流程，不在 CLI 层补丁化。
