use std::collections::HashSet;
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use crate::control::{solve_control, ControlOperator, ControlRoomInput};
use crate::error::Result;
use crate::global_resource::GlobalResourceKey;
use crate::pool::{combinations_indices, ControlPool};
use crate::skill_table::SkillTable;
use crate::layout::LayoutContext;
use crate::types::RecipeKind;

/// 木天蓼 consumer：贸易/制造侧的泰拉大陆调查团。
pub const MATATABI_CONSUMER_NAME: &str = "泰拉大陆调查团";

#[derive(Debug, Clone, Serialize)]
pub struct ControlSearchHit {
    pub names: Vec<String>,
    pub score: f64,
    pub trade_inject_pct: f64,
    pub manu_gold_inject_pct: f64,
}

/// 中枢补位策略：`base_systems` 钉死后剩余席位按公孙「公招 + 心情」填，而非热情贸易链。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlFillPolicy {
    #[default]
    Efficiency,
    HrAndMood,
}

#[derive(Debug, Clone)]
pub struct ControlSearchOptions {
    pub max_operators: u8,
    pub top_k: usize,
    pub mood: f64,
    pub layout: LayoutContext,
    /// 本班编制是否已有调查团在贸易/制造上岗；无 consumer 时木天蓼不计正分。
    pub matatabi_consumer_active: bool,
    /// 组合必须包含这些干员（如 `base_systems` 已钉死的中枢位）。
    pub must_include: HashSet<String>,
    pub fill_policy: ControlFillPolicy,
}

impl Default for ControlSearchOptions {
    fn default() -> Self {
        Self {
            max_operators: 5,
            top_k: 20,
            mood: 24.0,
            layout: LayoutContext::default(),
            matatabi_consumer_active: false,
            must_include: HashSet::new(),
            fill_policy: ControlFillPolicy::default(),
        }
    }
}

/// 热情/MyGO 经济链 buff：补位阶段排除，避免占满剩余中枢位。
pub fn control_passion_chain_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "control_dorm_bd[000]"
            | "control_mp_bd&trade[000]"
            | "control_prod_bd_spd[000]"
            | "control_prod_bd_spd[010]"
    )
}

fn control_efficiency_inject_buff(buff_id: &str) -> bool {
    buff_id.starts_with("control_prod_spd")
        || buff_id.starts_with("control_tra_spd")
        || buff_id.starts_with("control_token_prod_spd")
        || buff_id == "control_mp_bd[000]"
}

fn control_hr_mood_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "control_hire_spd&bd[000]"
            | "control_dorm_rec2[000]"
            | "control_mp_cost[007]"
            | "control_mp_cost[010]"
            | "control_mp_cost[012]"
            | "control_mp_psk[000]"
    ) || (buff_id.starts_with("control_mp_cost[") && !buff_id.contains('&'))
        || buff_id.starts_with("control_mp_cost&faction")
}

pub fn control_entry_hr_mood_fill(entry: &crate::pool::ControlPoolEntry) -> bool {
    if entry
        .buff_ids
        .iter()
        .any(|b| control_passion_chain_buff(b) || control_efficiency_inject_buff(b))
    {
        return false;
    }
    entry.buff_ids.iter().any(|b| control_hr_mood_buff(b))
}

/// 公招 / 心情类中枢技能（`atoms: []` 挡池条目）的补位加分。
fn control_hr_mood_ancillary(operators: &[ControlOperator], table: &SkillTable) -> f64 {
    let mut score = 0.0;
    for op in operators {
        for bid in &op.buff_ids {
            score += match bid.as_str() {
                // 八幡海铃·可靠伙伴：人脉联络 +10%（skill_table 仅建模热情，补位单独计分）
                "control_hire_spd&bd[000]" => 10.0,
                // 中枢内全员心情 +0.05/h
                "control_mp_cost[007]"
                | "control_mp_cost[010]"
                | "control_mp_cost[012]"
                | "control_mp_psk[000]" => 5.0,
                // 宿舍全员心情 +0.05/h（低于中枢内恢复）
                "control_dorm_rec2[000]" => 2.0,
                _ => {
                    let Some(skill) = table.get(bid) else {
                        continue;
                    };
                    if skill.facility != "control" || !skill.atoms.is_empty() {
                        continue;
                    }
                    if bid.starts_with("control_mp_cost[") && !bid.contains('&') {
                        5.0
                    } else if bid.starts_with("control_mp_cost&faction") {
                        5.0
                    } else {
                        0.0
                    }
                }
            };
        }
    }
    score
}

