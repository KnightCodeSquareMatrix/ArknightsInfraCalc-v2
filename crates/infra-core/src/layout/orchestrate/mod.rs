//! 全基建进驻编排：`System → Plan → Execute`。
//!
//! Phase 0–1：`System → Plan → Execute`；`select` 合并 integrity + `base_systems` 选型。

mod execute;
mod plan;
mod select;

pub use execute::{execute_plan, ExecuteResult};
pub use plan::{ActivatedSystem, AssignmentPlan, SlotFill};
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
