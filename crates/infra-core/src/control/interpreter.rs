use std::collections::HashMap;

use crate::global_resource::{
    GlobalInjectManifest, INJECT_FAMILY_MANU_GLOBAL_ALL, INJECT_FAMILY_TRADE_GLOBAL_FLAT,
};
use crate::control::input::ControlRoomInput;
use crate::global_resource::GlobalResourcePool;
use crate::skill_table::SkillTable;
use crate::layout::trade_station_tagged_gte_key;
use crate::types::{Action, Condition, EffectAtom, Phase, Selector, StateKey};

#[derive(Debug, Clone, Default)]
struct ControlOperatorRuntime {
    name: String,
    buff_ids: Vec<String>,
    tags: Vec<String>,
    mood_drain_delta: f64,
}

#[derive(Debug, Clone)]
struct ControlContext {
    operators: Vec<ControlOperatorRuntime>,
    layout: crate::layout::LayoutContext,
    mood: f64,
    state_pool: HashMap<StateKey, f64>,
    inject: GlobalInjectManifest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlCenterResult {
    pub global: GlobalResourcePool,
    pub inject: GlobalInjectManifest,
    /// 中枢干员 L1 心情消耗增量（按名；排班非目标，仅供机制记账）。
    pub operator_mood_drains: HashMap<String, f64>,
}

pub fn solve_control(input: &ControlRoomInput, table: &SkillTable) -> ControlCenterResult {
    let mut ctx = ControlContext {
        operators: input
            .operators
            .iter()
            .map(|o| ControlOperatorRuntime {
                name: o.name.clone(),
                buff_ids: o.buff_ids.clone(),
                tags: o.tags.clone(),
                mood_drain_delta: 0.0,
            })
            .collect(),
        layout: input.layout.clone(),
        mood: input.mood,
        state_pool: input.layout.global.to_room_state(),
        inject: GlobalInjectManifest::default(),
    };

    let atoms = collect_control_atoms(&ctx.operators, table);
    for (atom, owner) in atoms {
        if !condition_met(&atom.condition, &ctx, &owner) {
            continue;
        }
        apply_atom(&mut ctx, &atom, &owner);
    }

    let mut global = input.layout.global.clone();
    for (key, value) in ctx.state_pool {
        if value != 0.0 {
            global.set(key, value);
        }
    }

    let operator_mood_drains = ctx
        .operators
        .iter()
        .map(|o| (o.name.clone(), o.mood_drain_delta))
        .collect();

    ControlCenterResult {
        global,
        inject: ctx.inject,
        operator_mood_drains,
    }
}

fn collect_control_atoms<'a>(
    ops: &[ControlOperatorRuntime],
    table: &'a SkillTable,
) -> Vec<(&'a EffectAtom, String)> {
    let mut atoms = Vec::new();
    for op in ops {
        for bid in &op.buff_ids {
            let Some(skill) = table.get(bid) else { continue };
            if skill.facility != "control" {
                continue;
            }
            for atom in &skill.atoms {
                atoms.push((atom, op.name.clone()));
            }
        }
    }
    atoms.sort_by(|(a, _), (b, _)| {
        let pa = a.phase.sort_key();
        let pb = b.phase.sort_key();
        pa.cmp(&pb).then(a.phase_order.cmp(&b.phase_order))
    });
    atoms
}

fn condition_met(cond: &Option<Condition>, ctx: &ControlContext, owner: &str) -> bool {
    let Some(cond) = cond else { return true };
    match cond {
        Condition::MoodAbove { n } => ctx.mood > *n as f64,
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::OperatorInBase { name } => ctx.layout.base_workforce.iter().any(|n| n == name),
        Condition::OperatorInPower { name } => ctx.layout.power_workforce.iter().any(|n| n == name),
        Condition::OperatorInTraining { name } => {
            ctx.layout.training_assist.iter().any(|n| n == name)
        }
        Condition::NoPlatformInOtherPower {} => !ctx.layout.other_power_has_platform,
        Condition::OtherPlatformInPower {} => ctx.layout.other_platform_in_power,
        Condition::OtherLateranoInPower {} => ctx.layout.other_laterano_in_power,
        Condition::PartnerInRoom { name } => ctx.operators.iter().any(|o| o.name == *name),
        Condition::TagPresentInRoom { tag } => ctx
            .operators
            .iter()
            .any(|o| o.tags.iter().any(|t| t == tag)),
        Condition::PeerTagInRoom { tag } => ctx.operators.iter().any(|o| {
            o.name != owner && o.tags.iter().any(|t| t == tag)
        }),
        Condition::ExternalMomentumGteField {} => {
            ctx.layout.external_momentum() >= ctx.layout.field_momentum()
        }
        Condition::FieldMomentumGtExternal {} => {
            ctx.layout.field_momentum() > ctx.layout.external_momentum()
        }
        Condition::PlatformCountGte { min } => ctx.layout.platform_count_in_power >= *min,
        _ => true,
    }
}

