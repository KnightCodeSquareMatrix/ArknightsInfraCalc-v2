//! 心情消耗 / 宿舍回复 / 班次 ETA 建模。
//!
//! 数据源 `data/mood_model.json`（人工从 `MECHANICS_REGISTRY.csv` 整理，常量来自公孙长乐规范文档）。
//! 心情通常不影响生产速率；明确读取心情落差的技能（如铅踝）由设施求解器消费本模块的净消耗结果。
//!
//! 净消耗 `x = base − 满中枢减免 − 设施进驻减免 − 中枢回复 + 自身技能 + 同设施技能`，可工作时长 `= mood_cap / x`。

mod drain;
mod eta;
mod recovery;

pub use drain::{operator_net_drain, workable_hours, DrainInputs};
pub use eta::{shift_eta, shift_eta_with_instances, OperatorEta, ShiftEta};
pub use recovery::{dorm_recovery_rates, DormOccupant};

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::layout::FacilityKind;

/// 把引擎的 `FacilityKind` 映射到 mood_model.json 的设施键；无心情建模的设施返回 `None`。
pub fn facility_key(kind: FacilityKind) -> Option<&'static str> {
    match kind {
        FacilityKind::TradePost => Some("trade"),
        FacilityKind::Factory => Some("factory"),
        FacilityKind::PowerPlant => Some("power"),
        FacilityKind::Office => Some("office"),
        FacilityKind::ControlCenter => Some("control"),
        FacilityKind::MeetingRoom => Some("reception"),
        // 宿舍是回复侧；训练/加工暂不建模工作消耗。
        _ => None,
    }
}

/// 干员工作心情技能条目（消耗侧）。`drain_delta` 原样保留游戏符号（负=减免）。
#[derive(Debug, Clone, Deserialize)]
pub struct OperatorMoodSkill {
    pub name: String,
    pub facility: String,
    #[serde(default)]
    pub elite: u8,
    /// `self` = 只作用自身；`room` = 同设施全员；`room_others` = 同设施除自身。
    pub scope: String,
    #[serde(default)]
    pub drain_delta: f64,
    /// 仅在特定配方生效（如阿罗玛贵金属）；`None` = 全配方。
    #[serde(default)]
    pub recipe: Option<String>,
    #[serde(default)]
    pub skill: String,
}

/// 「消除自身心情消耗影响」类（槐琥/令）。
#[derive(Debug, Clone, Deserialize)]
pub struct MoodClearEntry {
    pub name: String,
    pub facility: String,
    #[serde(default)]
    pub elite: u8,
    /// `room` = 清同设施全员自身技能；`sui_only` = 仅岁干员（需派系数据，暂不建模）；`self` = 仅自身。
    pub scope: String,
}

/// 中枢全局回复（玛恩纳/维什戴尔/重岳）：取最高不叠加，`covers` 列出受益设施键。
#[derive(Debug, Clone, Deserialize)]
pub struct ControlGlobalRecovery {
    pub name: String,
    #[serde(default)]
    pub elite: u8,
    pub value: f64,
    #[serde(default)]
    pub covers: Vec<String>,
    /// 是否把中枢内笑脸类技能的总回复扩散到生产/功能设施（玛恩纳）。
    #[serde(default)]
    pub spread_smiley: bool,
    /// 指定干员同在中枢时额外增加的全局恢复值（维什戴尔 + 魔王）。
    #[serde(default)]
    pub bonus_when_control_contains: Option<String>,
    #[serde(default)]
    pub bonus_value: f64,
}

/// 中枢候选补位使用的全局回复摘要。它不参与实际心情结算，实际取最高仍由
/// `drain` 读取相同的 `control_global_recovery` 数据完成。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ControlGlobalRecoverySummary {
    pub effective_value: f64,
    pub effective_owner_count: usize,
    pub redundant_owner_count: usize,
    pub faction_recovery_value: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ControlSmileyProvider {
    pub name: String,
    #[serde(default)]
    pub elite: u8,
}

