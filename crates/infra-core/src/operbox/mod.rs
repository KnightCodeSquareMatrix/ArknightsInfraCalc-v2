mod xlsx;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::TAG_DURIN;
use crate::roster::{OperatorProgress, Roster};
use crate::tier::PromotionTier;

pub use xlsx::from_xlsx_path;

#[derive(Debug, Clone, Deserialize)]
pub struct OperBoxEntry {
    pub id: String,
    pub name: String,
    pub elite: u8,
    #[serde(default)]
    pub level: u32,
    pub own: bool,
    #[serde(default)]
    pub potential: u8,
    #[serde(default)]
    pub rarity: u8,
}

/// OperBox: instance name -> progress (matches `operator_instances.json` `name` field).
#[derive(Debug, Clone)]
pub struct OperBox {
    pub entries: Vec<OperBoxEntry>,
    pub owned: HashMap<String, OperatorProgress>,
}

impl OperBox {
    /// 加载练度盒：`.json` 或一图流导出的 `.xlsx` / `.xls`。
    pub fn load(path: &Path) -> Result<Self> {
        if is_xlsx_path(path) {
            return from_xlsx_path(path);
        }
        let json = std::fs::read_to_string(path).map_err(|e| {
            Error::msg(format!("operbox read {}: {e}", path.display()))
        })?;
        Self::from_json(&json).map_err(|error| {
            Error::msg(format!("operbox parse {}: {error}", path.display()))
        })
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let mut json = json.to_string();
        if json.starts_with('\u{feff}') {
            json = json.trim_start_matches('\u{feff}').to_string();
        }
        let entries: Vec<OperBoxEntry> = serde_json::from_str(&json)?;
        Ok(Self::from_entries(entries))
    }

    pub fn from_entries(entries: Vec<OperBoxEntry>) -> Self {
        let mut owned = HashMap::new();
        for e in &entries {
            if !e.own {
                continue;
            }
            let progress = OperatorProgress::new(e.elite, e.level, e.rarity);
            owned
                .entry(e.name.clone())
                .and_modify(|p: &mut OperatorProgress| {
                    if e.elite > p.elite {
                        *p = progress;
                    } else if e.elite == p.elite {
                        p.level = p.level.max(e.level);
                        p.rarity = p.rarity.max(e.rarity);
                    }
                })
                .or_insert(progress);
        }
        Self { entries, owned }
    }

    pub fn owns(&self, name: &str) -> bool {
        self.owned.contains_key(name.trim())
    }

    pub fn elite_of(&self, name: &str) -> Option<u8> {
        self.owned.get(name.trim()).map(|p| p.elite)
    }

    pub fn progress_of(&self, name: &str) -> Option<OperatorProgress> {
        self.owned.get(name.trim()).copied()
    }

    pub fn tier_of(&self, name: &str) -> Option<PromotionTier> {
        self.progress_of(name)
            .map(PromotionTier::from_progress)
    }

    pub fn owned_count(&self) -> usize {
        self.owned.len()
    }

    pub fn roster(&self) -> Roster {
        Roster::from_progress_map(self.owned.clone())
    }

    pub fn trade_roster(&self, instances: &OperatorInstances) -> Roster {
        Self::facility_roster(self, instances, "trade")
    }

    pub fn manufacture_roster(&self, instances: &OperatorInstances) -> Roster {
        Self::facility_roster(self, instances, "manufacture")
    }

    pub fn power_roster(&self, instances: &OperatorInstances) -> Roster {
        Self::facility_roster(self, instances, "power")
    }

    pub fn control_roster(&self, instances: &OperatorInstances) -> Roster {
        Self::facility_roster(self, instances, "control")
    }

    fn facility_roster(
        &self,
        instances: &OperatorInstances,
        facility: &str,
    ) -> Roster {
        let mut roster = Roster::default();
        for (name, progress) in &self.owned {
            let tier = PromotionTier::from_progress(*progress);
            if instances
                .get(name, tier)
                .is_some_and(|i| i.facilities.contains_key(facility))
            {
                roster.insert(name.clone(), *progress);
            }
        }
        roster
    }

    pub fn roster_excluding(&self, excluded: &HashSet<String>) -> Roster {
        let by_name = self
            .owned
            .iter()
            .filter(|(name, _)| !excluded.contains(*name))
            .map(|(name, progress)| (name.clone(), *progress))
            .collect();
        Roster::from_progress_map(by_name)
    }

