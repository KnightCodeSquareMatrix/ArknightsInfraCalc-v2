use std::collections::HashSet;

use crate::control::ControlOperator;
use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::roster::Roster;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

pub use crate::pool::trade::{PoolSkip, PoolStats};

#[derive(Debug, Clone)]
pub struct ControlPoolEntry {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct ControlPool {
    pub entries: Vec<ControlPoolEntry>,
    pub skipped: Vec<(String, u8, PoolSkip)>,
}

impl ControlPool {
    pub fn stats(&self) -> PoolStats {
        let n = self.entries.len();
        PoolStats {
            ready: n,
            skipped: self.skipped.len(),
            combinations_3: 0,
        }
    }

    pub fn entry(&self, name: &str) -> Option<&ControlPoolEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

pub fn build_control_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<ControlPool> {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();

    for name in roster.names() {
        let Some(progress) = roster.progress(name) else {
            continue;
        };
        match try_entry(name, progress, instances, table) {
            Ok(entry) => entries.push(entry),
            Err(skip) => skipped.push((name.clone(), progress.elite, skip)),
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ControlPool { entries, skipped })
}

pub fn filter_control_pool(pool: &ControlPool, exclude: &HashSet<String>) -> ControlPool {
    ControlPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| !exclude.contains(&e.name))
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
