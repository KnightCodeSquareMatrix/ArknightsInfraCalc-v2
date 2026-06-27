//! 全基建进驻编排：`System → Plan → Execute`。
//!
//! Phase 0–1：`System → Plan → Execute`；`select` 合并 integrity + `base_systems` 选型。

mod execute;
mod plan;
mod select;

pub use execute::{execute_plan, ExecuteResult};
pub use plan::{
    ActivatedSystem, AssignmentPlan, ProducerSlot, SlotFill, SystemAnchor, SystemConstraint,
};
pub use select::build_plan;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assignment::BaseAssignment;
    use crate::layout::shift::AssignShiftMode;
    use crate::layout::BaseBlueprint;
    use crate::operbox::OperBox;

    #[test]
    fn build_plan_recovery_is_empty() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Recovery,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(plan.activated.is_empty());
        assert!(plan.registry_claims.is_empty());
    }

    #[test]
    fn build_plan_peak_ideal_e2_claims_pinus_sylvestris() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        if !operbox.owns("焰尾") || !operbox.owns("薇薇安娜") {
            return;
        }
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(
            plan.registry_claims
                .iter()
                .any(|c| c.system_id == "pinus_sylvestris"),
            "peak plan 应含红松林: {:?}",
            plan.registry_system_ids()
        );
    }

    #[test]
    fn build_plan_peak_ideal_e2_does_not_registry_claim_rosemary() {
        // 迷迭香感知链走代码化体系层（system_integrity），不再作为 registry 体系。
        // build_plan 把代码化产出汇合进统一 plan 的 anchors/degradations/shift_binds，
        // 但不进 registry_claims；黑键走贸易贪心 + 上2休1 绑定。
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(
            !plan
                .registry_claims
                .iter()
                .any(|c| c.system_id == "rosemary_perception"),
            "迷迭香不应作为 registry 体系认领: {:?}",
            plan.registry_system_ids()
        );
    }

    #[test]
    fn build_plan_peak_ideal_e2_converges_rosemary_into_unified_plan() {
        // ADR 0001 决策 B：代码化体系层与 registry 汇合到统一 AssignmentPlan。
        // 迷迭香满配应产出 迷迭香制造 anchor + 上2休1 绑定 + 降级阶梯档位。
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        if !operbox.owns("迷迭香") || !operbox.owns("黑键") {
            return;
        }
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        // 迷迭香制造 anchor 汇入 plan.anchors；黑键不锚定。
        assert!(
            plan.anchors
                .iter()
                .any(|a| a.operator == "迷迭香"
                    && a.facility == crate::layout::blueprint::FacilityKind::Factory),
            "应含迷迭香制造 anchor: {:?}",
            plan.anchors
        );
        assert!(
            !plan.anchors.iter().any(|a| a.operator == "黑键"),
            "黑键不应锚定（走贸易贪心）"
        );
        // 上2休1 shift_bind 汇入 plan.shift_binds。
        assert!(
            plan.shift_binds.iter().any(|b| {
                b.operators.iter().any(|o| o == "迷迭香")
                    && b.operators.iter().any(|o| o == "黑键")
                    && b.on_shifts == 2
                    && b.off_shifts == 1
            }),
            "应含迷迭香+黑键上2休1绑定: {:?}",
            plan.shift_binds
        );
        // 降级阶梯档位汇入 plan.degradations。
        assert!(
            !plan.degradations.is_empty(),
            "应含迷迭香降级阶梯档位"
        );
        // forbid-same-room 约束汇入 plan.constraints（迷迭香 ≠ 清流/温蒂同房）。
        assert!(
            plan.constraints.iter().any(|c| matches!(
                c,
                crate::layout::orchestrate::SystemConstraint::ForbidSameRoom { a, b }
                    if a == "迷迭香" && (b == "清流" || b == "温蒂")
            )),
            "应含迷迭香≠清流/温蒂同房约束: {:?}",
            plan.constraints
        );
    }

    #[test]
    fn build_plan_peak_without_rosemary_leaves_codeized_plan_empty() {
        // 不拥有迷迭香/黑键时，代码化体系层不激活，统一 plan 的语义片段为空。
        use crate::operbox::OperBoxEntry;
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(vec![OperBoxEntry {
            id: "f".into(),
            name: "芬".into(),
            elite: 2,
            level: 1,
            own: true,
            potential: 0,
            rarity: 3,
        }]);
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(plan.anchors.is_empty(), "无迷迭香不应有 anchor");
        assert!(plan.shift_binds.is_empty(), "无迷迭香不应有 shift_bind");
        assert!(plan.degradations.is_empty(), "无迷迭香不应有降级档位");
    }

    #[test]
    fn build_plan_peak_ideal_e2_claims_blackkey_closure_with_docus() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        if !operbox.owns("可露希尔") || !operbox.owns("黑键") || !operbox.owns("吉星") {
            return;
        }
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(
            plan.registry_claims
                .iter()
                .any(|c| c.system_id == "blackkey_closure"),
            "但书+迷迭香绑定上下文应含可露希尔黑键站: {:?}",
            plan.registry_system_ids()
        );
        assert!(
            plan.registry_claims
                .iter()
                .any(|c| c.system_id == "docus_syracusa"),
            "peak plan 应与但书链共存"
        );
    }

    #[test]
    fn build_plan_peak_ideal_e2_activates_docus() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        if !operbox.owns("但书") {
            return;
        }
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(
            plan.registry_claims
                .iter()
                .any(|c| c.system_id == "docus_syracusa"),
            "应选型叙拉古链: {:?}",
            plan.registry_claims
                .iter()
                .map(|c| &c.system_id)
                .collect::<Vec<_>>()
        );
        assert!(
            !plan
                .registry_claims
                .iter()
                .any(|c| c.system_id == "ling_jie_karlan"),
            "exclusive_group 应优先但书链而非灵知喀兰"
        );
        assert!(!plan.activated.is_empty());
    }

    #[test]
    fn execute_plan_matches_select_registry_claims() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        let table = crate::skill_table::SkillTable::load(
            &crate::skill_table::default_skill_table_path().unwrap(),
        )
        .unwrap();
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        let executed = execute_plan(
            &blueprint,
            &operbox,
            &table,
            &plan,
            &BaseAssignment::default(),
        )
        .unwrap();
        assert!(
            executed
                .assignment
                .rooms
                .iter()
                .any(|r| r.operators.iter().any(|o| o.name == "但书")),
            "execute 应落位但书链"
        );
    }

    #[test]
    fn build_plan_selects_penguin_exusiai_lemuen_when_available() {
        use crate::operbox::OperBoxEntry;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(vec![
            OperBoxEntry {
                id: "exu".into(),
                name: "能天使".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 0,
                rarity: 6,
            },
            OperBoxEntry {
                id: "lem".into(),
                name: "蕾缪安".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 0,
                rarity: 6,
            },
            OperBoxEntry {
                id: "kong".into(),
                name: "空".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 0,
                rarity: 5,
            },
            OperBoxEntry {
                id: "f".into(),
                name: "芬".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 0,
                rarity: 3,
            },
        ]);
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(
            plan.registry_claims
                .iter()
                .any(|c| c.system_id == "penguin_exusiai_lemuen"),
            "应选型企鹅能蕾链: {:?}",
            plan.registry_claims
                .iter()
                .map(|c| &c.system_id)
                .collect::<Vec<_>>()
        );
        assert!(
            !plan
                .registry_claims
                .iter()
                .any(|c| c.system_id == "penguin_texlap_e0"),
            "全精2 operbox 不应选德E0狼链"
        );
    }
}
