# MECHANICS_REGISTRY 机制审计报告说明

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md
> 摘要：说明机制注册审计快照的生成入口、统计口径和误报边界。
> 代码范围：`scripts/audit_mechanics_registry.py`; `data/MECHANICS_REGISTRY.csv`; `data/{operator_instances,skill_table}.json`
> 代码锚点：审计脚本入口 `main`; runtime 对照 `SkillTable::validate_operator_refs`
> 关联链路：`docs/机制/需要完成的干员建模.md`; `docs/机制/MODELLED_OPERATORS.md`
> 核验基线：`947f1a2`

本页只定义如何读生成报告。基线快照（需重新生成确认）为 727 条 registry 行、453 条运行时 scope 行、制造 109 条、跨设施候选 100 条、`has_atoms` 249 条、`empty_atoms` 28 条、按名称未找到 176 条。该统计描述数据快照，不是语义完成度。

运行 `python3 scripts/audit_mechanics_registry.py` 生成 stdout；使用 `--json-output` 写入 target 产物。报告按设施、标签、runtime scope、instances binding 和 skill-table name 做 triage。

`has_atoms`、`empty_atoms`、`not_found_by_name`、`all_bound` 都是粗粒度信号，不是语义覆盖证明：名称漂移、同名技能、buff-id-only 行、多 operator 行和 L2/L3 委托都会产生误报。任何高风险行必须回代码 owner、知识库章节和实际 fixture；不要直接把候选行写入 `skill_table.json`。

交付报告应记录生成日期、registry 输入 hash、runtime data hash 和审计脚本 commit。生成文件属于 evidence/target，不作为手工维护的当前事实。
