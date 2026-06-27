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

const SYSTEM_ONLY_MANUFACTURE_OPERATORS: [&str; 2] = ["冬时", "温蒂"];

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

/// Expand the standalone manufacture candidate pool with low-cost mechanically
/// important fillers that should not live in the hand-maintained whitelist.
pub fn expand_manufacture_candidate_pool(primary: &ManuPool, full: &ManuPool) -> ManuPool {
    let mut seen: HashSet<String> = primary.entries.iter().map(|e| e.name.clone()).collect();
    let mut entries = primary.entries.clone();

    for entry in &full.entries {
        if seen.contains(&entry.name) {
            continue;
        }
        if is_manufacture_candidate_extension(entry) {
            entries.push(entry.clone());
            seen.insert(entry.name.clone());
        }
    }

    entries.sort_by(|a, b| {
        b.flat_eff_hint
            .partial_cmp(&a.flat_eff_hint)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });

    ManuPool {
        entries,
        skipped: full.skipped.clone(),
    }
}

fn is_manufacture_candidate_extension(entry: &ManuPoolEntry) -> bool {
    entry.buff_ids.iter().any(|buff_id| {
        matches!(
            buff_id.as_str(),
            "manu_prod_spd[010]"
                | "manu_prod_spd_addition[030]"
                | "manu_prod_spd_addition[031]"
                | "manu_prod_spd_addition[040]"
                | "manu_prod_spd_addition[041]"
        )
    })
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
        let Some(skill) = table.get(bid) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::operbox::{default_operbox_gongsun_path, OperBox};
    use crate::skill_table::{default_skill_table_path, SkillTable};

    #[test]
    fn gongsun_operbox_manufacture_pool_ready() {
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = operbox.manufacture_roster(&instances);
        let pool = build_manufacture_pool(&roster, &instances, &table).unwrap();
        assert!(pool.stats(3).ready > 20);
        assert!(pool.entry("蛇屠箱").is_some());
    }

    #[test]
    fn full_e2_pool_contains_rosemary_anchor() {
        let operbox =
            OperBox::load(&crate::skill_table::data_path("fixtures/243/operbox_full_e2.json").unwrap())
                .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = operbox.manufacture_roster(&instances);
        let pool = build_manufacture_pool(&roster, &instances, &table).unwrap();
        assert!(
            pool.entry("迷迭香").is_some(),
            "skipped={:?}",
            pool.skipped
                .iter()
                .filter(|(name, _, _)| name == "迷迭香")
                .collect::<Vec<_>>()
        );
    }

    fn test_entry(name: &str, buff_ids: &[&str], flat_eff_hint: f64) -> ManuPoolEntry {
        ManuPoolEntry {
            name: name.to_string(),
            elite: 0,
            progress: crate::roster::OperatorProgress::elite_only(0),
            buff_ids: buff_ids.iter().map(|id| (*id).to_string()).collect(),
            tags: vec![],
            flat_eff_hint,
            has_l2_delegate: false,
            tier: OperatorTier::Standalone,
        }
    }

    #[test]
    fn manufacture_candidate_extension_adds_standard_beta_and_ramps() {
        let primary = ManuPool {
            entries: vec![
                test_entry("褐果", &["manu_prod_spd[000]"], 20.0),
                test_entry("雪猎", &["manu_prod_spd&limit&cost[101]"], 20.0),
                test_entry("卡达", &["manu_formula_cost[000]"], 0.0),
            ],
            skipped: vec![],
        };
        let full = ManuPool {
            entries: vec![
                primary.entries[0].clone(),
                primary.entries[1].clone(),
                primary.entries[2].clone(),
                test_entry("史都华德", &["manu_prod_spd[010]"], 25.0),
                test_entry("芬", &["manu_prod_spd_addition[030]"], 0.0),
                test_entry("克洛丝", &["manu_prod_spd_addition[040]"], 0.0),
                test_entry("低效非候选", &["manu_prod_spd[999]"], 5.0),
            ],
            skipped: vec![],
        };

        let expanded = expand_manufacture_candidate_pool(&primary, &full);
        assert!(expanded.entry("史都华德").is_some());
        assert!(expanded.entry("芬").is_some());
        assert!(expanded.entry("克洛丝").is_some());
        assert!(expanded.entry("低效非候选").is_none());
    }

    #[test]
    fn general_manufacture_search_pool_excludes_system_only_automation_ops() {
        let pool = ManuPool {
            entries: vec![
                test_entry("冬时", &["manu_prod_spd&manu[100]"], 30.0),
                test_entry("温蒂", &["manu_prod_spd&power[020]"], 45.0),
                test_entry("芬", &["manu_prod_spd_addition[030]"], 0.0),
                test_entry("克洛丝", &["manu_prod_spd_addition[040]"], 0.0),
            ],
            skipped: vec![],
        };

        let filtered = filter_general_manufacture_search_pool(&pool);
        assert!(pool.entry("冬时").is_some(), "体系路径仍可显式取冬时");
        assert!(pool.entry("温蒂").is_some(), "体系路径仍可显式取温蒂");
        assert!(filtered.entry("冬时").is_none());
        assert!(filtered.entry("温蒂").is_none());
        assert!(filtered.entry("芬").is_some());
        assert!(filtered.entry("克洛丝").is_some());
    }
}
