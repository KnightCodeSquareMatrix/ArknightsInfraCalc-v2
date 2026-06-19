use std::collections::HashSet;

use crate::control::ControlOperator;
use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::roster::Roster;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

use super::base::{build_roster_pool, filter_pool, HasName, PoolCore};
pub use super::trade::PoolSkip;

#[derive(Debug, Clone)]
pub struct ControlPoolEntry {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
}

impl HasName for ControlPoolEntry {
    fn pool_name(&self) -> &str {
        &self.name
    }
}

impl ControlPoolEntry {
    pub fn to_control_operator(&self) -> ControlOperator {
        ControlOperator {
            name: self.name.clone(),
            elite: self.elite,
            buff_ids: self.buff_ids.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// 向后兼容别名
pub type ControlPool = PoolCore<ControlPoolEntry>;

pub fn build_control_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<ControlPool> {
    // control 按 name 排序（无 eff hint），用常数 0.0 作为 sort_key 统一排序逻辑
    build_roster_pool(roster, instances, table, |_| 0.0, try_entry)
}

pub fn filter_control_pool(pool: &ControlPool, exclude: &HashSet<String>) -> ControlPool {
    filter_pool(pool, exclude)
}

fn try_entry(
    name: &str,
    progress: crate::roster::OperatorProgress,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> std::result::Result<ControlPoolEntry, PoolSkip> {
    let tier = PromotionTier::from_progress(progress);
    let inst = instances.get(name, tier);
    if inst.is_none_or(|i| !i.facilities.contains_key("control")) {
        return Err(PoolSkip::NoTradeBinding);
    }

    let buff_ids = instances.resolve_control_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return Err(PoolSkip::NoTradeBinding);
    }

    for bid in &buff_ids {
        let Some(skill) = table.get(bid) else {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        };
        if skill.facility != "control" {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        }
    }

    let tags = inst.map(|i| i.tags.clone()).unwrap_or_default();

    Ok(ControlPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        buff_ids,
        tags,
    })
}
