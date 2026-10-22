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
    let Some(skill) = model.operator_self_skill(inputs.name, inputs.facility, inputs.elite, None)
    else {
        return 0.0;
    };
    // recipe 门限：技能限定某配方，但当前配方不符 → 不生效。
    if let Some(want) = &skill.recipe {
        if inputs.recipe != Some(want.as_str()) {
            return 0.0;
        }
    }
    // 槐琥等在场：抵消同房所有干员自身技能的 mood 影响（正负都消）。
    if room_clears_self(model, inputs, sui_member) {
        return 0.0;
    }
    skill.drain_delta
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> MoodModel {
        MoodModel::load_default().expect("bundled mood_model.json")
    }

    fn solo(name: &str, elite: u8) -> Vec<(String, u8)> {
        vec![(name.to_string(), elite)]
    }

    /// 泡泡三级制造 + 满中枢：1 − 0.25 − 0.1 − 0.25(自身仓储) = 0.4/h → 60h。
    #[test]
    fn paopao_l3_factory_full_control() {
        let m = model();
        let room = vec![
            ("泡泡".to_string(), 0),
            ("能天使".to_string(), 2),
            ("陈".to_string(), 2),
        ];
        let inputs = DrainInputs {
            name: "泡泡",
            elite: 0,
            facility: FacilityKind::Factory,
            room_ops: &room,
            full_control: true,
            control_ops: &[],
            recipe: Some("gold"),
        };
        let x = operator_net_drain(&m, &inputs);
        assert!((x - 0.4).abs() < 1e-9, "x={x}");
        assert!((workable_hours(&m, &inputs) - 60.0).abs() < 1e-6);
    }

    /// 斥罪办公室 + 满中枢：1 + 0.5 − 0.25 = 1.25/h → 19.2h。
    #[test]
    fn chuzui_office_full_control() {
        let m = model();
        let room = solo("斥罪", 0);
        let inputs = DrainInputs {
            name: "斥罪",
            elite: 0,
            facility: FacilityKind::Office,
            room_ops: &room,
            full_control: true,
            control_ops: &[],
            recipe: None,
        };
        let x = operator_net_drain(&m, &inputs);
        assert!((x - 1.25).abs() < 1e-9, "x={x}");
        assert!((workable_hours(&m, &inputs) - 19.2).abs() < 1e-6);
    }

    /// 无额外的三级贸易/制造 + 满中枢：1 − 0.25 − 0.1 = 0.65/h。
    #[test]
    fn plain_l3_trade_full_control() {
        let m = model();
        let room = vec![
            ("能天使".to_string(), 2u8),
            ("陈".to_string(), 2u8),
            ("阿米娅".to_string(), 2u8),
        ];
        let inputs = DrainInputs {
            name: "能天使",
            elite: 2,
            facility: FacilityKind::TradePost,
            room_ops: &room,
            full_control: true,
            control_ops: &[],
            recipe: None,
        };
        assert!((operator_net_drain(&m, &inputs) - 0.65).abs() < 1e-9);
    }

    /// 巫恋组：巫恋 E2 给全体 +0.25，火哨给全体 −0.1，同室净叠加。
    /// 龙舌兰在此房：1 − 0.25 − 0.1(L3) + 0.25(巫恋) − 0.1(火哨) − 0.25(龙舌兰自身投资) = 0.55/h。
    #[test]
    fn witch_room_stacks_room_effects() {
        let m = model();
        let room = vec![
            ("巫恋".to_string(), 2u8),
            ("火哨".to_string(), 0u8),
            ("龙舌兰".to_string(), 2u8),
        ];
        let inputs = DrainInputs {
            name: "龙舌兰",
            elite: 2,
            facility: FacilityKind::TradePost,
            room_ops: &room,
            full_control: true,
            control_ops: &[],
            recipe: None,
        };
        let x = operator_net_drain(&m, &inputs);
        assert!((x - 0.55).abs() < 1e-9, "x={x}");
    }

    /// 槐琥在制造站：抵消阿罗玛自身 +0.25，但不影响黍给全员的 −0.1。
    /// 阿罗玛（贵金属）同房槐琥+黍，L3，满中枢：1 − 0.25 − 0.1 + 0(自身被消) − 0.1(黍) = 0.55/h。
    #[test]
    fn huaihu_clears_self_but_not_room() {
        let m = model();
        let room = vec![
            ("阿罗玛".to_string(), 0u8),
            ("槐琥".to_string(), 0u8),
            ("黍".to_string(), 0u8),
        ];
        let inputs = DrainInputs {
            name: "阿罗玛",
            elite: 0,
            facility: FacilityKind::Factory,
            room_ops: &room,
            full_control: true,
            control_ops: &[],
            recipe: Some("gold"),
        };
        let x = operator_net_drain(&m, &inputs);
        assert!((x - 0.55).abs() < 1e-9, "x={x}");
    }

    /// 泡泡 + 精二玛恩纳(中枢)：玛恩纳自身是笑脸提供者，扩散 −0.05 到制造 → 0.35。
    #[test]
    fn manna_does_not_cover_factory() {
        let m = model();
        let room = vec![
            ("泡泡".to_string(), 0),
            ("能天使".to_string(), 2),
            ("陈".to_string(), 2),
        ];
        let control = vec![("玛恩纳".to_string(), 2u8)];
        let inputs = DrainInputs {
            name: "泡泡",
            elite: 0,
            facility: FacilityKind::Factory,
            room_ops: &room,
            full_control: true,
            control_ops: &control,
            recipe: Some("gold"),
        };
        assert!((operator_net_drain(&m, &inputs) - 0.35).abs() < 1e-9);
    }

    /// 发电站空构 + 满中枢 + 玛恩纳：直接 −0.1，另扩散自身笑脸 −0.05。
    #[test]
    fn power_konggou_with_manna() {
        let m = model();
        let room = vec![("空构".to_string(), 0u8)];
        let control = vec![("玛恩纳".to_string(), 2u8)];
        let inputs = DrainInputs {
            name: "空构",
            elite: 0,
            facility: FacilityKind::PowerPlant,
            room_ops: &room,
            full_control: true,
            control_ops: &control,
            recipe: None,
        };
        let x = operator_net_drain(&m, &inputs);
        assert!((x - 0.3).abs() < 1e-9, "x={x}");
    }

    #[test]
    fn wisadel_with_civilight_eternals_doubles_global_recovery() {
        let m = model();
        let room = solo("空构", 0);
        let ew_only = vec![("维什戴尔".to_string(), 2u8)];
        let ew_with_civilight = vec![("维什戴尔".to_string(), 2u8), ("魔王".to_string(), 2u8)];
        let base = DrainInputs {
            name: "空构",
            elite: 0,
            facility: FacilityKind::PowerPlant,
            room_ops: &room,
            full_control: true,
            control_ops: &ew_only,
            recipe: None,
        };
        assert!((operator_net_drain(&m, &base) - 0.35).abs() < 1e-9);
        let boosted = DrainInputs {
            control_ops: &ew_with_civilight,
            ..base
        };
        assert!((operator_net_drain(&m, &boosted) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn faction_recovery_scales_provider_count_times_member_count() {
        let m = model();
        let room = vec![
            ("能天使".to_string(), 2),
            ("灰烬".to_string(), 2),
            ("战车".to_string(), 2),
            ("闪击".to_string(), 2),
            ("霜华".to_string(), 2),
        ];
        let control = vec![
            ("灰烬".to_string(), 2),
            ("战车".to_string(), 2),
            ("闪击".to_string(), 2),
            ("霜华".to_string(), 2),
        ];
        let inputs = DrainInputs {
            name: "能天使",
            elite: 2,
            facility: FacilityKind::ControlCenter,
            room_ops: &room,
            full_control: true,
            control_ops: &control,
            recipe: None,
        };
        // 0.05 * 4 providers * 4 members = 0.8; full control adds -0.25,
        // so an otherwise neutral member reaches zero net drain.
        assert_eq!(operator_net_drain(&m, &inputs), 0.0);
    }

    #[test]
    fn faction_recovery_is_limited_to_control_center() {
        let m = model();
        let room = solo("能天使", 2);
        let control = vec![
            ("灰烬".to_string(), 2),
            ("战车".to_string(), 2),
            ("闪击".to_string(), 2),
            ("霜华".to_string(), 2),
        ];
        let base = DrainInputs {
            name: "能天使",
            elite: 2,
            facility: FacilityKind::TradePost,
            room_ops: &room,
            full_control: false,
            control_ops: &control,
            recipe: None,
        };
        let control_case = DrainInputs {
            facility: FacilityKind::ControlCenter,
            full_control: true,
            ..base
        };
        assert!((operator_net_drain(&m, &base) - 1.0).abs() < 1e-9);
        assert!((operator_net_drain(&m, &control_case) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn global_recovery_and_full_control_reduction_stack() {
        let m = model();
        let room = solo("空构", 0);
        let control = vec![("维什戴尔".to_string(), 2u8)];
        let inputs = DrainInputs {
            name: "空构",
            elite: 0,
            facility: FacilityKind::PowerPlant,
            room_ops: &room,
            full_control: true,
            control_ops: &control,
            recipe: None,
        };
        // 1.0 - 0.25 (full control) - 0.1 (Wisadel) - 0.3 (Konggou self)
        // = 0.35/h; each source remains additive.
        assert!((operator_net_drain(&m, &inputs) - 0.35).abs() < 1e-9);
    }

    /// recipe 门限：阿罗玛的 +0.25 只在贵金属配方生效；非贵金属时不加。
    #[test]
    fn aroma_recipe_gate() {
        let m = model();
        let room = vec![
            ("阿罗玛".to_string(), 0),
            ("能天使".to_string(), 2),
            ("陈".to_string(), 2),
        ];
        let base = DrainInputs {
            name: "阿罗玛",
            elite: 0,
            facility: FacilityKind::Factory,
            room_ops: &room,
            full_control: true,
            control_ops: &[],
            recipe: Some("battle_record"),
        };
        // 非贵金属：1 − 0.25 − 0.1 = 0.65（自身 +0.25 不生效）。
        assert!((operator_net_drain(&m, &base) - 0.65).abs() < 1e-9);
    }

    /// room_others 只影响提供者以外的人：艾拉自身不吃 +0.25，凯尔希会吃。
    #[test]
    fn room_others_excludes_provider() {
        let m = model();
        let room = vec![("艾拉".to_string(), 2), ("凯尔希".to_string(), 2)];
        let control = room.clone();
        let ella = DrainInputs {
            name: "艾拉",
            elite: 2,
            facility: FacilityKind::ControlCenter,
            room_ops: &room,
            full_control: false,
            control_ops: &control,
            recipe: None,
        };
        let kaltsit = DrainInputs {
            name: "凯尔希",
            ..ella
        };
        assert!((operator_net_drain(&m, &ella) - 1.0).abs() < 1e-9);
        assert!((operator_net_drain(&m, &kaltsit) - 1.25).abs() < 1e-9);
    }

    /// 玛恩纳 + 4 名笑脸提供者把 0.25/h 扩散到制造；泡泡黄金例降到 0.15/h。
    #[test]
    fn manna_spreads_all_smiley_providers() {
        let m = model();
        let room = vec![
            ("泡泡".to_string(), 0),
            ("能天使".to_string(), 2),
            ("陈".to_string(), 2),
        ];
        let control = vec![
            ("玛恩纳".to_string(), 2),
            ("临光".to_string(), 0),
            ("杜宾".to_string(), 0),
            ("灰喉".to_string(), 0),
            ("暴雨".to_string(), 0),
        ];
        let inputs = DrainInputs {
            name: "泡泡",
            elite: 0,
            facility: FacilityKind::Factory,
            room_ops: &room,
            full_control: true,
            control_ops: &control,
            recipe: Some("gold"),
        };
        let x = operator_net_drain(&m, &inputs);
        assert!((x - 0.15).abs() < 1e-9, "x={x}");
    }

    #[test]
    fn workable_hours_respects_rest_threshold() {
        let mut m = model();
        m.rest_threshold = 4.0;
        let room = vec![("能天使".to_string(), 2)];
        let inputs = DrainInputs {
            name: "能天使",
            elite: 2,
            facility: FacilityKind::TradePost,
            room_ops: &room,
            full_control: false,
            control_ops: &[],
            recipe: None,
        };
        assert!((workable_hours(&m, &inputs) - 20.0).abs() < 1e-9);
    }
}
