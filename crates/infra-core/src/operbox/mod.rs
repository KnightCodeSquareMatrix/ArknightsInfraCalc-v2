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
    /// 自动排班在搜索前冻结的按设施初始 ETA 资格；不删除其他岗位资格。
    facility_eta_exclusions: HashMap<String, HashSet<String>>,
}

impl OperBox {
    /// 加载练度盒：`.json` 或一图流导出的 `.xlsx` / `.xls`。
    pub fn load(path: &Path) -> Result<Self> {
        if is_xlsx_path(path) {
            return from_xlsx_path(path);
        }
        let json = std::fs::read_to_string(path)
            .map_err(|e| Error::msg(format!("operbox read {}: {e}", path.display())))?;
        Self::from_json(&json)
            .map_err(|error| Error::msg(format!("operbox parse {}: {error}", path.display())))
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let mut json = json.to_string();
        if json.starts_with('\u{feff}') {
            json = json.trim_start_matches('\u{feff}').to_string();
        }
        let entries: Vec<OperBoxEntry> = serde_json::from_str(&json)?;
        Self::from_entries(entries)
    }

    pub fn from_entries(entries: Vec<OperBoxEntry>) -> Result<Self> {
        Self::from_entries_inner(entries, false)
    }

    /// Loads historical `plan.compute` snapshots whose producer used rarity=1
    /// as an unknown-rarity sentinel while preserving elite/level facts.
    /// Runtime contract validation remains strict; this is only for local
    /// debug-bundle replay.
    pub fn from_entries_for_plan_compute_compat(entries: Vec<OperBoxEntry>) -> Result<Self> {
        Self::from_entries_inner(entries, true)
    }

    fn from_entries_inner(
        entries: Vec<OperBoxEntry>,
        allow_unreachable_progress: bool,
    ) -> Result<Self> {
        let mut owned = HashMap::new();
        for e in &entries {
            validate_entry_progress(e, allow_unreachable_progress)?;
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
        Ok(Self {
            entries,
            owned,
            facility_eta_exclusions: HashMap::new(),
        })
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
        self.progress_of(name).map(PromotionTier::from_progress)
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

    fn facility_roster(&self, instances: &OperatorInstances, facility: &str) -> Roster {
        let eta_key = if facility == "manufacture" {
            "factory"
        } else {
            facility
        };
        let mut roster = Roster::default();
        for (name, progress) in &self.owned {
            if self
                .facility_eta_exclusions
                .get(eta_key)
                .is_some_and(|names| names.contains(name))
            {
                continue;
            }
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

    pub(crate) fn admits_facility(
        &self,
        name: &str,
        facility: crate::layout::FacilityKind,
    ) -> bool {
        crate::mood::facility_key(facility).is_none_or(|key| {
            self.facility_eta_exclusions
                .get(key)
                .is_none_or(|names| !names.contains(name))
        })
    }

    pub(crate) fn roster_for_facility(&self, facility: crate::layout::FacilityKind) -> Roster {
        Roster::from_progress_map(
            self.owned
                .iter()
                .filter(|(name, _)| self.admits_facility(name, facility))
                .map(|(name, progress)| (name.clone(), *progress))
                .collect(),
        )
    }

    /// 初始单人试算 policy：满中枢基础减耗、计划满房人数、自身技能；不借未来同房/中枢技能减免。
    pub(crate) fn with_auto_roster_eta(
        &self,
        blueprint: &crate::layout::BaseBlueprint,
        instances: &OperatorInstances,
        hours: f64,
    ) -> Result<Self> {
        let model = crate::mood::default_mood_model()?;
        let mut result = self.clone();
        result.facility_eta_exclusions.clear();
        for room in &blueprint.rooms {
            let Some(key) = crate::mood::facility_key(room.kind) else {
                continue;
            };
            if room.kind == crate::layout::FacilityKind::ControlCenter {
                continue;
            }
            for (name, progress) in &self.owned {
                if !crate::mood::auto_roster_room_fits(
                    model, blueprint, room, instances, name, *progress, hours,
                ) {
                    result
                        .facility_eta_exclusions
                        .entry(key.to_string())
                        .or_default()
                        .insert(name.clone());
                }
            }
        }
        Ok(result)
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
            facility_eta_exclusions: self.facility_eta_exclusions.clone(),
        }
    }

    /// 仅保留指定干员的练度盒（用于按队伍名册过滤中枢池）。
    pub fn keeping_only(&self, names: &HashSet<String>) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|e| names.contains(&e.name))
                .cloned()
                .collect(),
            owned: self
                .owned
                .iter()
                .filter(|(k, _)| names.contains(*k))
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            facility_eta_exclusions: self.facility_eta_exclusions.clone(),
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

fn validate_entry_progress(entry: &OperBoxEntry, allow_unreachable_progress: bool) -> Result<()> {
    if !(1..=6).contains(&entry.rarity) {
        return Err(Error::msg(format!(
            "operbox entry {} has invalid rarity {}; expected 1..=6",
            entry.name, entry.rarity
        )));
    }

    let max_elite = match entry.rarity {
        1 | 2 => 0,
        3 => 1,
        4..=6 => 2,
        _ => unreachable!(),
    };
    if !allow_unreachable_progress && entry.elite > max_elite {
        return Err(Error::msg(format!(
            "operbox entry {} has impossible progress: rarity {} cannot reach elite {}",
            entry.name, entry.rarity, entry.elite
        )));
    }
    if !entry.own {
        return Ok(());
    }

    if allow_unreachable_progress {
        if !(1..=90).contains(&entry.level) {
            return Err(Error::msg(format!(
                "operbox entry {} has invalid level {} for plan.compute compatibility",
                entry.name, entry.level
            )));
        }
        return Ok(());
    }

    let max_level = match (entry.rarity, entry.elite) {
        (1 | 2, 0) => 30,
        (3, 0) => 40,
        (3, 1) => 55,
        (4, 0) => 45,
        (4, 1) => 60,
        (4, 2) => 70,
        (5, 0) => 50,
        (5, 1) => 70,
        (5, 2) => 80,
        (6, 0) => 50,
        (6, 1) => 80,
        (6, 2) => 90,
        _ => unreachable!(),
    };
    if !(1..=max_level).contains(&entry.level) {
        return Err(Error::msg(format!(
            "operbox entry {} has invalid level {} for rarity {} elite {}; expected 1..={max_level}",
            entry.name, entry.level, entry.rarity, entry.elite
        )));
    }
    Ok(())
}

fn is_xlsx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx") || ext.eq_ignore_ascii_case("xls"))
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

/// 243 标准测试样例：各星级最高合法练度（`data/fixtures/243/operbox_full_e2.json`）。
pub fn default_operbox_full_e2_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("fixtures/243/operbox_full_e2.json")
}

/// 243 标准布局（`data/fixtures/243/layout.json`）。
pub fn default_layout_243_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("fixtures/243/layout.json")
}
