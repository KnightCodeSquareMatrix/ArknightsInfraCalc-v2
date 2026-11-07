//! 赤金生产线链式结算（贸易站内按进驻顺序迭代）。
//!
//! **L2 域短路**（见 `docs/机制/EFFECT_ATOM_DESIGN.md` §8.5）：`skill_table` 中对应 buff 的
//! `atoms` 为空是刻意委托，不在此重复建模为 EffectAtom。
//!
//! 真实制造赤金线数由 `TradeRoomInput::gold_production_lines` 注入；
//! 虚拟线由在岗贸易干员生成，供所有赤金贸易站读取；本房按进驻顺序重放自身的线。

use crate::skill_table::SkillTable;
use crate::trade::interpreter::TradeContext;

use crate::response_dependency::{DomainDependencyInputDecl, DomainInputSource};

const DEPENDENCY_INPUTS: &[DomainDependencyInputDecl] = &[
    DomainDependencyInputDecl {
        name: "real_gold_lines",
        source: DomainInputSource::Blueprint,
        external_signature: true,
        note: "physical gold factory count",
    },
    DomainDependencyInputDecl {
        name: "virtual_gold_lines",
        source: DomainInputSource::GlobalResource,
        external_signature: true,
        note: "initial global virtual gold lines before room-local transitions",
    },
    DomainDependencyInputDecl {
        name: "durin_virtual_lines",
        source: DomainInputSource::LayoutStatistic,
        external_signature: true,
        note: "Durin operators in base, capped at four",
    },
    DomainDependencyInputDecl {
        name: "ordered_gold_flow_roles",
        source: DomainInputSource::CandidateRow,
        external_signature: false,
        note: "ordered L2 buff roles carried by the room candidate",
    },
    DomainDependencyInputDecl {
        name: "active_order_kind",
        source: DomainInputSource::RoomContext,
        external_signature: false,
        note: "gold-flow executes only for gold orders",
    },
];

pub fn dependency_inputs() -> &'static [DomainDependencyInputDecl] {
    DEPENDENCY_INPUTS
}

/// 候选准入消费本域委托技能的外部虚拟线读取事实，不重新实现赤金公式。
pub(crate) fn reads_virtual_lines(buff_ids: &[String]) -> bool {
    buff_ids.iter().any(|buff| {
        matches!(
            buff.as_str(),
            "trade_ord_spd&gold[100]" | "trade_ord_spd&gold[010]" | "trade_ord_spd&gold[000]"
        )
    })
}

pub fn virtual_line_yield(buff_ids: &[String], real: u32, durin: u32) -> u32 {
    buff_ids
        .iter()
        .map(|id| match id.as_str() {
            "trade_ord_line_gold[010]" => (real / 2) * 2,
            "trade_ord_line_gold[000]" => (real / 4) * 2,
            "trade_ord_line_durin[010]" => durin,
            _ => 0,
        })
        .sum()
}

pub fn ordered_trio_priority(name: &str) -> u8 {
    match name {
        "绮良" => 0,
        "鸿雪" => 1,
        "图耶" => 2,
        _ => 3,
    }
}

pub fn has_ordered_trio<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = [false; 3];
    for name in names {
        let priority = ordered_trio_priority(name);
        if priority < 3 {
            seen[priority as usize] = true;
        }
    }
    seen.into_iter().all(|present| present)
}

pub fn has_ordered_gold_flow<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = [false; 3];
    for name in names {
        let priority = ordered_trio_priority(name);
        if priority < 3 {
            seen[priority as usize] = true;
        }
    }
    seen.into_iter().filter(|present| *present).count() >= 2
}

/// 在 `PeerAbsorb` 之前执行：按干员进驻顺序结算赤金链效率。
pub fn apply_gold_flow_chain(ctx: &mut TradeContext<'_>, table: &SkillTable) {
    let real = ctx.real_gold_lines;
    let mut virtual_lines = ctx.virtual_gold_lines;
    for (name, amount, _) in &ctx.layout.trade_gold_line_sources {
        if ctx
            .operators
            .iter()
            .any(|operator| operator.name == name.as_str())
        {
            virtual_lines = virtual_lines.saturating_sub(*amount);
        }
    }
    for owner_idx in 0..ctx.operators.len() {
        let name = ctx.operators[owner_idx].name;
        // Preserve the existing first-matching owner behavior for duplicate names.
        let buff_ids = ctx
            .operators
            .iter()
            .find(|o| o.name == name)
            .map(|o| o.buff_ids)
            .unwrap_or_default();

        let mut extra = 0.0f64;
        for bid in buff_ids {
            if table.get(bid).is_none() {
                continue;
            }
            let total = real + virtual_lines;
            match bid.as_str() {
                "trade_ord_line_gold[010]" => {
                    extra += 5.0;
                    virtual_lines += (real / 2) * 2;
                }
                "trade_ord_line_gold[000]" => {
                    extra += 5.0;
                    virtual_lines += (real / 4) * 2;
                }
                "trade_ord_line_durin[010]" => {
                    virtual_lines += ctx.durin_virtual_lines;
                }
                "trade_ord_spd&gold[100]" => {
                    extra += total as f64 * 5.0;
                }
                "trade_ord_spd&gold[010]" => {
                    extra += 5.0 + (total / 2) as f64 * 15.0;
                }
                "trade_ord_spd&gold[000]" => {
                    extra += 5.0 + (total / 4) as f64 * 15.0;
                }
                _ => {}
            }
        }

        if extra > 0.0 {
            if let Some(idx) = ctx.operators.iter().position(|o| o.name == name) {
                ctx.operators[idx].settled_eff += extra;
            }
        }
        ctx.virtual_gold_lines = virtual_lines;
    }
}
