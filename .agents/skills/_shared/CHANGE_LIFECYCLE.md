# Change Lifecycle Adapter

文档角色、状态、唯一 owner 和关闭事务唯一以 [`docs/文档生命周期.md`](../../../docs/文档生命周期.md) 为准。本文只说明项目 Skill 如何调用该协议，不定义第二套生命周期。

## 何时调用

任务拥有 active change、计划、ADR 变更、文档迁移或关闭动作时，在 primary maintenance / feature / quality Skill 之外调用本适配器。它不改变用户授权或任务 scope。

## 执行入口

1. 开始时确认 active change、current owner 和一个 writer。
2. 实施中把用户裁决先写入 canonical；change 只记录 delta、证据和开放项。
3. 结束时按 canonical 完成事实吸收、开放项拆分、移动/删除、生成索引和引用闭合。
4. 通过 `scripts/codex/docs_inventory.py --check` 检查最终树；大型迁移或需要长期审计时，再使用 Evidence Skill 留痕。`in-progress` 或 report-only 不能作为完成状态。

遇到业务语义冲突、产品范围变化或未吸收用户裁决时暂停询问。文件数、移动数和 diff 大小不是暂停理由。
