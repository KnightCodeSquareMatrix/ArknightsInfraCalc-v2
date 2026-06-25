use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::power::PowerOperator;
use crate::roster::{OperatorProgress, Roster};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::types::{Action, Phase, SkillDef};

use crate::layout::tier::OperatorTier;

use super::base::{build_roster_pool, HasName, PoolCore, TierTagged};
pub use super::trade::PoolSkip;

#[derive(Debug, Clone)]
pub struct PowerPoolEntry {
    pub name: String,
    pub elite: u8,
    pub progress: OperatorProgress,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    /// Sum of constant `AddFlatEff` — sort hint only.
    pub flat_charge_hint: f64,
    pub has_l2_delegate: bool,
    pub tier: OperatorTier,
}

impl HasName for PowerPoolEntry {
    fn pool_name(&self) -> &str {
        &self.name
    }
}

impl TierTagged for PowerPoolEntry {
    fn tier(&self) -> OperatorTier {
        self.tier
    }

    fn set_tier(&mut self, tier: OperatorTier) {
        self.tier = tier;
    }
}

impl PowerPoolEntry {
    pub fn to_power_operator(&self) -> PowerOperator {
        PowerOperator {
            name: self.name.clone(),
            elite: self.elite,
            buff_ids: self.buff_ids.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// 向后兼容别名
pub type PowerPool = PoolCore<PowerPoolEntry>;

pub fn build_power_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<PowerPool> {
    build_roster_pool(roster, instances, table, |e| e.flat_charge_hint, try_entry)
}

fn try_entry(
    name: &str,
    progress: crate::roster::OperatorProgress,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> std::result::Result<PowerPoolEntry, PoolSkip> {
    let tier = PromotionTier::from_progress(progress);
    let inst = instances.get(name, tier);
    if inst.is_none_or(|i| !i.facilities.contains_key("power")) {
        return Err(PoolSkip::NoTradeBinding);
    }

    let buff_ids = instances.resolve_power_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return Err(PoolSkip::NoTradeBinding);
    }

    let mut flat_charge_hint = 0.0;
    let mut has_l2_delegate = false;
    for bid in &buff_ids {
        let Some(skill) = table.get(bid) else {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        };
        if skill.facility != "power" {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        }
        let (flat, delegated) = power_skill_hints(skill);
        flat_charge_hint += flat;
        has_l2_delegate |= delegated;
    }

    let tags = inst.map(|i| i.tags.clone()).unwrap_or_default();

    Ok(PowerPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids,
        tags,
        flat_charge_hint,
        has_l2_delegate,
        tier: OperatorTier::Standalone,
    })
}

fn power_skill_hints(skill: &SkillDef) -> (f64, bool) {
    if skill.atoms.is_empty() {
        return (0.0, true);
    }
    let mut flat = 0.0;
    for atom in &skill.atoms {
        if atom.phase == Phase::Constant {
            if let Action::AddFlatEff { value, .. } = atom.action {
                flat += value;
            }
        }
    }
    (flat, false)
}
