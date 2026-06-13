use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::power::PowerOperator;
use crate::roster::Roster;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::types::{Action, Phase, SkillDef};

pub use crate::pool::trade::{PoolSkip, PoolStats};

#[derive(Debug, Clone)]
pub struct PowerPoolEntry {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    /// Sum of constant `AddFlatEff` — sort hint only.
    pub flat_charge_hint: f64,
    pub has_l2_delegate: bool,
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

#[derive(Debug, Clone)]
pub struct PowerPool {
    pub entries: Vec<PowerPoolEntry>,
    pub skipped: Vec<(String, u8, PoolSkip)>,
}

impl PowerPool {
    pub fn stats(&self) -> PoolStats {
        let n = self.entries.len();
        PoolStats {
            ready: n,
            skipped: self.skipped.len(),
            combinations_3: n as u64,
        }
    }

    pub fn entry(&self, name: &str) -> Option<&PowerPoolEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

pub fn build_power_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<PowerPool> {
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

    entries.sort_by(|a, b| {
        b.flat_charge_hint
            .partial_cmp(&a.flat_charge_hint)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(PowerPool { entries, skipped })
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
        let (flat, delegated) = skill_hints(skill);
        flat_charge_hint += flat;
        has_l2_delegate |= delegated;
    }

    let tags = inst.map(|i| i.tags.clone()).unwrap_or_default();

    Ok(PowerPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        buff_ids,
        tags,
        flat_charge_hint,
        has_l2_delegate,
    })
}

fn skill_hints(skill: &SkillDef) -> (f64, bool) {
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