/// 中枢笑脸类回复：每名在岗提供者贡献固定值，可叠加。
#[derive(Debug, Clone, Deserialize)]
pub struct ControlSmileyRecovery {
    pub per_provider: f64,
    #[serde(default)]
    pub providers: Vec<ControlSmileyProvider>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ControlFactionRecovery {
    pub id: String,
    pub providers: Vec<String>,
    pub members: Vec<String>,
    #[serde(default = "default_faction_recovery_per_member")]
    pub per_member: f64,
}

fn default_faction_recovery_per_member() -> f64 {
    0.05
}

/// 宿舍回复技能。`kind` ∈ {self, single, group, pool}。
#[derive(Debug, Clone, Deserialize)]
pub struct DormRecoverySkill {
    pub name: String,
    #[serde(default)]
    pub elite: u8,
    pub kind: String,
    #[serde(default)]
    pub value: f64,
    /// 该技能在提供主效果的同时，额外给「同宿全员」的群回值（风笛/解脱等）。
    #[serde(default)]
    pub also_group: Option<f64>,
    /// 技能对自身的额外心情恢复（活泼/慵懒等）。
    #[serde(default)]
    pub self_delta: Option<f64>,
    /// 菲亚梅塔自律：自身固定回复且不吃任何外部来源。
    #[serde(default)]
    pub no_external: bool,
    #[serde(default)]
    pub skill: String,
}

impl DormRecoverySkill {
    pub fn is_self(&self) -> bool {
        self.kind == "self"
    }
    pub fn is_single(&self) -> bool {
        self.kind == "single"
    }
    pub fn is_group(&self) -> bool {
        self.kind == "group"
    }
    pub fn is_pool(&self) -> bool {
        self.kind == "pool"
    }
    /// 该干员是否为「宿管」（提供群回/单回，倾向最后回满）。
    pub fn is_manager(&self) -> bool {
        self.is_single() || self.is_group() || self.is_pool()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoodModel {
    pub mood_cap: f64,
    pub base_drain_per_hour: f64,
    pub full_control_reduction: f64,
    #[serde(default)]
    pub rest_threshold: f64,
    /// 设施键 → 当前进驻人数 → 减免值（仅 trade/factory）。
    pub facility_occupancy_reduction: HashMap<String, HashMap<String, f64>>,
    /// 宿舍等级字符串 → 基线回复（白字+满氛围绿字，干员技能之前）。
    pub dorm_recovery_by_level: HashMap<String, f64>,
    #[serde(default)]
    pub mood_clear_self: Vec<MoodClearEntry>,
    #[serde(default)]
    pub operator_mood_skills: Vec<OperatorMoodSkill>,
    #[serde(default)]
    pub control_global_recovery: Vec<ControlGlobalRecovery>,
    #[serde(default)]
    pub control_smiley_recovery: Option<ControlSmileyRecovery>,
    #[serde(default)]
    pub control_faction_recovery: Vec<ControlFactionRecovery>,
    #[serde(default)]
    pub dorm_recovery_skills: Vec<DormRecoverySkill>,
}

impl MoodModel {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| Error::msg(format!("mood_model read {}: {e}", path.display())))?;
        Self::from_json(&raw)
    }

    pub fn from_json(raw: &str) -> Result<Self> {
        let model: MoodModel =
            serde_json::from_str(raw).map_err(|e| Error::msg(format!("mood_model parse: {e}")))?;
        model.validate()?;
        Ok(model)
    }

    pub fn load_default() -> Result<Self> {
        Self::load(&crate::skill_table::data_path("mood_model.json")?)
    }

    fn validate(&self) -> Result<()> {
        if self.mood_cap <= 0.0 {
            return Err(Error::msg("mood_model.mood_cap must be > 0"));
        }
        if self.base_drain_per_hour <= 0.0 {
            return Err(Error::msg("mood_model.base_drain_per_hour must be > 0"));
        }
        if !(0.0..self.mood_cap).contains(&self.rest_threshold) {
            return Err(Error::msg(
                "mood_model.rest_threshold must be >= 0 and < mood_cap",
            ));
        }
        Ok(())
    }

    /// 从满心情工作到休息阈值之间可消耗的心情值。
    pub fn workable_mood(&self) -> f64 {
        self.mood_cap - self.rest_threshold
    }

    pub fn control_global_recovery_summary(
        &self,
        control_operators: &[(String, u8)],
    ) -> ControlGlobalRecoverySummary {
        let active: Vec<_> = self
            .control_global_recovery
            .iter()
            .filter(|entry| {
                control_operators
                    .iter()
                    .any(|(name, elite)| name == &entry.name && *elite >= entry.elite)
            })
            .collect();
        let effective_value = active
            .iter()
            .map(|entry| control_global_recovery_value(entry, control_operators))
            .fold(0.0_f64, f64::max);
        let effective_owner_count = active
            .iter()
            .filter(|entry| {
                (control_global_recovery_value(entry, control_operators) - effective_value).abs()
                    < f64::EPSILON
            })
            .count();
        ControlGlobalRecoverySummary {
            effective_value,
            effective_owner_count,
            // Equal maximum sources settle as tied winners, but represent
            // duplicate control filler ownership for the same recovery fact.
            redundant_owner_count: active.len().saturating_sub(1),
            faction_recovery_value: self.control_faction_recovery_value(control_operators),
        }
    }

