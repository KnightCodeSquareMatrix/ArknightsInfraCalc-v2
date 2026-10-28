# Contributing to ArknightsInfraCalc

感谢你改进这个项目。仓库同时维护游戏机制、搜索与编排、CLI/接口、运行时数据和领域文档；开始写代码前，请先确认改动由哪一层负责。

## 从哪里开始

- 想了解项目或运行示例：从 [文档入口](docs/INDEX.md) 开始。
- 想定位代码、数据或命令 owner：查 [项目地图](docs/PROJECT_MAP.md)。
- 想修改领域行为：先完整阅读对应的 canonical Markdown；完整 owner 表见 [文档入口的领域规范](docs/INDEX.md#4-领域规范)。
- 使用 AI Agent 协作：先读根 [AGENTS.md](AGENTS.md)，由其中的任务路由和硬门禁选择工作流。

若两个 canonical Markdown 对同一行为给出冲突口径，请先提出冲突并取得裁决，不要依据当前代码、fixture 或某次 top hit 猜测业务规则。

## 责任边界

| 路径 | 负责 | 不负责 |
|---|---|---|
| `crates/infra-core/` | 机制、搜索、编排、排班和导出数据结构 | CLI 参数解析和展示策略 |
| `crates/infra-cli/` | 参数、文件加载、调用编排和忠实输出 | 重新实现机制或重新选人 |
| `data/` | 运行时规则载体、fixture 和回归锚点 | 裁决业务语义 |
| `docs/` | 当前领域规范、实现参考、决策和任务生命周期 | 用历史材料冒充当前能力 |
| `scripts/` | 数据构建、检查、发布和证据工具 | 隐式改变 solver 语义 |

详细 owner 和调用链以 [PROJECT_MAP.md](docs/PROJECT_MAP.md) 为准。Rust 公共 API 从 `crates/infra-core/src/lib.rs` 的 crate 文档进入。

## 实施原则

1. 先复现或定义需要保持的不变量，再修改它的唯一 owner。
2. 保持 `build_plan -> execute_plan -> fill -> resolve` 主路径；CLI 和 export 只消费 core 结果。
3. 不用干员名、房号、Shift 下标、当前 fixture 或 top hit 写补偿特判。
4. 修改候选、剪枝、目标、Bake 或 cache 时，明确它属于 hard constraint、safe reduction、heuristic、policy 还是 approximation；无法证明完整时不要声称全局最优。
5. 代码变化只有在行为语义、接口、持久决策或活动任务状态变化时才需要同步最近的文档 owner。

## 验证

按改动风险选择最小但充分的验证半径。常用入口包括：

```bash
cargo fmt --all -- --check
cargo test -p infra-core <test-filter>
cargo test -p infra-cli <test-filter>
cargo run -q -p infra-cli -- verify --all
python3 scripts/codex/docs_inventory.py --check
python3 scripts/codex/check_repository_facts.py
```

- Rust 改动至少运行相关测试和 `cargo fmt --all`。
- 文档角色、状态、owner 或索引变化必须运行两个文档检查器；生成区块使用 `python3 scripts/codex/docs_inventory.py --write-indexes` 更新，不手工维护表格行。
- 搜索空间、求解保证、性能或跨设施行为变化按 [质量与审计](docs/QUALITY_AND_AUDIT.md) 扩大证据。
- Agent 产出的 build、test、CLI、benchmark、format 和 structure 结论通过 [证据工具](scripts/codex/README.md) 留痕。

若 full suite 存在既有失败，请比较精确失败集合，不要用“本次命令成功”代替整仓结论。

## 提交前检查

- 改动位于正确 owner，没有把机制下沉到 CLI、fixture 或输出层。
- 新行为有对应的边界测试，且未把单一当前结果固化成领域规则。
- 相关格式、测试、文档和真实入口验证已经运行；未运行的类别与残余风险已明确说明。
- 活动任务、ADR 和文档索引符合 [文档生命周期](docs/文档生命周期.md)，完成任务没有继续留在 `docs/TODO/`。
- 提交只包含本项工作的文件，不混入共享工作树中的其他修改。