fn scaled_inject_value(ctx: &ControlContext, atom: &EffectAtom, base: f64) -> f64 {
    if atom.selector.is_some() {
        resolve_selector_value(ctx, atom.selector.as_ref()) * base
    } else {
        base
    }
}

fn apply_atom(ctx: &mut ControlContext, atom: &EffectAtom, owner: &str) {
    match atom.phase {
        Phase::StateWrite => apply_state_write(ctx, atom),
        Phase::GlobalInject => apply_global_inject(ctx, atom),
        Phase::Mood => apply_mood_action(ctx, &atom.action, owner),
        _ => {}
    }
}

fn apply_mood_action(ctx: &mut ControlContext, action: &Action, owner: &str) {
    match action {
        Action::MoodDrainDelta { delta, scope } => match scope {
            crate::types::MoodDrainScope::SelfOp => {
                if let Some(op) = ctx.operators.iter_mut().find(|o| o.name == owner) {
                    op.mood_drain_delta += delta;
                }
            }
            crate::types::MoodDrainScope::RoomOperators => {
                for op in &mut ctx.operators {
                    op.mood_drain_delta += delta;
                }
            }
        },
        Action::MoodDrainPerStateStep {
            key,
            step_size,
            delta_per_step,
            scope,
        } => {
            let Some(sk) = StateKey::parse(key) else {
                return;
            };
            let state = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            let steps = if *step_size > 0.0 {
                (state / step_size).floor()
            } else {
                0.0
            };
            let delta = steps * delta_per_step;
            match scope {
                crate::types::MoodDrainScope::SelfOp => {
                    if let Some(op) = ctx.operators.iter_mut().find(|o| o.name == owner) {
                        op.mood_drain_delta += delta;
                    }
                }
                crate::types::MoodDrainScope::RoomOperators => {
                    for op in &mut ctx.operators {
                        op.mood_drain_delta += delta;
                    }
                }
            }
        }
        _ => {}
    }
}

fn apply_state_write(ctx: &mut ControlContext, atom: &EffectAtom) {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            let Some(sk) = StateKey::parse(key) else {
                return;
            };
            let scale = resolve_selector_value(ctx, atom.selector.as_ref());
            let add = if atom.selector.is_some() {
                scale * amount
            } else {
                *amount
            };
            *ctx.state_pool.entry(sk).or_insert(0.0) += add;
        }
        _ => {}
    }
}

fn apply_global_inject(ctx: &mut ControlContext, atom: &EffectAtom) {
    let family = atom
        .tag
        .as_deref()
        .unwrap_or(match &atom.action {
            Action::GlobalInjectTradeEff { .. } => INJECT_FAMILY_TRADE_GLOBAL_FLAT,
            Action::GlobalInjectManuEff { .. } => INJECT_FAMILY_MANU_GLOBAL_ALL,
            _ => "default",
        });
    match &atom.action {
        Action::GlobalInjectTradeEff { value } => {
            let v = scaled_inject_value(ctx, atom, *value);
            if v != 0.0 {
                ctx.inject.record_trade(family, v);
            }
        }
        Action::GlobalInjectManuEff { value, recipe } => {
            let v = scaled_inject_value(ctx, atom, *value);
            if v != 0.0 {
                ctx.inject.record_manu(family, *recipe, v);
            }
        }
        Action::GlobalInjectKarlanPrecision {
            eff_per_karlan,
            limit_per_karlan,
        } => {
            ctx.inject
                .record_karlan_precision(*eff_per_karlan, *limit_per_karlan);
        }
        _ => {}
    }
}

