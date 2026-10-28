use std::collections::HashSet;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::manufacture::ManuOperator;
use crate::roster::{OperatorProgress, Roster};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::types::{Action, Phase, RecipeKind, SkillDef};

use crate::layout::tier::OperatorTier;

use super::base::{build_roster_pool, filter_pool, HasName, HasProgress, PoolCore, TierTagged};
pub use super::trade::PoolSkip;

#[derive(Debug, Clone)]
pub struct ManuPoolEntry {
    pub name: String,
    pub elite: u8,
    pub progress: OperatorProgress,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    /// Sum of general `AddFlatEff` in `constant` phase — sort hint only.
    pub flat_eff_hint: f64,
    pub has_l2_delegate: bool,
    pub tier: OperatorTier,
}

impl HasName for ManuPoolEntry {
    fn pool_name(&self) -> &str {
        &self.name
    }
}

impl HasProgress for ManuPoolEntry {
    fn progress(&self) -> OperatorProgress {
        self.progress
    }
}

impl TierTagged for ManuPoolEntry {
    fn tier(&self) -> OperatorTier {
        self.tier
    }

    fn set_tier(&mut self, tier: OperatorTier) {
        self.tier = tier;
    }
}

impl ManuPoolEntry {
    pub fn to_manu_operator(&self) -> ManuOperator {
        ManuOperator {
            name: self.name.clone(),
            elite: self.elite,
            buff_ids: self.buff_ids.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// 向后兼容别名
pub type ManuPool = PoolCore<ManuPoolEntry>;

const SYSTEM_ONLY_MANUFACTURE_OPERATORS: [&str; 3] = ["冬时", "温蒂", "迷迭香"];

pub fn build_manufacture_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<ManuPool> {
    build_roster_pool(roster, instances, table, |e| e.flat_eff_hint, try_entry)
}

pub fn filter_manufacture_pool(pool: &ManuPool, exclude: &HashSet<String>) -> ManuPool {
    filter_pool(pool, exclude)
}

pub fn filter_general_manufacture_search_pool(pool: &ManuPool) -> ManuPool {
    ManuPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| {
                !SYSTEM_ONLY_MANUFACTURE_OPERATORS
                    .iter()
                    .any(|name| e.name == *name)
            })
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}

fn try_entry(
    name: &str,
    progress: crate::roster::OperatorProgress,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> std::result::Result<ManuPoolEntry, PoolSkip> {
    let tier = PromotionTier::from_progress(progress);
    let inst = instances.get(name, tier);
    if inst.is_none_or(|i| !i.facilities.contains_key("manufacture")) {
        return Err(PoolSkip::NoTradeBinding);
    }

    let buff_ids = instances.resolve_manufacture_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return Err(PoolSkip::NoTradeBinding);
    }

    let mut flat_eff_hint = 0.0;
    let mut has_l2_delegate = false;
    for bid in &buff_ids {
        let Some(skill) = table
            .dense_id_of(bid)
            .and_then(|id| table.get_by_dense_id(id))
        else {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        };
        if skill.facility != "manufacture" {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        }
        let (flat, delegated) = manu_skill_hints(skill);
        flat_eff_hint += flat;
        has_l2_delegate |= delegated;
    }

    let tags = instances.tags_for(name, tier);

    Ok(ManuPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids,
        tags,
        flat_eff_hint,
        has_l2_delegate,
        tier: OperatorTier::Standalone,
    })
}

fn manu_skill_hints(skill: &SkillDef) -> (f64, bool) {
    if skill.atoms.is_empty() {
        return (0.0, true);
    }
    let mut flat = 0.0;
    for atom in &skill.atoms {
        if atom.tag.as_deref() == Some("station") {
            if let Action::AddFlatEffFromSelector { multiplier, .. } = atom.action {
                flat += multiplier * 3.0;
            }
        }
        if atom.phase == Phase::Constant {
            if let Action::AddFlatEff { value, recipe, .. } = atom.action {
                if recipe.is_none() || recipe == Some(RecipeKind::All) {
                    flat += value;
                }
            }
        }
    }
    (flat, false)
}
