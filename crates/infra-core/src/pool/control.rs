use std::collections::HashSet;

use crate::control::ControlOperator;
use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::roster::{OperatorProgress, Roster};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

use crate::layout::tier::OperatorTier;
use crate::operbox::OperBox;

use super::base::{build_roster_pool, filter_pool, HasName, HasProgress, PoolCore, TierTagged};
pub use super::trade::PoolSkip;

#[derive(Debug, Clone)]
pub struct ControlPoolEntry {
    pub name: String,
    pub elite: u8,
    pub progress: OperatorProgress,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    pub tier: OperatorTier,
}

impl HasName for ControlPoolEntry {
    fn pool_name(&self) -> &str {
        &self.name
    }
}

impl HasProgress for ControlPoolEntry {
    fn progress(&self) -> OperatorProgress {
        self.progress
    }
}

impl TierTagged for ControlPoolEntry {
    fn tier(&self) -> OperatorTier {
        self.tier
    }

    fn set_tier(&mut self, tier: OperatorTier) {
        self.tier = tier;
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

/// Build the modeled control pool plus genuine skill-less identities. A skipped
/// unmodeled capability must not return with its buffs silently erased.
pub fn build_control_pool_with_fillers(
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<ControlPool> {
    let mut pool = build_control_pool(&operbox.control_roster(instances), instances, table)?;
    let existing: HashSet<String> = pool
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .chain(pool.skipped.iter().map(|(name, _, _)| name.clone()))
        .collect();
    let roster = operbox.roster_for_facility(crate::layout::FacilityKind::ControlCenter);
    let mut filler_names: Vec<_> = roster
        .names()
        .filter(|name| !existing.contains(*name))
        .cloned()
        .collect();
    filler_names.sort();
    for name in filler_names {
        let Some(progress) = roster.progress(&name) else {
            continue;
        };
        match try_entry(&name, progress, instances, table) {
            Ok(entry) => pool.entries.push(entry),
            Err(reason) => pool.skipped.push((name, progress.elite, reason)),
        }
    }
    Ok(pool)
}

pub fn filter_control_pool(pool: &ControlPool, exclude: &HashSet<String>) -> ControlPool {
    filter_pool(pool, exclude)
}

/// Initial single-operator ETA admission. Called while building the roster,
/// before search; fixed Plan members have already passed Plan admission.
pub(crate) fn admit_control_roster(
    pool: &mut ControlPool,
    work_hours: Option<f64>,
    initial_mood: f64,
    protected: &HashSet<String>,
) -> Result<()> {
    let Some(hours) = work_hours else {
        return Ok(());
    };
    let model = crate::mood::default_mood_model()?;
    pool.entries.retain(|entry| {
        protected.contains(&entry.name)
            || crate::mood::control_roster_fits(
                model,
                &entry.name,
                entry.elite,
                initial_mood,
                hours,
            )
    });
    Ok(())
}

/// 自动轮换在建池前从 roster 删除依赖指定生产站消费方的中枢角色。
///
/// 这些 deferred producer 需要主动寻找跨设施搭档，属于自动轮换明确禁用的
/// 跨站组合。按干员身份删除所有练度档，避免低档从 filler 路径重新进入中枢。
pub(crate) fn auto_rotation_control_roster(
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<OperBox> {
    let catalog = crate::response_dependency::producer_rule_catalog(table)?;
    let mut forbidden_names = HashSet::new();
    // 低练度档没有 producer buff，但自动轮换按干员身份禁用该体系角色；否则同一
    // 干员会从 filler 路径重新进入中枢。用完整实例表补齐账号当前档位未暴露的来源。
    for (_, instance) in instances.iter() {
        let name = &instance.name;
        for tier in [PromotionTier::Tier0, PromotionTier::TierUp] {
            let buff_ids = instances.resolve_control_buff_ids(name, tier);
            if buff_ids.iter().any(|buff_id| {
                catalog.rules.iter().any(|rule| {
                    rule.source_buff_id == *buff_id
                        && rule.admission
                            == crate::response_dependency::ProducerAdmission::DeferredOptional
                })
            }) {
                forbidden_names.insert(name.clone());
            }
        }
    }
    Ok(operbox.excluding(&forbidden_names))
}

fn try_entry(
    name: &str,
    progress: crate::roster::OperatorProgress,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> std::result::Result<ControlPoolEntry, PoolSkip> {
    let tier = PromotionTier::from_progress(progress);
    let buff_ids = instances.resolve_control_buff_ids(name, tier);
    for bid in &buff_ids {
        let Some(skill) = table.get(bid) else {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        };
        if skill.facility != "control" {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        }
    }

    let tags = instances.tags_for(name, tier);

    Ok(ControlPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids,
        tags,
        tier: OperatorTier::Standalone,
    })
}