fn resolve_selector_value(ctx: &ControlContext, selector: Option<&Selector>) -> f64 {
    match selector {
        Some(Selector::TaggedCountInControl { tag }) => ctx
            .operators
            .iter()
            .filter(|o| o.tags.iter().any(|t| t == tag))
            .count() as f64,
        Some(Selector::ControlOperatorCount) => ctx.operators.len() as f64,
        Some(Selector::SuiFacilityCount) => f64::from(ctx.layout.sui_facility_count),
        Some(Selector::CappedSuiFacilityCount { max }) => {
            f64::from(ctx.layout.sui_facility_count.min(*max))
        }
        Some(Selector::DormOccupantCount) => f64::from(ctx.layout.dorm_occupant_count),
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::PlatformCountInPower) => f64::from(ctx.layout.platform_count_in_power),
        Some(Selector::TaggedCountInTradeSum { tag }) => f64::from(
            *ctx.layout
                .trade_tagged_count_sum
                .get(tag)
                .unwrap_or(&0),
        ),
        Some(Selector::TradeStationsWithTaggedGte { tag, min }) => f64::from(
            *ctx.layout
                .trade_stations_tagged_gte
                .get(&trade_station_tagged_gte_key(tag, *min))
                .unwrap_or(&0),
        ),
        Some(Selector::TaggedCountInManuSum { tag }) => f64::from(
            *ctx.layout.manu_tagged_count_sum.get(tag).unwrap_or(&0),
        ),
        Some(Selector::StatePoolFloored { key, div }) => {
            let Some(sk) = StateKey::parse(key) else {
                return 0.0;
            };
            let v = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            if *div <= 0.0 {
                0.0
            } else {
                (v / div).floor()
            }
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::input::ControlOperator;
    use crate::global_resource::GlobalResourceKey;
    use crate::instances::{default_instances_path, resolve_buff_ids, OperatorInstances};
    use crate::tier::PromotionTier;
    use crate::layout::LayoutContext;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    fn instances() -> OperatorInstances {
        OperatorInstances::load(&default_instances_path().unwrap()).unwrap()
    }

    fn control_op(name: &str, elite: u8) -> ControlOperator {
        let inst = instances();
        let tier = PromotionTier::from_elite(elite);
        let binding = inst.get(name, tier).and_then(|i| i.facilities.get("control"));
        let tier0 = inst
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("control"));
        let buff_ids = binding
            .map(|b| resolve_buff_ids(tier, b, tier0))
            .unwrap_or_default();
        let tags = inst
            .get(name, tier)
            .map(|i| i.tags.clone())
            .unwrap_or_default();
        ControlOperator {
            name: name.into(),
            elite,
            buff_ids,
            tags,
        }
    }

    #[test]
    fn amiya_global_trade_plus_seven() {
        let input = ControlRoomInput::with_operators(vec![control_op("阿米娅", 0)]);
        let result = solve_control(&input, &table());
        assert!((result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn trade_global_flat_takes_max_not_sum() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("阿米娅", 0),
            control_op("诗怀雅", 0),
        ]);
        let result = solve_control(&input, &table());
        assert!((result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn starbear_with_lgd_peer_manu_plus_three() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("斩业星熊", 2),
            control_op("诗怀雅", 0),
        ]);
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 3.0).abs() < f64::EPSILON,
            "manu={}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn mon3tr_global_manu_plus_two() {
        let input = ControlRoomInput::with_operators(vec![control_op("Mon3tr", 2)]);
        let result = solve_control(&input, &table());
        assert!((result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn kaltsit_global_manu_plus_two() {
        let input = ControlRoomInput::with_operators(vec![control_op("凯尔希", 2)]);
        let result = solve_control(&input, &table());
        assert!((result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn saria_virtual_power_when_lancet_in_power() {
        let mut layout = LayoutContext::default();
        layout.power_workforce = vec!["Lancet-2".into()];
        let input = ControlRoomInput {
            operators: vec![control_op("森蚺", 2)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!((result.global.get(GlobalResourceKey::VirtualPower) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn matatabi_from_monhun_ops() {
        let mut layout = LayoutContext::default();
        let input = ControlRoomInput {
            operators: vec![
                control_op("火龙S黑角", 0),
                control_op("麒麟R夜刀", 0),
            ],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        // 黑角 2×2 + 夜刀 8
        assert!((result.global.get(GlobalResourceKey::Matatabi) - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn kirin_yato_stamina_mood_drain() {
        let input = ControlRoomInput::with_operators(vec![control_op("麒麟R夜刀", 0)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.operator_mood_drains["麒麟R夜刀"] - 0.5).abs() < f64::EPSILON,
            "mood={}",
            result.operator_mood_drains["麒麟R夜刀"]
        );
    }

    #[test]
    fn monhun_tier2_global_inject_requires_peer() {
        let solo = solve_control(
            &ControlRoomInput::with_operators(vec![control_op("火龙S黑角", 2)]),
            &table(),
        );
        assert!(solo.inject.trade_eff_pct().abs() < f64::EPSILON);

        let duo = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("火龙S黑角", 2),
                control_op("麒麟R夜刀", 2),
            ]),
            &table(),
        );
        assert!((duo.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
        assert!(
            (duo.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn monhun_trade_global_flat_max_with_amiya() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("阿米娅", 0),
            control_op("火龙S黑角", 2),
            control_op("麒麟R夜刀", 2),
        ]);
        let result = solve_control(&input, &table());
        assert!((result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ling_e2_produces_human_fireworks_at_high_mood() {
        let input = ControlRoomInput::with_operators(vec![control_op("令", 2)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::HumanFireworks) - 15.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::HumanFireworks)
        );
    }

    #[test]
    fn chongyue_sui_facility_produces_fireworks() {
        let mut layout = LayoutContext::default();
        layout.sui_facility_count = 2;
        let input = ControlRoomInput {
            operators: vec![control_op("重岳", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::HumanFireworks) - 10.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::HumanFireworks)
        );
    }

    #[test]
    fn tucha_usaut_drink_from_tagged_peer_in_control() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("战车", 2),
            ControlOperator {
                name: "凛冬".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec!["cc.g.usaut".into()],
            },
        ]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::UsautDrink) - 1.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::UsautDrink)
        );
    }

    #[test]
    fn wang_quanbian_trade_when_external_gte_field() {
        let mut layout = LayoutContext::default();
        layout.trade_station_count = 2;
        layout.power_station_count = 3;
        layout.manufacture_station_count = 4;
        let input = ControlRoomInput {
            operators: vec![control_op("望", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON,
            "243-like 5>=4 → trade +7%, got {}",
            result.inject.trade_eff_pct()
        );
        assert!(
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold).abs() < f64::EPSILON,
            "manu inject should be 0, got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn wang_quanbian_manu_when_field_gt_external() {
        let mut layout = LayoutContext::default();
        layout.trade_station_count = 1;
        layout.power_station_count = 1;
        layout.manufacture_station_count = 4;
        let input = ControlRoomInput {
            operators: vec![control_op("望", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(result.inject.trade_eff_pct().abs() < f64::EPSILON);
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON,
            "2<4 → manu +2%, got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn kjera_precision_calculation_records_karlan_rule() {
        let input = ControlRoomInput::with_operators(vec![control_op("灵知", 2)]);
        let result = solve_control(&input, &table());
        let kp = result
            .inject
            .karlan_precision()
            .expect("精密计算应写入 manifest");
        assert!((kp.eff_per_karlan + 15.0).abs() < f64::EPSILON);
        assert_eq!(kp.limit_per_karlan, 6);
    }

    #[test]
    fn xi_perception_when_mood_above_twelve() {
        let input = ControlRoomInput::with_operators(vec![control_op("夕", 0)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::Perception) - 10.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::Perception)
        );
        assert!(
            (result.operator_mood_drains["夕"] - 0.5).abs() < f64::EPSILON,
            "mood={}",
            result.operator_mood_drains["夕"]
        );
    }

    #[test]
    fn ash_intelligence_reserve_from_rainbow_peer() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("灰烬", 2),
            ControlOperator {
                name: "闪击".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec!["cc.g.rainbow".into()],
            },
        ]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::IntelligenceReserve) - 2.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::IntelligenceReserve)
        );
    }

    #[test]
    fn muzu_passion_trade_and_mood_step() {
        let input = ControlRoomInput::with_operators(vec![control_op("若叶睦", 0)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::Passion) - 20.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::Passion)
        );
        assert!(
            (result.inject.trade_eff_pct() - 2.0).abs() < f64::EPSILON,
            "trade inject {}",
            result.inject.trade_eff_pct()
        );
        assert!(
            (result.operator_mood_drains["若叶睦"] - 0.02).abs() < f64::EPSILON,
            "mood {}",
            result.operator_mood_drains["若叶睦"]
        );
    }

    #[test]
    fn pudding_manu_inject_when_two_platforms() {
        let mut layout = LayoutContext::default();
        layout.platform_count_in_power = 2;
        let input = ControlRoomInput {
            operators: vec![control_op("布丁", 2)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON,
            "got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }
}
