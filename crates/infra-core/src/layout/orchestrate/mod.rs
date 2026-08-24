//! 全基建进驻编排：`Rule → resolved Plan → Execute`。
//!
//! `select` 汇合高优先声明式规则、兼容 registry 与 late competitive 规则；
//! `execute` 只消费已解析 placement，不按体系 id 重判。

mod execute;
mod plan;
mod rules;
mod select;

pub use execute::{execute_plan, ExecuteResult};
pub(crate) use plan::pack_production_components;
pub use plan::{
    ActivatedSystem, ActiveDependency, AnchorFillPolicy, AssignmentPlan, ContinuousRole,
    ControlCandidateRequirement, ProducerSlot, ReserveReusePolicy, ResolvedRoleReserve,
    SelectedRuleAlternative, ShiftBind, SlotFill, SystemAnchor, SystemConstraint,
};
pub use select::{build_plan, build_plan_with_runtime};
pub(crate) use select::{build_plan_with_runtime_and_disabled_packs, guarded_pack_violation};

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
    fn mujica_fallback_requires_full_roster_and_no_other_cross_station_plan() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(
            [
                ("若叶睦", 0),
                ("三角初华", 0),
                ("丰川祥子", 2),
                ("祐天寺若麦", 0),
                ("八幡海铃", 0),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (name, elite))| crate::operbox::OperBoxEntry {
                id: format!("mujica-{index}"),
                name: name.to_string(),
                elite,
                level: 1,
                own: true,
                potential: 0,
                rarity: 6,
            })
            .collect(),
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

        let mujica = plan
            .selected_rules
            .iter()
            .find(|selected| selected.rule_id == "mujica_fallback");
        assert!(
            mujica.is_some(),
            "Mujica fallback should be selected: {plan:?}"
        );
        assert_eq!(mujica.unwrap().operators.len(), 5);
        assert_eq!(
            plan.anchors
                .iter()
                .filter(|anchor| anchor.system_id == "mujica_fallback")
                .count(),
            5
        );
        assert!(plan.shift_binds.iter().any(|bind| {
            bind.on_shifts == 2
                && bind.off_shifts == 1
                && ["若叶睦", "三角初华", "丰川祥子", "祐天寺若麦", "八幡海铃"]
                    .iter()
                    .all(|name| bind.operators.iter().any(|operator| operator == name))
        }));

        let incomplete = OperBox::from_entries(
            [
                ("若叶睦", 0),
                ("三角初华", 0),
                ("丰川祥子", 2),
                ("祐天寺若麦", 0),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (name, elite))| crate::operbox::OperBoxEntry {
                id: format!("mujica-incomplete-{index}"),
                name: name.to_string(),
                elite,
                level: 1,
                own: true,
                potential: 0,
                rarity: 6,
            })
            .collect(),
        )
        .unwrap();
        let incomplete_plan = build_plan(
            &blueprint,
            &incomplete,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(!incomplete_plan
            .selected_rules
            .iter()
            .any(|selected| selected.rule_id == "mujica_fallback"));
        for name in ["若叶睦", "三角初华", "丰川祥子", "祐天寺若麦"] {
            assert!(
                incomplete_plan
                    .control_exclusions
                    .contains(&name.to_string()),
                "incomplete Mujica member {name} must not return to normal control fill"
            );
        }

        let with_pinuses = OperBox::from_entries(
            [
                ("若叶睦", 0),
                ("三角初华", 0),
                ("丰川祥子", 2),
                ("祐天寺若麦", 0),
                ("八幡海铃", 0),
                ("焰尾", 2),
                ("薇薇安娜", 2),
                ("灰毫", 2),
                ("远牙", 2),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (name, elite))| crate::operbox::OperBoxEntry {
                id: format!("mujica-pinus-{index}"),
                name: name.to_string(),
                elite,
                level: 1,
                own: true,
                potential: 0,
                rarity: 6,
            })
            .collect(),
        )
        .unwrap();
        let pinus_plan = build_plan(
            &blueprint,
            &with_pinuses,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        assert!(pinus_plan
            .selected_rules
            .iter()
            .any(|selected| selected.rule_id == "pinus_sylvestris"));
        assert!(!pinus_plan
            .selected_rules
            .iter()
            .any(|selected| selected.rule_id == "mujica_fallback"));
        for name in ["若叶睦", "三角初华", "丰川祥子", "祐天寺若麦"] {
            assert!(
                pinus_plan.control_exclusions.contains(&name.to_string()),
                "Mujica member {name} must stay excluded when another cross-station system wins"
            );
        }
    }

    #[test]
    fn pure_fireworks_declares_solver_candidate_requirement() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let full = OperBox::load(&crate::operbox::default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = full.excluding(&std::collections::HashSet::from([
            "迷迭香".to_string(),
            "黑键".to_string(),
            "灵知".to_string(),
            "孑".to_string(),
            "银灰".to_string(),
            "崖心".to_string(),
            "讯使".to_string(),
            "角峰".to_string(),
        ]));
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();

        assert!(plan.registry_system_ids().contains(&"human_fireworks_pure"));
        assert!(plan
            .control_candidate_requirements
            .iter()
            .any(|requirement| {
                requirement.min_count == 1
                    && requirement.candidates.contains(&"重岳".to_string())
                    && requirement.candidates.contains(&"令".to_string())
            }));
        let fixed_control_count: usize = plan
            .registry_claims
            .iter()
            .flat_map(|claim| &claim.slots)
            .filter(|slot| {
                slot.facility == crate::layout::FacilityKind::ControlCenter
                    && slot.fill != crate::layout::system::SlotFillMode::Search
            })
            .map(|slot| slot.operators.len())
            .sum();
        let control_anchors = plan
            .anchors
            .iter()
            .filter(|anchor| anchor.facility == crate::layout::FacilityKind::ControlCenter)
            .count();
        assert!(
            control_anchors + fixed_control_count + 1 <= 5,
            "代码化 anchor、registry fixed 与 required_count 不得超卖中枢容量"
        );
        assert!(plan
            .registry_operator_names()
            .iter()
            .all(|name| !name.starts_with("__control_reservation_")));
    }

    #[test]
    fn perception_fireworks_requires_both_control_candidates() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let full = OperBox::load(&crate::operbox::default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = full.excluding(&std::collections::HashSet::from([
            "焰尾".to_string(),
            "薇薇安娜".to_string(),
            "斩业星熊".to_string(),
            "诗怀雅".to_string(),
            "火龙S黑角".to_string(),
            "麒麟R夜刀".to_string(),
        ]));
        let instances = crate::instances::OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let table = crate::skill_table::SkillTable::load(
            &crate::skill_table::default_skill_table_path().unwrap(),
        )
        .unwrap();
        let plan = crate::layout::assign_shift_with_plan(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &crate::layout::AssignBaseOptions::default(),
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap()
        .plan;

        assert!(!plan.registry_system_ids().contains(&"human_fireworks_pure"));
        assert!(!plan
            .registry_system_ids()
            .contains(&"human_fireworks_perception"));
        assert!(plan
            .selected_rules
            .iter()
            .any(|selected| selected.rule_id == "rosemary_perception"));
        assert!(plan.selected_rules.iter().any(|selected| {
            selected.rule_id == "human_fireworks_perception"
                && selected.alternative_id == "actual_perception_core"
        }));
        let rosemary = plan
            .selected_rules
            .iter()
            .find(|selected| selected.rule_id == "rosemary_perception")
            .unwrap();
        assert_eq!(
            rosemary.conditional_pack_id, None,
            "最终 exact-presence 依赖使两座贸易房不可分时，三 cohort pack 必须在 plan 阶段事务性降级"
        );
        assert!(
            plan.rotation_reserves.is_empty(),
            "降级不得遗留不可分 half 的 cohort C reserve"
        );
        let attached: Vec<_> = plan
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == "human_fireworks_perception")
            .map(|anchor| anchor.operator.as_str())
            .collect();
        assert_eq!(attached, vec!["重岳", "令", "乌有"]);
        assert!(plan.shift_binds.iter().any(|bind| {
            ["重岳", "令", "乌有"]
                .iter()
                .all(|name| bind.operators.iter().any(|operator| operator == name))
        }));
    }

    #[test]
    fn fireworks_branches_close_when_required_members_are_missing() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let full = OperBox::load(&crate::operbox::default_operbox_full_e2_path().unwrap()).unwrap();
        let build = |operbox: &OperBox| {
            build_plan(
                &blueprint,
                operbox,
                AssignShiftMode::Peak,
                &BaseAssignment::default(),
                &std::collections::HashSet::new(),
            )
            .unwrap()
        };
        let assert_closed = |plan: &AssignmentPlan, system_id: &str| {
            assert!(!plan.registry_system_ids().contains(&system_id));
            assert!(plan
                .selected_rules
                .iter()
                .all(|selected| selected.rule_id != system_id));
        };

        let pure_base = full.excluding(&std::collections::HashSet::from([
            "迷迭香".to_string(),
            "黑键".to_string(),
        ]));
        for missing in [vec!["桑葚"], vec!["重岳", "令"]] {
            let operbox = pure_base.excluding(&std::collections::HashSet::from_iter(
                missing.into_iter().map(str::to_string),
            ));
            assert_closed(&build(&operbox), "human_fireworks_pure");
        }
        let mut low_wuyou_entries = pure_base.entries.clone();
        low_wuyou_entries
            .iter_mut()
            .filter(|entry| entry.name == "乌有")
            .for_each(|entry| {
                entry.elite = 0;
                entry.level = 1;
            });
        assert_closed(
            &build(&OperBox::from_entries(low_wuyou_entries).unwrap()),
            "human_fireworks_pure",
        );

        for missing in ["重岳", "令"] {
            let operbox = full.excluding(&std::collections::HashSet::from([missing.to_string()]));
            let plan = build(&operbox);
            assert_closed(&plan, "human_fireworks_perception");
            assert!(
                !plan.excluded_operators.contains(&"桑葚".to_string()),
                "感知附带分支不成立时不得把桑葚排除出普通候选"
            );
        }
        assert_closed(
            &build(&full.excluding(&std::collections::HashSet::from(["乌有".to_string()]))),
            "human_fireworks_perception",
        );
        let mut low_wuyou_entries = full.entries.clone();
        low_wuyou_entries
            .iter_mut()
            .filter(|entry| entry.name == "乌有")
            .for_each(|entry| {
                entry.elite = 0;
                entry.level = 1;
            });
        assert_closed(
            &build(&OperBox::from_entries(low_wuyou_entries).unwrap()),
            "human_fireworks_perception",
        );

        let no_mulberry = full.excluding(&std::collections::HashSet::from(["桑葚".to_string()]));
        let perception_without_mulberry = build(&no_mulberry);
        assert!(perception_without_mulberry
            .selected_rules
            .iter()
            .any(|selected| selected.rule_id == "rosemary_perception"));
        assert!(!perception_without_mulberry
            .registry_system_ids()
            .contains(&"human_fireworks_pure"));
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
        assert!(plan
            .registry_claims
            .iter()
            .all(|c| c.system_id != "pinus_sylvestris"));
        let pinus_anchors: Vec<_> = plan
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == "pinus_sylvestris")
            .collect();
        assert_eq!(pinus_anchors.len(), 5);
        assert!(
            pinus_anchors
                .iter()
                .filter(|a| a.recipe == Some(crate::types::RecipeKind::BattleRecord))
                .count()
                == 3
        );
        assert!(plan.shift_binds.iter().any(|bind| {
            ["焰尾", "薇薇安娜", "灰毫", "远牙", "野鬃"]
                .iter()
                .all(|name| bind.operators.iter().any(|op| op == name))
                && bind.on_shifts == 2
                && bind.off_shifts == 1
        }));
    }

    #[test]
    fn build_plan_peak_ideal_e2_does_not_registry_claim_rosemary() {
        // 迷迭香感知链走声明式规则，不再作为 registry 体系；
        // 核心、producer、约束和 bind 汇入统一 plan。
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
        // 声明式规则与兼容 registry 汇合到统一 AssignmentPlan。
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
        // 两名硬核心都必须作为 required anchor 汇入 plan。
        assert!(
            plan.anchors.iter().any(|a| a.operator == "迷迭香"
                && a.facility == crate::layout::blueprint::FacilityKind::Factory),
            "应含迷迭香制造 anchor: {:?}",
            plan.anchors
        );
        assert!(
            plan.anchors.iter().any(|a| a.operator == "黑键"
                && a.facility == crate::layout::blueprint::FacilityKind::TradePost),
            "应含黑键贸易 anchor: {:?}",
            plan.anchors
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
        assert!(plan.selected_rules.iter().any(|selected| {
            selected.rule_id == "rosemary_perception"
                && [
                    "recruit_refresh",
                    "high_perception",
                    "recruit_refresh_zilan",
                ]
                .contains(&selected.alternative_id.as_str())
        }));
        if operbox.owns("塑心") {
            assert!(plan.anchors.iter().any(|a| a.operator == "塑心"
                && a.facility == crate::layout::blueprint::FacilityKind::Dormitory));
        }
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
        // 不拥有迷迭香/黑键时声明式规则不激活，统一 plan 不含 Rosemary 片段。
        use crate::operbox::OperBoxEntry;
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(vec![OperBoxEntry {
            id: "f".into(),
            name: "芬".into(),
            elite: 1,
            level: 1,
            own: true,
            potential: 0,
            rarity: 3,
        }])
        .unwrap();
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
    fn build_plan_peak_ideal_e2_requires_blackkey_trade_anchor() {
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
        assert!(plan.anchors.iter().any(|anchor| {
            anchor.operator == "黑键" && anchor.facility == crate::layout::FacilityKind::TradePost
        }));
    }

    #[test]
    fn build_plan_does_not_reserve_docus_trade_teammates() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        for name in ["但书", "八幡海铃", "伺夜", "贝洛内"] {
            assert!(operbox.owns(name), "ideal E2 fixture must own {name}");
        }
        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        for forbidden in ["docus_syracusa", "syracusa_pair", "syracusa_cross_station"] {
            assert!(
                plan.registry_claims
                    .iter()
                    .all(|claim| claim.system_id != forbidden),
                "叙拉古成员应留给自然中枢/贸易搜索，不得认领 {forbidden}: {:?}",
                plan.registry_claims
            );
        }
    }

    #[test]
    fn failed_generic_automation_does_not_reactivate_legacy_registry() {
        let blueprint =
            BaseBlueprint::load(&crate::skill_table::data_path("layout/252.json").unwrap())
                .unwrap();
        let operbox =
            OperBox::load(&crate::operbox::default_operbox_full_e2_path().unwrap()).unwrap();
        let mut occupied_control = BaseAssignment::default();
        occupied_control.set_room(
            "control",
            (0..5)
                .map(|index| crate::layout::AssignedOperator::new(format!("occupied-{index}"), 0))
                .collect(),
        );

        let plan = build_plan(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &occupied_control,
            &std::collections::HashSet::new(),
        )
        .unwrap();

        assert!(plan
            .selected_rules
            .iter()
            .all(|selected| selected.rule_id != "automation_group"));
        assert!(plan.registry_claims.iter().all(|claim| {
            !matches!(
                claim.system_id.as_str(),
                "automation_group" | "gongsun_greyy2_power_line"
            )
        }));
        assert!(plan.activated.iter().all(|system| {
            !matches!(
                system.system_id.as_str(),
                "automation_group" | "gongsun_greyy2_power_line"
            )
        }));
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
        for name in ["但书", "八幡海铃", "伺夜", "贝洛内"] {
            assert!(operbox.owns(name), "ideal E2 fixture must own {name}");
        }
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
            executed.used.contains("但书"),
            "多贸易站但书是 required core"
        );
        assert!(
            executed.used.contains("八幡海铃"),
            "高刷新路径成员必须由 plan 实际进编"
        );
        for name in ["伺夜", "贝洛内"] {
            assert!(
                !executed.used.contains(name),
                "execute_plan 不应提前占用贸易自由搜索干员 {name}"
            );
        }
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
                elite: 1,
                level: 1,
                own: true,
                potential: 0,
                rarity: 3,
            },
        ])
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
