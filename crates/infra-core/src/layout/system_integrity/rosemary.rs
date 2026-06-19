//! 迷迭香感知链完整性判定（公孙长乐 `ROSEMARY_PERCEPTION_CHAIN.md`）。

use crate::layout::blueprint::FacilityKind;
use crate::layout::shift::AssignShiftMode;

use super::context::EvaluateContext;
use super::plan::{
    OptionalProducer, RosemaryPlan, RosemaryTier, RosemaryVerdict, ShiftBind, SkipReason,
    SystemAnchor,
};

const ROSEMARY: &str = "迷迭香";
const BLACKKEY: &str = "黑键";
const XUYU: &str = "絮雨";
const XI: &str = "夕";
const ALICE: &str = "爱丽丝";
const CHERNI: &str = "车尔尼";
const HAIL: &str = "八幡海铃";
const ZILAN: &str = "焰狐龙梓兰";

const SYSTEM_FULL: &str = "rosemary_perception";
const SYSTEM_CORE: &str = "rosemary_perception_core";

pub fn evaluate_rosemary(ctx: &EvaluateContext<'_>) -> RosemaryVerdict {
    if ctx.mode == AssignShiftMode::Recovery {
        return RosemaryVerdict::Skip(SkipReason::RecoveryShift);
    }

    if let Some(reason) = require_core(ctx) {
        return RosemaryVerdict::Skip(reason);
    }

    let power = ctx.blueprint.count_facility(FacilityKind::PowerPlant);
    if power >= 4 {
        return RosemaryVerdict::Skip(SkipReason::UnsupportedLayout {
            power_stations: power,
        });
    }

    if !has_minimum_perception_source(ctx) {
        return RosemaryVerdict::Skip(SkipReason::InsufficientPerceptionSources);
    }

    let has_xuyu = ctx.owns_at_least(XUYU, 2);
    let has_xi = ctx.operbox.elite_of(XI).is_some();
    let has_dorm_producer = ctx.owns_at_least(ALICE, 2) || ctx.owns_at_least(CHERNI, 2);

    let (tier, system_id, priority) = if has_xuyu {
        if has_xi && has_dorm_producer {
            (RosemaryTier::Tier1, SYSTEM_FULL, 21)
        } else if has_xi {
            (RosemaryTier::Tier2, SYSTEM_FULL, 21)
        } else {
            (RosemaryTier::Tier3, SYSTEM_CORE, 15)
        }
    } else {
        (RosemaryTier::Tier3Substitute, SYSTEM_CORE, 15)
    };

    let mut producers_present = Vec::new();
    let mut producers_missing = Vec::new();

    record_producer(ctx, XI, 0, &mut producers_present, &mut producers_missing);
    record_producer(ctx, XUYU, 2, &mut producers_present, &mut producers_missing);
    record_producer(ctx, ALICE, 2, &mut producers_present, &mut producers_missing);
    record_producer(ctx, CHERNI, 2, &mut producers_present, &mut producers_missing);
    record_producer(ctx, HAIL, 2, &mut producers_present, &mut producers_missing);
    record_producer(ctx, ZILAN, 2, &mut producers_present, &mut producers_missing);

    let mut optional_producers = Vec::new();
    if has_xi {
        optional_producers.push(OptionalProducer {
            operator: XI.into(),
            elite: ctx.operbox.elite_of(XI).unwrap_or(0),
            facility: FacilityKind::ControlCenter,
        });
    }
    if has_xuyu && ctx.has_facility(FacilityKind::Office) {
        optional_producers.push(OptionalProducer {
            operator: XUYU.into(),
            elite: 2,
            facility: FacilityKind::Office,
        });
    }
    if ctx.owns_at_least(ALICE, 2) && ctx.has_facility(FacilityKind::Dormitory) {
        optional_producers.push(OptionalProducer {
            operator: ALICE.into(),
            elite: 2,
            facility: FacilityKind::Dormitory,
        });
    }
    if ctx.owns_at_least(CHERNI, 2) && ctx.has_facility(FacilityKind::Dormitory) {
        optional_producers.push(OptionalProducer {
            operator: CHERNI.into(),
            elite: 2,
            facility: FacilityKind::Dormitory,
        });
    }

    RosemaryVerdict::Activate(RosemaryPlan {
        system_id: system_id.into(),
        priority,
        tier,
        shift_mode: AssignShiftMode::Peak,
        anchors: vec![SystemAnchor {
            operator: ROSEMARY.into(),
            elite: 2,
            facility: FacilityKind::Factory,
            room_id: None,
        }],
        optional_producers,
        shift_bind: ShiftBind {
            operators: vec![ROSEMARY.into(), BLACKKEY.into()],
            on_shifts: 2,
            off_shifts: 1,
        },
        producers_present,
        producers_missing,
    })
}

fn require_core(ctx: &EvaluateContext<'_>) -> Option<SkipReason> {
    for (name, elite) in [(ROSEMARY, 2), (BLACKKEY, 2)] {
        if !ctx.owns_at_least(name, elite) {
            return Some(SkipReason::MissingOperator {
                name: name.into(),
                need_elite: elite,
            });
        }
    }
    None
}