/// 中枢搜索评分：木天蓼仅在有调查团 consumer 时折算为贸易 eff%；producer 心情消耗恒扣分。
fn score_control_result(
    result: &crate::control::ControlCenterResult,
    operators: &[ControlOperator],
    table: &SkillTable,
    options: &ControlSearchOptions,
) -> f64 {
    let mood_penalty: f64 = result
        .operator_mood_drains
        .values()
        .filter(|v| **v > 0.0)
        .sum();

    if options.fill_policy == ControlFillPolicy::HrAndMood {
        let ancillary = control_hr_mood_ancillary(operators, table);
        return ancillary - mood_penalty * 3.0;
    }

    let mut score = result.inject.trade_eff_pct()
        + result.inject.manu_eff_for(RecipeKind::Gold)
        + result.inject.manu_eff_for(RecipeKind::BattleRecord)
        + result.global.get(GlobalResourceKey::VirtualPower) * 2.0;

    if options.matatabi_consumer_active {
        let matatabi = result.global.get(GlobalResourceKey::Matatabi);
        // 可爱的艾露猫：5% + floor(木天蓼)×3%（中枢搜索按贸易侧计分）
        score += 5.0 + matatabi * 3.0;
    }

    score -= mood_penalty * 2.0;

    score
}

/// 中枢 C(n,k)，k ∈ [1, max_operators]；按全局注入与资源池评分。
pub fn search_control_combos(
    pool: &ControlPool,
    table: &SkillTable,
    options: &ControlSearchOptions,
) -> Result<Vec<ControlSearchHit>> {
    let n = pool.entries.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    let start = Instant::now();
    let max_k = options.max_operators.min(5).min(n as u8) as usize;
    let mut combos: Vec<Vec<usize>> = Vec::new();
    for k in 1..=max_k {
        combos.extend(combinations_indices(n, k));
    }

    let layout = options.layout.clone();
    let mood = options.mood;

    let mut hits: Vec<ControlSearchHit> = combos
        .par_iter()
        .filter_map(|idxs| {
            let operators: Vec<_> = idxs
                .iter()
                .map(|i| pool.entries[*i].to_control_operator())
                .collect();
            let mut names: Vec<String> = operators.iter().map(|o| o.name.clone()).collect();
            names.sort();
            let input = ControlRoomInput {
                operators: operators.clone(),
                mood,
                layout: layout.clone(),
            };
            let result = solve_control(&input, table);
            Some(ControlSearchHit {
                score: score_control_result(&result, &operators, table, options),
                trade_inject_pct: result.inject.trade_eff_pct(),
                manu_gold_inject_pct: result.inject.manu_eff_for(RecipeKind::Gold),
                names,
            })
        })
        .collect();

    if !options.must_include.is_empty() {
        hits.retain(|h| {
            options
                .must_include
                .iter()
                .all(|name| h.names.contains(name))
        });
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.names.cmp(&b.names))
    });
    hits.truncate(options.top_k);
    let _elapsed = start.elapsed();
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{ControlOperator, ControlRoomInput};
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::pool::build_control_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn monhun_control_ops(table: &SkillTable) -> (crate::control::ControlCenterResult, Vec<ControlOperator>) {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("火龙S黑角", 2), ("麒麟R夜刀", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, table).unwrap();
        let ops: Vec<ControlOperator> = ["火龙S黑角", "麒麟R夜刀"]
            .iter()
            .map(|n| pool.entry(n).unwrap().to_control_operator())
            .collect();
        let result = solve_control(
            &ControlRoomInput {
                operators: ops.clone(),
                mood: 24.0,
                layout: LayoutContext::default(),
            },
            table,
        );
        (result, ops)
    }

    #[test]
    fn matatabi_scores_only_with_survey_consumer() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let (result, ops) = monhun_control_ops(&table);
        assert!(result.global.get(GlobalResourceKey::Matatabi) > 0.0);

        let without = score_control_result(
            &result,
            &ops,
            &table,
            &ControlSearchOptions {
                matatabi_consumer_active: false,
                ..Default::default()
            },
        );
        let with = score_control_result(
            &result,
            &ops,
            &table,
            &ControlSearchOptions {
                matatabi_consumer_active: true,
                ..Default::default()
            },
        );
        assert!(
            with > without,
            "consumer active should credit matatabi: with={with} without={without}"
        );
        assert!(
            result.operator_mood_drains.get("麒麟R夜刀").copied().unwrap_or(0.0) > 0.0,
            "夜刀应记账心情消耗"
        );
    }
}
