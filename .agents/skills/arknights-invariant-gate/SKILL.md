---
name: arknights-invariant-gate
description: 改 solver/search、Plan admission、Team/Shift、rotation、MAA/export、跨设施机制或大规模运行时数据后，在 Rust 最终 assignment 上注册并运行 --assert-invariants，再用 Python 对持久 box 集合批量调用。用于发现"看起来合理但最终分配非法"的结果。
---

# 终态不变量自检

## 断言怎么写

1. 不变量用最终 `TeamRotationReport.shifts[].assignment` 表达，不用文本输出、MAA JSON、fixture 名或当前 top hit。
2. 注册到 Rust `--assert-invariants` gate：排班之后、profile / MAA 输出之前。每项检查有稳定的诊断名。
3. 用结算时同一套运行时输入重建检查：真实 operbox 练度、instances、skill table、解析后的 layout。失败返回 `Result` 错误。
4. Python 只负责批量调用 Rust gate 和分类结果，不解析文本 / MAA，不重写机制语义。现有批量入口是 `scripts/assert_rotation_invariants.py`。

这类大改动的验证靠终态断言加批量 box，不靠新增源码测试。单房间计算这类局部修复可以补最小复现测试。

## 批量输入

- 有代表性的 box：`data/fixtures/box-bundles/collected/collected_operboxes/`。
- high-owned box：`data/fixtures/box-bundles/high-owned/`。
- 不读临时外部压缩包。确实需要时，先解压一次到 `data/fixtures/box-bundles/` 再跑。

## 跑和报告

- 跑一次真实 `plan --assert-invariants`，再跑两组批量。需要留痕时用 `scripts/codex/run_evidence.sh` 包一层。
- 分开报告：检查数、不变量失败、求解无解、输入 / 解析失败、超时。
- 批量全绿只证明这些运行满足所列不变量，不证明全局最优。
- 中高风险改动做完后，按 `arknights-intent-review` 复审。