    /// 排除指定干员后的练度盒（用于相邻班池修剪）。
    pub fn excluding(&self, excluded: &HashSet<String>) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|e| !excluded.contains(&e.name))
                .cloned()
                .collect(),
            owned: self.owned_subset(excluded),
        }
    }

    pub fn owned_subset(&self, excluded: &HashSet<String>) -> HashMap<String, OperatorProgress> {
        self.owned
            .iter()
            .filter(|(k, _)| !excluded.contains(*k))
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// 规划假设：box 内杜林族干员视同进驻宿舍计入基建，供鸿雪「际崖居民」等消费（cap 4）。
    ///
    /// 与编制落位无关；`resolve_base(..., durin_dorm_planning: Some(...))` 与编制内计数取较大值。
    pub fn durin_dorm_planning_count(&self, instances: &OperatorInstances) -> u8 {
        const CAP: u8 = 4;
        let n = self
            .owned
            .keys()
            .filter(|name| owned_operator_has_tag(self, instances, name, TAG_DURIN))
            .count();
        (n as u8).min(CAP)
    }
}

fn is_xlsx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("xlsx") || ext.eq_ignore_ascii_case("xls")
        })
}

fn owned_operator_has_tag(
    operbox: &OperBox,
    instances: &OperatorInstances,
    name: &str,
    tag: &str,
) -> bool {
    if !operbox.owns(name) {
        return false;
    }
    for tier in [PromotionTier::Tier0, PromotionTier::TierUp] {
        if let Some(inst) = instances.get(name, tier) {
            if inst.tags.iter().any(|t| t == tag) {
                return true;
            }
        }
    }
    false
}

pub fn default_operbox_gongsun_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("operbox_gongsun.json")
}

/// 243 标准测试样例：全精2 operbox（`data/fixtures/243/operbox_full_e2.json`）。
pub fn default_operbox_full_e2_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("fixtures/243/operbox_full_e2.json")
}

/// 243 标准布局（`data/fixtures/243/layout.json`）。
pub fn default_layout_243_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("fixtures/243/layout.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_gongsun_operbox() {
        let path = default_operbox_gongsun_path().unwrap();
        let box_ = OperBox::load(&path).unwrap();
        assert!(box_.owned_count() > 50);
        assert!(box_.owns("巫恋"));
    }

    #[test]
    fn roster_excluding_removes_workers() {
        let path = default_operbox_gongsun_path().unwrap();
        let box_ = OperBox::load(&path).unwrap();
        let mut ex = HashSet::new();
        ex.insert("巫恋".to_string());
        ex.insert("龙舌兰".to_string());
        let r = box_.roster_excluding(&ex);
        assert!(!r.elite("巫恋").is_some());
        assert!(r.elite("能天使").is_some());
    }

    #[test]
    fn trade_roster_filters_to_trade_bindings() {
        let path = default_operbox_gongsun_path().unwrap();
        let box_ = OperBox::load(&path).unwrap();
        let instances = OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let trade = box_.trade_roster(&instances);
        assert!(trade.len() < box_.owned_count());
        assert!(trade.elite("巫恋").is_some());
        assert!(trade.elite("凯尔希").is_none());
    }

    #[test]
    fn durin_dorm_planning_count_caps_at_four() {
        let path = default_operbox_gongsun_path().unwrap();
        let operbox = OperBox::load(&path).unwrap();
        let instances = OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let n = operbox.durin_dorm_planning_count(&instances);
        assert!(n >= 4, "gongsun box should include multiple durin operators");
        assert!(n <= 4);
    }

    #[test]
    fn four_star_e1_enters_manufacture_pool_at_tier_up() {
        let entries = vec![OperBoxEntry {
            id: "test".into(),
            name: "清流".into(),
            elite: 1,
            level: 1,
            own: true,
            potential: 1,
            rarity: 4,
        }];
        let operbox = OperBox::from_entries(entries);
        let instances = OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let table = crate::skill_table::SkillTable::load(
            &crate::skill_table::default_skill_table_path().unwrap(),
        )
        .unwrap();
        let roster = operbox.manufacture_roster(&instances);
        let pool = crate::pool::build_manufacture_pool(&roster, &instances, &table).unwrap();
        let q = pool
            .entry("清流")
            .expect("清流 4★e1 should use tier_up manufacture");
        assert!(q
            .buff_ids
            .iter()
            .any(|id| id.contains("manu_prod_spd&trade")));
    }
}