/// §8.2 最低感知源：絮雨 / 八幡海铃 / 焰狐龙梓兰 至少其一。
fn has_minimum_perception_source(ctx: &EvaluateContext<'_>) -> bool {
    ctx.owns_at_least(XUYU, 2) || ctx.owns_at_least(HAIL, 2) || ctx.owns_at_least(ZILAN, 2)
}

fn record_producer(
    ctx: &EvaluateContext<'_>,
    name: &str,
    min_elite: u8,
    present: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    if ctx.owns_at_least(name, min_elite) {
        present.push(name.into());
    } else {
        missing.push(name.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::BaseBlueprint;
    use crate::operbox::OperBox;
    use crate::roster::OperatorProgress;
    use std::collections::HashMap;

    fn mini_operbox(owned: HashMap<&str, u8>) -> OperBox {
        let mut box_owned = HashMap::new();
        for (name, elite) in owned {
            box_owned.insert(name.to_string(), OperatorProgress::new(elite, 90, 6));
        }
        OperBox {
            entries: vec![],
            owned: box_owned,
        }
    }

    fn blueprint_243() -> BaseBlueprint {
        BaseBlueprint::template_243_use_this().unwrap()
    }

    fn full_e2_operbox() -> OperBox {
        let path = crate::skill_table::data_path("fixtures/243/operbox_full_e2.json").unwrap();
        OperBox::load(&path).unwrap()
    }

    #[test]
    fn full_e2_243_peak_activates_tier1() {
        let blueprint = blueprint_243();
        let operbox = full_e2_operbox();
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Peak);
        let verdict = evaluate_rosemary(&ctx);
        let RosemaryVerdict::Activate(plan) = verdict else {
            panic!("expected activate, got {verdict:?}");
        };
        assert_eq!(plan.system_id, SYSTEM_FULL);
        assert_eq!(plan.priority, 21);
        assert_eq!(plan.tier, RosemaryTier::Tier1);
        assert_eq!(plan.anchors.len(), 1);
        assert!(plan.anchors.iter().any(|a| a.operator == ROSEMARY));
        assert!(!plan.anchors.iter().any(|a| a.operator == BLACKKEY));
        assert!(plan.shift_bind.operators.contains(&BLACKKEY.to_string()));
        assert!(plan.producers_present.contains(&XUYU.to_string()));
        assert!(plan.producers_present.contains(&XI.to_string()));
    }

    #[test]
    fn missing_blackkey_skips() {
        let blueprint = blueprint_243();
        let operbox = mini_operbox(HashMap::from([(ROSEMARY, 2), (XUYU, 2)]));
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Peak);
        let verdict = evaluate_rosemary(&ctx);
        assert_eq!(
            verdict,
            RosemaryVerdict::Skip(SkipReason::MissingOperator {
                name: BLACKKEY.into(),
                need_elite: 2,
            })
        );
    }

    #[test]
    fn recovery_shift_skips() {
        let operbox = full_e2_operbox();
        let blueprint = blueprint_243();
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Recovery);
        assert_eq!(
            evaluate_rosemary(&ctx),
            RosemaryVerdict::Skip(SkipReason::RecoveryShift)
        );
    }

    #[test]
    fn no_minimum_perception_source_skips_even_with_xi() {
        let blueprint = blueprint_243();
        let operbox = mini_operbox(HashMap::from([
            (ROSEMARY, 2),
            (BLACKKEY, 2),
            (XI, 0),
            (ALICE, 2),
        ]));
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Peak);
        assert_eq!(
            evaluate_rosemary(&ctx),
            RosemaryVerdict::Skip(SkipReason::InsufficientPerceptionSources)
        );
    }

    #[test]
    fn core_plus_xuyu_no_xi_is_tier3() {
        let blueprint = blueprint_243();
        let operbox = mini_operbox(HashMap::from([(ROSEMARY, 2), (BLACKKEY, 2), (XUYU, 2)]));
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Peak);
        let RosemaryVerdict::Activate(plan) = evaluate_rosemary(&ctx) else {
            panic!("expected activate");
        };
        assert_eq!(plan.tier, RosemaryTier::Tier3);
        assert_eq!(plan.system_id, SYSTEM_CORE);
        assert_eq!(plan.priority, 15);
    }

    #[test]
    fn hail_substitute_without_xuyu_is_tier3_substitute() {
        let blueprint = blueprint_243();
        let operbox = mini_operbox(HashMap::from([(ROSEMARY, 2), (BLACKKEY, 2), (HAIL, 2)]));
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Peak);
        let RosemaryVerdict::Activate(plan) = evaluate_rosemary(&ctx) else {
            panic!("expected activate");
        };
        assert_eq!(plan.tier, RosemaryTier::Tier3Substitute);
        assert_eq!(plan.system_id, SYSTEM_CORE);
        assert_eq!(plan.priority, 15);
    }

}