    pub fn control_faction_recovery_value(&self, control_operators: &[(String, u8)]) -> f64 {
        self.control_faction_recovery
            .iter()
            .map(|entry| {
                let providers = entry
                    .providers
                    .iter()
                    .filter(|name| control_operators.iter().any(|(active, _)| active == *name))
                    .count();
                let members = entry
                    .members
                    .iter()
                    .filter(|name| control_operators.iter().any(|(active, _)| active == *name))
                    .count();
                providers as f64 * members as f64 * entry.per_member
            })
            .sum()
    }

    pub fn control_smiley_recovery_value(&self, control_operators: &[(String, u8)]) -> f64 {
        self.control_smiley_recovery_for_facility(FacilityKind::ControlCenter, control_operators)
    }

    pub fn control_smiley_recovery_for_facility(
        &self,
        facility: FacilityKind,
        control_operators: &[(String, u8)],
    ) -> f64 {
        let Some(smiley) = &self.control_smiley_recovery else {
            return 0.0;
        };
        let provider_count = smiley
            .providers
            .iter()
            .filter(|provider| {
                control_operators
                    .iter()
                    .any(|(name, elite)| name == &provider.name && *elite >= provider.elite)
            })
            .count();
        if provider_count == 0 {
            return 0.0;
        }
        let total = smiley.per_provider * provider_count as f64;
        if facility == FacilityKind::ControlCenter {
            return total;
        }
        let spread = self.control_global_recovery.iter().any(|entry| {
            entry.spread_smiley
                && control_operators
                    .iter()
                    .any(|(name, elite)| name == &entry.name && *elite >= entry.elite)
        });
        if spread {
            total
        } else {
            0.0
        }
    }

    /// 当前进驻人数减免（仅 trade/factory；1/2/3 人对应 0/0.05/0.1）。
    pub fn occupancy_reduction(&self, facility: FacilityKind, occupancy: usize) -> f64 {
        let Some(key) = facility_key(facility) else {
            return 0.0;
        };
        self.facility_occupancy_reduction
            .get(key)
            .and_then(|by_occupancy| by_occupancy.get(&occupancy.min(3).to_string()))
            .copied()
            .unwrap_or(0.0)
    }

    /// 宿舍基线回复（按等级查表；越界取最近端点）。
    pub fn dorm_base_recovery(&self, level: u8) -> f64 {
        if let Some(v) = self.dorm_recovery_by_level.get(&level.to_string()) {
            return *v;
        }
        // 越界：取表中最大等级或最小等级的值。
        let mut entries: Vec<(u8, f64)> = self
            .dorm_recovery_by_level
            .iter()
            .filter_map(|(k, v)| k.parse::<u8>().ok().map(|lv| (lv, *v)))
            .collect();
        entries.sort_by_key(|(lv, _)| *lv);
        match entries.first() {
            None => 0.0,
            Some(&(min_lv, min_v)) => {
                if level <= min_lv {
                    min_v
                } else {
                    entries.last().map(|(_, v)| *v).unwrap_or(min_v)
                }
            }
        }
    }

    /// 干员在某设施的自身消耗技能条目（scope=self/room_others 的自身项由调用方区分）。
    /// 选取策略：匹配 name+facility 且 `elite ≤ op_elite` 中精英最高者；`active_skill` 可精确指定技能名。
    pub fn operator_self_skill(
        &self,
        name: &str,
        facility: FacilityKind,
        op_elite: u8,
        active_skill: Option<&str>,
    ) -> Option<&OperatorMoodSkill> {
        let key = facility_key(facility)?;
        self.operator_mood_skills
            .iter()
            .filter(|s| s.name == name && s.facility == key && s.elite <= op_elite)
            .filter(|s| s.scope == "self")
            .filter(|s| active_skill.is_none_or(|want| s.skill == want))
            .max_by_key(|s| s.elite)
    }

    /// 干员在某设施提供给「同设施全员」的消耗修正（黍/火哨/巫恋）。
    pub fn operator_room_skill(
        &self,
        name: &str,
        facility: FacilityKind,
        op_elite: u8,
    ) -> Option<&OperatorMoodSkill> {
        let key = facility_key(facility)?;
        self.operator_mood_skills
            .iter()
            .filter(|s| s.name == name && s.facility == key && s.elite <= op_elite)
            .filter(|s| s.scope == "room" || s.scope == "room_others")
            .max_by_key(|s| s.elite)
    }

