//! 单个在岗干员的净心情消耗 `x`（每小时），及可工作时长 `mood_cap / x`。
//!
//! 组装顺序（来自公孙长乐规范文档）：
//! ```text
//! x = base_drain
//!   − full_control_reduction        （中枢满 5 人）
//!   − facility_occupancy_reduction   （仅贸易/制造按当前进驻人数）
//!   − control_global_recovery        （玛恩纳/维什戴尔/重岳，取最高，按 covers 命中设施）
//!   − control_smiley_recovery        （笑脸类每名 +0.05；玛恩纳可扩散到全局）
//!   + self_skill_delta               （自身技能，受同房「消除自身消耗」类抵消）
//!   + room_skill_delta               （同设施他人提供的 room/room_others 修正，可叠加）
//! ```
//! `x` 下限为 0；`x ≤ 0` 表示在岗即回复（ETA 无限）。可用心情为
//! `mood_cap - rest_threshold`。

use crate::layout::FacilityKind;

use super::MoodModel;

/// 计算净消耗所需的房间上下文。所有名字用干员本名（与 assignment 一致）。
#[derive(Clone, Copy)]
pub struct DrainInputs<'a> {
    pub name: &'a str,
    pub elite: u8,
    pub facility: FacilityKind,
    /// 同设施全部在岗干员 `(name, elite)`（含自身）。
    pub room_ops: &'a [(String, u8)],
    /// 中枢是否满 5 人（全局 −0.25 减免生效）。
    pub full_control: bool,
    /// 中枢在岗干员 `(name, elite)`（用于判定玛恩纳等全局回复是否在场）。
    pub control_ops: &'a [(String, u8)],
    /// 当前配方键（"gold"/"battle_record"/"originium"）；用于 recipe 门限技能。`None` = 未知，按不生效处理。
    pub recipe: Option<&'a str>,
}

/// 该房间是否有「消除自身心情消耗影响」的干员在场（槐琥·团队精神，制造站）。
fn room_clears_self(model: &MoodModel, inputs: &DrainInputs<'_>, sui_member: bool) -> bool {
    let Some(key) = super::facility_key(inputs.facility) else {
        return false;
    };
    model.mood_clear_self.iter().any(|c| {
        c.facility == key
            && (c.scope == "room" || (c.scope == "sui_only" && sui_member))
            && inputs
                .room_ops
                .iter()
                .any(|(n, e)| n == &c.name && *e >= c.elite)
    })
}

/// 自身技能对本人产生的消耗修正（受同房「消除自身消耗」抵消；recipe 门限校验）。
fn self_skill_delta(model: &MoodModel, inputs: &DrainInputs<'_>, sui_member: bool) -> f64 {
    if room_clears_self(model, inputs, sui_member) {
        return 0.0;
    }
    model
        .operator_self_skills(inputs.name, inputs.facility, inputs.elite)
        .into_iter()
        .filter(|skill| inputs.room_ops.len() >= skill.min_room_occupancy)
        .filter(|skill| {
            skill
                .recipe
                .as_deref()
                .is_none_or(|want| inputs.recipe == Some(want))
        })
        .map(|skill| skill.drain_delta)
        .sum()
}

/// 同设施他人提供的 room / room_others 修正之和（黍/火哨/巫恋，可叠加）。
fn room_skill_delta(model: &MoodModel, inputs: &DrainInputs<'_>) -> f64 {
    let mut sum = 0.0;
    for (peer_name, peer_elite) in inputs.room_ops {
        let Some(skill) = model.operator_room_skill(peer_name, inputs.facility, *peer_elite) else {
            continue;
        };
        // room_others：只作用于「除提供者外」的人；提供者本人不吃自己的这份。
        if skill.scope == "room_others" && peer_name == inputs.name {
            continue;
        }
        sum += skill.drain_delta;
    }
    sum
}

/// 干员净每小时心情消耗 `x`（下限 0）。
pub fn operator_net_drain(model: &MoodModel, inputs: &DrainInputs<'_>) -> f64 {
    operator_net_drain_with_sui(model, inputs, false)
}

pub(crate) fn operator_net_drain_with_sui(
    model: &MoodModel,
    inputs: &DrainInputs<'_>,
    sui_member: bool,
) -> f64 {
    let mut x = model.base_drain_per_hour;
    if inputs.full_control {
        x -= model.full_control_reduction;
    }
    x -= model.occupancy_reduction(inputs.facility, inputs.room_ops.len());
    x -= model.control_uniform_recovery_for_facility(inputs.facility, inputs.control_ops);
    x += self_skill_delta(model, inputs, sui_member);
    x += room_skill_delta(model, inputs);
    x -= model.control_pair_recovery_for_operator(inputs.facility, inputs.name, inputs.room_ops);
    x.max(0.0)
}

/// 从满心情工作到休息阈值的可工作时长（小时）；`x ≤ 0` 返回 `f64::INFINITY`。
pub fn workable_hours(model: &MoodModel, inputs: &DrainInputs<'_>) -> f64 {
    let x = operator_net_drain(model, inputs);
    if x <= 0.0 {
        f64::INFINITY
    } else {
        model.workable_mood() / x
    }
}

/// 深海中枢修正；ETA与候选房窗口共享，名单统计由调用方提供。
pub(crate) fn control_tag_drain(
    mut drain: f64,
    recovery: bool,
    buffs: &[String],
    abyssal_outside_dorm: usize,
) -> f64 {
    if recovery {
        drain -= 0.05;
    }
    if buffs.iter().any(|buff| buff == "control_mp_aegir2[010]") {
        drain += abyssal_outside_dorm as f64 * 0.5;
    }
    drain.max(0.0)
}