    /// 干员在宿舍同时生效的回复技能。
    ///
    /// 每种 `kind` 独立选取 `elite ≤ op_elite` 的最高阶段；同阶段重复项取数值较高者。
    /// 这样能同时保留波登可的群回与单回，同时让杜林的「嗜睡」覆盖「慵懒」。
    pub fn dorm_skills(&self, name: &str, op_elite: u8) -> Vec<&DormRecoverySkill> {
        let mut best: HashMap<&str, &DormRecoverySkill> = HashMap::new();
        for skill in self
            .dorm_recovery_skills
            .iter()
            .filter(|s| s.name == name && s.elite <= op_elite)
        {
            let replace = best
                .get(skill.kind.as_str())
                .is_none_or(|current| (skill.elite, skill.value) > (current.elite, current.value));
            if replace {
                best.insert(skill.kind.as_str(), skill);
            }
        }
        let mut skills: Vec<_> = best.into_values().collect();
        skills.sort_by(|a, b| a.kind.cmp(&b.kind));
        skills
    }
}

pub(crate) fn control_global_recovery_value(
    entry: &ControlGlobalRecovery,
    control_operators: &[(String, u8)],
) -> f64 {
    let bonus_active = entry
        .bonus_when_control_contains
        .as_ref()
        .is_some_and(|name| {
            control_operators
                .iter()
                .any(|(operator, _)| operator == name)
        });
    entry.value + if bonus_active { entry.bonus_value } else { 0.0 }
}

pub fn default_mood_model() -> Result<&'static MoodModel> {
    static MODEL: OnceLock<std::result::Result<MoodModel, String>> = OnceLock::new();
    match MODEL.get_or_init(|| MoodModel::load_default().map_err(|error| error.to_string())) {
        Ok(model) => Ok(model),
        Err(error) => Err(Error::msg(error.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> MoodModel {
        MoodModel::load_default().expect("load bundled mood_model.json")
    }

    #[test]
    fn loads_bundled_model() {
        let m = model();
        assert_eq!(m.mood_cap, 24.0);
        assert_eq!(m.base_drain_per_hour, 1.0);
        assert_eq!(m.full_control_reduction, 0.25);
    }

    #[test]
    fn occupancy_reduction_matches_doc() {
        let m = model();
        assert_eq!(m.occupancy_reduction(FacilityKind::Factory, 3), 0.1);
        assert_eq!(m.occupancy_reduction(FacilityKind::TradePost, 2), 0.05);
        assert_eq!(m.occupancy_reduction(FacilityKind::TradePost, 1), 0.0);
        // 发电/办公不按进驻人数减免。
        assert_eq!(m.occupancy_reduction(FacilityKind::PowerPlant, 3), 0.0);
        assert_eq!(m.occupancy_reduction(FacilityKind::Office, 3), 0.0);
    }

    #[test]
    fn wisadel_global_recovery_summary_includes_civilight_bonus() {
        let m = model();
        let ew_only = vec![("维什戴尔".to_string(), 2u8)];
        let boosted = vec![("维什戴尔".to_string(), 2u8), ("魔王".to_string(), 2u8)];
        assert_eq!(
            m.control_global_recovery_summary(&ew_only).effective_value,
            0.1
        );
        assert_eq!(
            m.control_global_recovery_summary(&boosted).effective_value,
            0.2
        );
    }

    #[test]
    fn faction_recovery_summary_uses_provider_times_member_count() {
        let m = model();
        let control = vec![
            ("灰烬".to_string(), 2),
            ("战车".to_string(), 2),
            ("闪击".to_string(), 2),
            ("霜华".to_string(), 2),
        ];
        assert_eq!(
            m.control_global_recovery_summary(&control)
                .faction_recovery_value,
            0.8
        );
    }

    #[test]
    fn dorm_base_recovery_table() {
        let m = model();
        assert_eq!(m.dorm_base_recovery(1), 2.0);
        assert_eq!(m.dorm_base_recovery(5), 4.0);
        // 越界取端点。
        assert_eq!(m.dorm_base_recovery(0), 2.0);
        assert_eq!(m.dorm_base_recovery(9), 4.0);
    }

    #[test]
    fn operator_skill_lookup() {
        let m = model();
        // 泡泡三级制造仓储技能 -0.25（self）。
        let s = m
            .operator_self_skill("泡泡", FacilityKind::Factory, 0, None)
            .expect("泡泡 factory skill");
        assert_eq!(s.drain_delta, -0.25);
        // 火哨给贸易全员 -0.1（room）。
        let r = m
            .operator_room_skill("火哨", FacilityKind::TradePost, 0)
            .expect("火哨 room skill");
        assert_eq!(r.drain_delta, -0.1);
    }

    #[test]
    fn dorm_lookup_keeps_different_skill_kinds() {
        let m = model();
        let skills = m.dorm_skills("波登可", 1);
        assert!(skills.iter().any(|s| s.kind == "group" && s.value == 0.15));
        assert!(skills.iter().any(|s| s.kind == "single" && s.value == 0.65));
    }
}
