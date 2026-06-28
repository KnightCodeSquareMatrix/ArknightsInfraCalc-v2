use std::collections::HashSet;

mod commit;
mod control_fill;
mod manufacture_fill;
mod pipeline;
mod power_fill;
mod producer_fill;
mod run;
mod team_fill;
mod trade_fill;

pub(crate) use control_fill::assign_control;
pub use power_fill::{assign_power_rooms, assign_power_stations};
pub use team_fill::{assign_team_gamma_half, assign_team_producer_rooms};
pub use trade_fill::blackkey_witch_same_trade_room;
use trade_fill::skip_trade_core_registry_systems;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{BaseBlueprint, FacilityKind};
use crate::layout::orchestrate::{build_plan, AssignmentPlan};
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::{explain_registry_systems, SlotFillMode, SystemExplainReport};
use crate::operbox::OperBox;
use crate::pool::{compile_operator_atoms, ManuPoolEntry, TradePoolEntry};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

#[derive(Debug, Clone)]
pub struct AssignBaseOptions {
    pub top_k: usize,
    pub mood: f64,
    pub shift_hours: f64,
    /// 中枢分配时跳过 standalone_roster 白名单过滤（轮换编排中体系绑定干员可能不在白名单内）。
    pub skip_standalone_control: bool,
}

impl Default for AssignBaseOptions {
    fn default() -> Self {
        Self {
            top_k: 20,
            mood: 24.0,
            shift_hours: 24.0,
            skip_standalone_control: false,
        }
    }
}

/// 全基建单班进驻编制：producer 落位 → resolve → consumer 搜 + `used` 顺序认领。
pub fn assign_base_greedy(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<BaseAssignment> {
    assign_shift(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
    )
}

/// `assign_shift` 完整输出：编制 + 编排计划（轮换层只读 plan，不重跑 `build_plan`）。
#[derive(Debug, Clone)]
pub struct AssignShiftResult {
    pub assignment: BaseAssignment,
    pub plan: AssignmentPlan,
}

/// 单班进驻；`seed` 非空时保留已钉死房间（中枢/宿舍），仅补贸易/制造/发电。
pub fn assign_shift(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<BaseAssignment> {
    Ok(
        assign_shift_with_plan(blueprint, operbox, instances, table, options, mode, seed)?
            .assignment,
    )
}

/// 解释主路径 `assign_shift` 会如何认领 registry systems。
///
/// 这会套用与实际排班一致的 trade role registry skip 规则，但不执行设施搜索或效率结算。
pub fn explain_assignment_systems(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> SystemExplainReport {
    let mut skip_system_ids = HashSet::new();
    skip_trade_core_registry_systems(&mut skip_system_ids);
    let used = seed.operator_names();
    explain_registry_systems(blueprint, operbox, mode, seed, &used, &skip_system_ids)
}

/// 从编排计划提取 claimed 干员名并按 tier 标注池条目。
pub(super) fn tag_pool_from_plan<T: crate::pool::HasName + crate::pool::TierTagged>(
    plan: &AssignmentPlan,
    pool: &mut crate::pool::PoolCore<T>,
) {
    for claim in &plan.registry_claims {
        let mut names = HashSet::new();
        for slot in &claim.slots {
            for op in &slot.operators {
                names.insert(op.name.clone());
            }
        }
        pool.tag_tier(&names, claim.tier);
    }
}

pub(super) fn inject_search_anchor_pool_entries(
    plan: &AssignmentPlan,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    trade_pool: &mut crate::pool::TradePool,
    manu_pool: &mut crate::pool::ManuPool,
) {
    for claim in &plan.registry_claims {
        for slot in &claim.slots {
            if slot.fill != SlotFillMode::Search {
                continue;
            }
            for op in &slot.operators {
                match slot.facility {
                    FacilityKind::TradePost => {
                        inject_trade_anchor(op.name.as_str(), operbox, instances, table, trade_pool)
                    }
                    FacilityKind::Factory => {
                        inject_manu_anchor(op.name.as_str(), operbox, instances, table, manu_pool)
                    }
                    _ => {}
                }
            }
        }
    }
}

fn inject_trade_anchor(
    name: &str,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    pool: &mut crate::pool::TradePool,
) {
    if pool.entry(name).is_some() {
        return;
    }
    let Some(progress) = operbox.progress_of(name) else {
        return;
    };
    let tier = PromotionTier::from_progress(progress);
    let buff_ids = instances.resolve_trade_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return;
    }
    pool.entries.push(TradePoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids: buff_ids.clone(),
        tags: instances.tags_for(name, tier),
        compiled_atoms: compile_operator_atoms(&buff_ids, table),
        flat_eff_hint: 0.0,
        is_mechanic: true,
        tier: crate::layout::tier::OperatorTier::CrossStation,
    });
}

fn inject_manu_anchor(
    name: &str,
    operbox: &OperBox,
    instances: &OperatorInstances,
    _table: &SkillTable,
    pool: &mut crate::pool::ManuPool,
) {
    if pool.entry(name).is_some() {
        return;
    }
    let Some(progress) = operbox.progress_of(name) else {
        return;
    };
    let tier = PromotionTier::from_progress(progress);
    let buff_ids = instances.resolve_manufacture_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return;
    }
    pool.entries.push(ManuPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids,
        tags: instances.tags_for(name, tier),
        flat_eff_hint: 0.0,
        has_l2_delegate: false,
        tier: crate::layout::tier::OperatorTier::CrossStation,
    });
}

/// 同 [`assign_shift`]，额外返回编排 `AssignmentPlan`（peak 班供 αβγ 轮换只读）。
pub fn assign_shift_with_plan(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<AssignShiftResult> {
    assign_shift_with_plan_skip(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        seed,
        &HashSet::new(),
    )
}

/// 同 [`assign_shift_with_plan`]，额外允许跳过指定体系。
pub fn assign_shift_with_plan_skip(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &HashSet<String>,
) -> Result<AssignShiftResult> {
    blueprint.validate()?;

    let mut skip_system_ids = skip_system_ids.clone();
    skip_trade_core_registry_systems(&mut skip_system_ids);
    let plan = build_plan(blueprint, operbox, mode, seed, &skip_system_ids)?;

    let assignment = pipeline::run_shift_pipeline(
        blueprint, operbox, instances, table, options, mode, seed, &plan,
    )?;

    Ok(AssignShiftResult { assignment, plan })
}

/// 编制内所有上岗干员。
pub fn assignment_operator_names(assignment: &BaseAssignment) -> HashSet<String> {
    assignment.operator_names()
}

/// 贸易 / 制造 / 发电岗位干员（跨班互斥池）。
pub fn rotating_workers(assignment: &BaseAssignment, blueprint: &BaseBlueprint) -> HashSet<String> {
    let rotating_kinds = [
        FacilityKind::TradePost,
        FacilityKind::Factory,
        FacilityKind::PowerPlant,
    ];
    let mut names = HashSet::new();
    for room in &assignment.rooms {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !rotating_kinds.contains(&bp.kind) {
            continue;
        }
        for op in &room.operators {
            names.insert(op.name.clone());
        }
    }
    names
}

/// 宿舍 + 办公室感知 producer（三班钉死，从高峰班拷贝）。
///
/// 中枢在 `schedule_team_rotation` 中按 αβγ 队伍轮休重分配，不在这里钉死。
pub fn pinned_assignment(assignment: &BaseAssignment, blueprint: &BaseBlueprint) -> BaseAssignment {
    let mut pinned = BaseAssignment::default();
    for room in &assignment.rooms {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !matches!(bp.kind, FacilityKind::Dormitory | FacilityKind::Office) {
            continue;
        }
        if room.operators.is_empty() {
            continue;
        }
        pinned.set_room_assignment(room.clone());
    }
    pinned
}

#[cfg(test)]
mod tests {
    use super::commit::{commit_manu_room, manu_hit_names};
    use super::manufacture_fill::{
        gongsun_gold_manu_anchors_ready, manufacture_candidate_pool_for_demand, pick_manu_hit,
        try_assign_gongsun_gold_manu_team, QINGLIU_RENEWABLE_ENERGY_BUFF,
    };
    use super::producer_fill::{
        assign_sphinx_urrbian_dorm_anchor, cleanup_unused_sphinx_urrbian_dorm_anchor,
    };
    use super::*;
    use crate::layout::resolve::resolve_base;
    use std::collections::HashSet;

    use crate::instances::default_instances_path;
    use crate::layout::shift::AssignShiftMode;
    use crate::layout::{AssignedOperator, BaseBlueprint, RoomId, RoomProduct};
    use crate::manufacture::input::ManuSearchRecipeMode;
    use crate::operbox::{
        default_operbox_full_e2_path, default_operbox_gongsun_path, OperBox, OperBoxEntry,
    };
    use crate::pool::ManuPool;
    use crate::search::{ManuSearchOptions, MATATABI_CONSUMER_NAME};
    use crate::skill_table::{default_skill_table_path, SkillTable};
    use crate::types::RecipeKind;

    const GONGSUN_GOLD_MANU_TEAM: [&str; 3] = ["清流", "温蒂", "森蚺"];

    fn fixtures() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
    }

    fn operbox_from_names(entries: &[(&str, u8, u8)]) -> OperBox {
        OperBox::from_entries(
            entries
                .iter()
                .enumerate()
                .map(|(i, (name, elite, rarity))| OperBoxEntry {
                    id: format!("test_{i:03}"),
                    name: (*name).to_string(),
                    elite: *elite,
                    level: 1,
                    own: true,
                    potential: 1,
                    rarity: *rarity,
                })
                .collect(),
        )
    }

    fn manu_pool_entry_with_progress(
        name: &str,
        buff_ids: &[&str],
        progress: crate::roster::OperatorProgress,
    ) -> crate::pool::ManuPoolEntry {
        crate::pool::ManuPoolEntry {
            name: name.to_string(),
            elite: progress.elite,
            progress,
            buff_ids: buff_ids.iter().map(|id| (*id).to_string()).collect(),
            tags: vec![],
            flat_eff_hint: 0.0,
            has_l2_delegate: false,
            tier: crate::layout::tier::OperatorTier::Standalone,
        }
    }

    fn manu_pool_entry(name: &str, buff_ids: &[&str]) -> crate::pool::ManuPoolEntry {
        manu_pool_entry_with_progress(
            name,
            buff_ids,
            crate::roster::OperatorProgress::elite_only(2),
        )
    }

    fn manu_pool_entry_tier0(name: &str, buff_ids: &[&str]) -> crate::pool::ManuPoolEntry {
        manu_pool_entry_with_progress(
            name,
            buff_ids,
            crate::roster::OperatorProgress::elite_only(0),
        )
    }

    #[test]
    fn sphinx_urrbian_anchor_places_urrbian_in_dorm_base_workforce() {
        let (blueprint, _, instances, table) = fixtures();
        let operbox = operbox_from_names(&[("深巡", 2, 6), ("乌尔比安", 2, 6)]);
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assign_sphinx_urrbian_dorm_anchor(&blueprint, &operbox, &mut assignment, &mut used);

        assert!(used.contains("乌尔比安"));
        assert!(
            assignment
                .rooms
                .iter()
                .any(|room| room.room_id.0.starts_with("dorm_")
                    && room.operators.iter().any(|op| op.name == "乌尔比安")),
            "深巡存在时应把乌尔比安作为宿舍进驻锚点"
        );
        let layout = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap()
        .layout;
        assert!(
            layout.base_workforce.iter().any(|name| name == "乌尔比安"),
            "宿舍进驻的乌尔比安应进入 OperatorInBase 判定"
        );
    }

    #[test]
    fn sphinx_urrbian_anchor_is_removed_when_sphinx_not_assigned() {
        let (blueprint, _, _, _) = fixtures();
        let operbox = operbox_from_names(&[("深巡", 2, 6), ("乌尔比安", 2, 6)]);
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assign_sphinx_urrbian_dorm_anchor(&blueprint, &operbox, &mut assignment, &mut used);
        cleanup_unused_sphinx_urrbian_dorm_anchor(&blueprint, &mut assignment, &mut used);

        assert!(!used.contains("乌尔比安"));
        assert!(
            !assignment.rooms.iter().any(|room| {
                room.room_id.0.starts_with("dorm_")
                    && room.operators.iter().any(|op| op.name == "乌尔比安")
            }),
            "深巡未进贸易站时，不应保留乌尔比安宿舍锚点"
        );
    }

    #[test]
    fn sphinx_urrbian_anchor_stays_when_sphinx_is_assigned() {
        let (blueprint, _, _, _) = fixtures();
        let operbox = operbox_from_names(&[("深巡", 2, 6), ("乌尔比安", 2, 6)]);
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assign_sphinx_urrbian_dorm_anchor(&blueprint, &operbox, &mut assignment, &mut used);
        assignment.set_room(
            RoomId::from("trade_1"),
            vec![AssignedOperator::new("深巡", 2)],
        );
        used.insert("深巡".to_string());
        cleanup_unused_sphinx_urrbian_dorm_anchor(&blueprint, &mut assignment, &mut used);

        assert!(used.contains("乌尔比安"));
        assert!(
            assignment.rooms.iter().any(|room| {
                room.room_id.0.starts_with("dorm_")
                    && room.operators.iter().any(|op| op.name == "乌尔比安")
            }),
            "深巡进贸易站时，应保留乌尔比安作为 OperatorInBase 锚点"
        );
    }

    #[test]
    fn gongsun_gold_fixed_team_requires_wendy_tier_up() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let pool = ManuPool {
            entries: vec![
                manu_pool_entry("清流", &[QINGLIU_RENEWABLE_ENERGY_BUFF]),
                manu_pool_entry_tier0("温蒂", &["manu_prod_spd&power[010]"]),
                manu_pool_entry_tier0("冬时", &["manu_prod_spd&manu[000]"]),
            ],
            skipped: vec![],
        };
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assert!(!gongsun_gold_manu_anchors_ready(&pool));
        try_assign_gongsun_gold_manu_team(&blueprint, &mut assignment, &pool, &mut used).unwrap();

        assert!(
            assignment.rooms.is_empty(),
            "温蒂未解锁仿生海龙时不应强制清流+温蒂+冬时金线: {:?}",
            assignment.rooms
        );
    }

    #[test]
    fn manufacture_candidate_extension_picks_ramp_skills_over_low_standalone_pool() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = ManuPool {
            entries: vec![
                manu_pool_entry_tier0("褐果", &["manu_prod_spd[000]"]),
                manu_pool_entry_tier0("雪猎", &["manu_prod_spd&limit&cost[101]"]),
                manu_pool_entry("卡达", &["manu_formula_cost[000]"]),
                manu_pool_entry("史都华德", &["manu_prod_spd[010]"]),
                manu_pool_entry("芬", &["manu_prod_spd_addition[030]"]),
                manu_pool_entry("克洛丝", &["manu_prod_spd_addition[040]"]),
                manu_pool_entry("泡普卡", &["manu_prod_spd&limit&cost[010]"]),
            ],
            skipped: vec![],
        };
        let candidate_pool = manufacture_candidate_pool_for_demand(&pool, &HashSet::new(), 3);
        assert!(
            candidate_pool.entries.len() < pool.entries.len(),
            "manufacture candidate extension should not fall back to the full pool"
        );
        assert!(candidate_pool.entry("芬").is_some());
        assert!(candidate_pool.entry("克洛丝").is_some());

        let hit = pick_manu_hit(
            &candidate_pool,
            &table,
            ManuSearchOptions {
                recipe_mode: ManuSearchRecipeMode::Single(RecipeKind::Gold),
                top_k: 20,
                ..Default::default()
            },
            &HashSet::new(),
            20,
        )
        .unwrap();
        let names: HashSet<_> = manu_hit_names(&hit).iter().map(String::as_str).collect();
        assert!(names.contains("芬"), "hit={hit:?}");
        assert!(names.contains("克洛丝"), "hit={hit:?}");
        assert!(
            !names.contains("褐果") && !names.contains("卡达"),
            "低效白名单组合不应压过扩展候选池中的爬升技能: {hit:?}"
        );
        assert!(hit.breakdown.prod_skill > 50.0, "hit={hit:?}");
    }

    #[test]
    fn manufacture_candidate_pool_stays_primary_when_standalone_can_fill_rooms() {
        let pool = ManuPool {
            entries: vec![
                manu_pool_entry("槐琥", &["manu_prod_spd[000]"]),
                manu_pool_entry("雪猎", &["manu_prod_spd&limit&cost[101]"]),
                manu_pool_entry("至简", &["manu_prod_spd[000]"]),
                manu_pool_entry("芬", &["manu_prod_spd_addition[030]"]),
                manu_pool_entry("克洛丝", &["manu_prod_spd_addition[040]"]),
            ],
            skipped: vec![],
        };

        let candidate_pool = manufacture_candidate_pool_for_demand(&pool, &HashSet::new(), 3);
        assert!(candidate_pool.entry("槐琥").is_some());
        assert!(candidate_pool.entry("雪猎").is_some());
        assert!(candidate_pool.entry("至简").is_some());
        assert!(candidate_pool.entry("芬").is_none());
        assert!(candidate_pool.entry("克洛丝").is_none());
    }

    #[test]
    fn manufacture_candidate_pool_falls_back_to_full_pool_when_expansion_still_lacks_capacity() {
        let pool = ManuPool {
            entries: vec![
                manu_pool_entry("褐果", &["manu_prod_spd[000]"]),
                manu_pool_entry("雪猎", &["manu_prod_spd&limit&cost[101]"]),
                manu_pool_entry("卡达", &["manu_formula_cost[000]"]),
                manu_pool_entry("芬", &["manu_prod_spd_addition[030]"]),
                manu_pool_entry("低效非候选A", &["manu_prod_spd[999]"]),
                manu_pool_entry("低效非候选B", &["manu_prod_spd[998]"]),
            ],
            skipped: vec![],
        };

        let candidate_pool = manufacture_candidate_pool_for_demand(&pool, &HashSet::new(), 6);
        assert_eq!(candidate_pool.entries.len(), pool.entries.len());
        assert!(candidate_pool.entry("低效非候选A").is_some());
        assert!(candidate_pool.entry("低效非候选B").is_some());
    }

    #[test]
    fn manufacture_candidate_pool_does_not_fallback_automation_ops_as_general_pieces() {
        let pool = ManuPool {
            entries: vec![
                manu_pool_entry("褐果", &["manu_prod_spd[000]"]),
                manu_pool_entry("雪猎", &["manu_prod_spd&limit&cost[101]"]),
                manu_pool_entry("卡达", &["manu_formula_cost[000]"]),
                manu_pool_entry("冬时", &["manu_prod_spd&manu[100]"]),
                manu_pool_entry("温蒂", &["manu_prod_spd&power[020]"]),
                manu_pool_entry("低效非候选A", &["manu_prod_spd[999]"]),
                manu_pool_entry("低效非候选B", &["manu_prod_spd[998]"]),
            ],
            skipped: vec![],
        };

        let candidate_pool = manufacture_candidate_pool_for_demand(&pool, &HashSet::new(), 6);
        assert!(pool.entry("冬时").is_some(), "自动化组仍可从原池显式取冬时");
        assert!(pool.entry("温蒂").is_some(), "自动化组仍可从原池显式取温蒂");
        assert!(
            candidate_pool.entry("冬时").is_none(),
            "普通制造候选池容量兜底也不应带回冬时"
        );
        assert!(
            candidate_pool.entry("温蒂").is_none(),
            "普通制造候选池容量兜底也不应带回温蒂"
        );
        assert!(candidate_pool.entry("低效非候选A").is_some());
        assert!(candidate_pool.entry("低效非候选B").is_some());
    }

    #[test]
    fn commit_manu_room_stores_efficiency_snapshot_and_progress() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let progress = crate::roster::OperatorProgress::new(1, 55, 3);
        let pool = ManuPool {
            entries: vec![
                crate::pool::ManuPoolEntry {
                    name: "芬".to_string(),
                    elite: progress.elite,
                    progress,
                    buff_ids: vec!["manu_prod_spd_addition[030]".to_string()],
                    tags: vec![],
                    flat_eff_hint: 0.0,
                    has_l2_delegate: false,
                    tier: crate::layout::tier::OperatorTier::Standalone,
                },
                manu_pool_entry("克洛丝", &["manu_prod_spd_addition[040]"]),
                manu_pool_entry("泡普卡", &["manu_prod_spd&limit&cost[010]"]),
            ],
            skipped: vec![],
        };
        let hit = pick_manu_hit(
            &pool,
            &table,
            ManuSearchOptions {
                recipe_mode: ManuSearchRecipeMode::Single(RecipeKind::Gold),
                top_k: 20,
                ..Default::default()
            },
            &HashSet::new(),
            20,
        )
        .unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        commit_manu_room(
            &mut assignment,
            &RoomId::from("manu_1"),
            &hit,
            &pool,
            &mut used,
        )
        .unwrap();

        let room = assignment
            .room_assignment(&RoomId::from("manu_1"))
            .expect("room committed");
        let snapshot = room.efficiency.as_ref().expect("manufacture snapshot");
        assert_eq!(snapshot.manu_prod_total, hit.breakdown.prod_total);
        assert_eq!(snapshot.manu_prod_skill, hit.breakdown.prod_skill);
        let fen = room
            .operators
            .iter()
            .find(|op| op.name == "芬")
            .expect("fen committed");
        assert_eq!(fen.level, 55);
        assert_eq!(fen.rarity, 3);
        assert_eq!(fen.tier(), crate::tier::PromotionTier::TierUp);
    }

    #[test]
    fn assign_ideal_e2_peak_claims_syracusa_pair_system() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("八幡海铃")
            || !operbox.owns("但书")
            || !operbox.owns("伺夜")
            || !operbox.owns("贝洛内")
        {
            return;
        }
        let assignment = assign_base_greedy(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
        )
        .unwrap();
        // 叙拉古同站 meta 由 registry 锚定；但书同站三级组合只作为 shortcut 命中。
        let syracusa_room = assignment.rooms.iter().find(|r| {
            r.operators.iter().any(|o| o.name == "伺夜")
                && r.operators.iter().any(|o| o.name == "贝洛内")
        });
        assert!(syracusa_room.is_some(), "伺夜+贝洛内应锚定同一贸易站");

        let control_ops = assignment.control_operators();
        let control: HashSet<_> = control_ops.iter().map(|o| o.name.as_str()).collect();
        assert!(control.contains("八幡海铃"), "control: {:?}", control);
        assert!(
            control.contains("斩业星熊") && control.contains("诗怀雅"),
            "control: {:?}",
            control
        );
        assert!(
            !control.contains("三角初华") && !control.contains("若叶睦"),
            "钉死后补位应为公招/心情而非 MyGO 热情链: {:?}",
            control
        );
        assert!(
            control.contains("薇薇安娜") || control.contains("焰尾"),
            "应有中枢心情回复补位: {:?}",
            control
        );
        assert!(
            !control.contains("火龙S黑角") && !control.contains("麒麟R夜刀"),
            "高峰无调查团时不应因木天蓼选怪猎中枢: {:?}",
            control
        );
    }

    #[test]
    fn assign_252_keeps_syracusa_pair_and_puts_docus_in_lv2_trade() {
        let blueprint =
            BaseBlueprint::load(&crate::skill_table::data_path("layout/252.json").unwrap())
                .unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("fixtures/243/operbox_full_e2.json").unwrap(),
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in ["八幡海铃", "但书", "伺夜", "贝洛内"] {
            if !operbox.owns(name) {
                return;
            }
        }

        let assignment = assign_base_greedy(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
        )
        .unwrap();

        let docus_room = assignment
            .rooms
            .iter()
            .find(|r| r.operators.iter().any(|o| o.name == "但书"))
            .expect("但书应被排入贸易站");
        let docus_blueprint = blueprint.room(&docus_room.room_id).unwrap();
        assert_eq!(docus_blueprint.kind, FacilityKind::TradePost);
        assert_eq!(
            docus_blueprint.level, 2,
            "但书应优先进入二级贸易站: {:?}",
            docus_room
        );

        let syracusa_room = assignment
            .rooms
            .iter()
            .find(|r| {
                r.operators.iter().any(|o| o.name == "伺夜")
                    && r.operators.iter().any(|o| o.name == "贝洛内")
            })
            .expect("伺夜+贝洛内应同站");
        let syracusa_blueprint = blueprint.room(&syracusa_room.room_id).unwrap();
        assert_eq!(syracusa_blueprint.kind, FacilityKind::TradePost);
        assert_eq!(
            syracusa_blueprint.level, 3,
            "伺夜+贝洛内应保留在三级贸易站: {:?}",
            syracusa_room
        );
        assert!(
            !syracusa_room.operators.iter().any(|o| o.name == "但书"),
            "252 中但书不应抢占伺夜+贝洛内三级站: {:?}",
            syracusa_room
        );
    }

    #[test]
    fn assign_full_e2_without_top_three_trade_cores_uses_vina_before_karlan() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = operbox.excluding(&HashSet::from([
            "龙舌兰".to_string(),
            "可露希尔".to_string(),
            "但书".to_string(),
        ]));
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in ["戴菲恩", "推进之王", "摩根", "维娜·维多利亚", "灵知", "孑"]
        {
            if !operbox.owns(name) {
                return;
            }
        }

        let assignment = assign_base_greedy(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
        )
        .unwrap();

        let control: HashSet<_> = assignment
            .control_operators()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert!(control.contains("戴菲恩"), "control: {:?}", control);

        let vina_room = assignment.rooms.iter().find(|r| {
            ["推进之王", "摩根", "维娜·维多利亚"]
                .iter()
                .all(|name| r.operators.iter().any(|o| o.name == *name))
        });
        assert!(
            vina_room.is_some(),
            "推王摩根维娜应优先于灵知孑上站: {:?}",
            assignment.rooms
        );
    }

    #[test]
    fn assign_243_use_this_has_no_duplicate_operators() {
        let (blueprint, operbox, instances, table) = fixtures();
        let assignment = assign_shift_with_plan_skip(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
        )
        .unwrap()
        .assignment;
        let mut seen = HashSet::new();
        for room in &assignment.rooms {
            for op in &room.operators {
                assert!(seen.insert(op.name.clone()), "duplicate {}", op.name);
            }
        }
    }

    #[test]
    fn assign_full_e2_blackkey_never_colocated_with_witch() {
        use crate::operbox::default_operbox_full_e2_path;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("黑键") || !operbox.owns("巫恋") {
            return;
        }
        let assignment = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        assert!(
            !blackkey_witch_same_trade_room(&assignment, &blueprint),
            "243 双贸：黑键与巫恋不得同房"
        );
        let report = crate::schedule::schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        for shift in &report.shifts {
            assert!(
                !blackkey_witch_same_trade_room(&shift.assignment, &blueprint),
                "team-rotation shift {} 黑键与巫恋同房",
                shift.index + 1
            );
        }
    }

    #[test]
    fn team_rotation_partial_trade_meta_keeps_docus_closure_and_witch() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_from_names(&[
            ("但书", 2, 5),
            ("可露希尔", 2, 6),
            ("巫恋", 2, 5),
            ("龙舌兰", 2, 5),
            ("卡夫卡", 1, 5),
            ("德克萨斯", 2, 5),
            ("拉普兰德", 2, 5),
            ("古米", 2, 4),
            ("夜刀", 0, 2),
            ("石英", 2, 4),
            ("慕斯", 2, 4),
            ("空弦", 2, 6),
            ("能天使", 2, 6),
            ("空", 2, 5),
            ("八幡海铃", 2, 5),
            ("灵知", 2, 6),
            ("斩业星熊", 2, 6),
            ("诗怀雅", 2, 5),
            ("Mon3tr", 2, 6),
            ("凯尔希", 2, 6),
            ("明椒", 0, 5),
            ("望", 2, 5),
            ("薇薇安娜", 2, 6),
            ("阿米娅", 2, 5),
            ("格雷伊", 2, 4),
            ("烛煌", 2, 6),
            ("澄闪", 2, 6),
            ("雷蛇", 2, 5),
            ("海霓", 2, 5),
            ("清流", 2, 4),
            ("砾", 2, 4),
            ("苍苔", 2, 5),
            ("白雪", 2, 4),
            ("红豆", 2, 4),
            ("酒神", 2, 6),
            ("褐果", 2, 4),
            ("卡达", 2, 4),
            ("槐琥", 2, 5),
            ("铅踝", 2, 5),
            ("雪猎", 2, 5),
            ("斑点", 2, 3),
            ("乌尔比安", 2, 6),
            ("斯卡蒂", 1, 6),
            ("冬时", 2, 5),
            ("幽灵鲨", 2, 5),
            ("安哲拉", 0, 5),
            ("水月", 2, 6),
            ("炎熔", 2, 3),
            ("艾雅法拉", 2, 6),
            ("阿罗玛", 0, 5),
        ]);
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let report = crate::schedule::schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(
            report.shifts.iter().any(|shift| trade_room_contains(
                &shift.assignment,
                &blueprint,
                &["但书"]
            )),
            "partial meta account should keep docus core in rotation"
        );
        assert!(
            report.shifts.iter().any(|shift| {
                trade_room_contains(&shift.assignment, &blueprint, &["可露希尔"])
            }),
            "partial meta account should keep closure core in rotation"
        );
        assert!(
            report.shifts.iter().any(|shift| {
                trade_room_contains(&shift.assignment, &blueprint, &["巫恋", "龙舌兰"])
            }),
            "partial meta account should keep witch + tequila fallback in rotation"
        );
    }

    fn trade_room_contains(
        assignment: &BaseAssignment,
        blueprint: &BaseBlueprint,
        names: &[&str],
    ) -> bool {
        assignment.rooms.iter().any(|room| {
            blueprint
                .rooms
                .iter()
                .any(|bp| bp.id == room.room_id && bp.kind == FacilityKind::TradePost)
                && names
                    .iter()
                    .all(|name| room.operators.iter().any(|op| op.name == *name))
        })
    }

    #[test]
    fn assign_full_e2_peak_manu_teams_match_gongsun() {
        use crate::manufacture::{solve_manufacture, ManuRoomInput};
        use crate::operbox::default_operbox_full_e2_path;
        use crate::pool::build_manufacture_pool;
        use std::sync::Arc;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("清流") || !operbox.owns("迷迭香") {
            return;
        }
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 30,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        let durin = operbox.durin_dorm_planning_count(&instances);
        let resolved = resolve_base(
            &blueprint,
            &peak,
            Some(&instances),
            Some(&table),
            24.0,
            Some(durin),
        )
        .unwrap();

        let room_ops = |room_id: &str| -> Vec<String> {
            peak.operators_in(&RoomId::from(room_id))
                .iter()
                .map(|o| o.name.clone())
                .collect()
        };

        let gold_trio: HashSet<_> = GONGSUN_GOLD_MANU_TEAM.iter().map(|s| *s).collect();
        let gold_room = peak.rooms.iter().find(|r| {
            blueprint.rooms.iter().any(|b| {
                b.id == r.room_id
                    && b.kind == FacilityKind::Factory
                    && matches!(
                        b.product.as_ref(),
                        Some(RoomProduct::Factory {
                            recipe: RecipeKind::Gold
                        })
                    )
            }) && gold_trio
                .iter()
                .all(|n| r.operators.iter().any(|o| o.name == *n))
        });
        assert!(
            gold_room.is_some(),
            "金线应有清流+温蒂+森蚺，实际制造编制: {:?}",
            peak.rooms
                .iter()
                .filter(|r| {
                    blueprint
                        .rooms
                        .iter()
                        .any(|b| b.id == r.room_id && b.kind == FacilityKind::Factory)
                })
                .collect::<Vec<_>>()
        );

        let br_winter = room_ops("manu_2");
        assert!(
            !br_winter.contains(&"清流".to_string()),
            "经验线 manu_2 不应占清流 trio: {br_winter:?}"
        );

        let pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        let mk = |names: &[&str]| -> Vec<_> {
            names
                .iter()
                .map(|n| pool.entry(n).unwrap().to_manu_operator())
                .collect()
        };
        let gold_room_resolved = resolved
            .manu_rooms
            .iter()
            .find(|r| {
                gold_trio
                    .iter()
                    .all(|n| r.operators.iter().any(|o| o.name == *n))
            })
            .expect("resolved gold trio");
        let gold_skill = solve_manufacture(
            &ManuRoomInput {
                level: gold_room_resolved.level,
                operators: mk(&GONGSUN_GOLD_MANU_TEAM),
                active_recipe: RecipeKind::Gold,
                mood: 24.0,
                layout: Arc::new(gold_room_resolved.layout.clone()),
            },
            &table,
        )
        .unwrap()
        .prod_skill;
        assert!(
            (gold_skill - 140.0).abs() <= 1.0,
            "清流金线纸面约 140，got {gold_skill:.1}"
        );
    }

    #[test]
    fn assign_peak_fills_automation_gold_line_with_dongshi_without_senxi() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let excluded = HashSet::from(["森蚺".to_string()]);
        let operbox = operbox.excluding(&excluded);
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("清流") || !operbox.owns("温蒂") || !operbox.owns("冬时") {
            return;
        }

        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 30,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();

        let gold_room = peak.rooms.iter().find(|r| {
            blueprint.rooms.iter().any(|b| {
                b.id == r.room_id
                    && b.kind == FacilityKind::Factory
                    && matches!(
                        b.product.as_ref(),
                        Some(RoomProduct::Factory {
                            recipe: RecipeKind::Gold
                        })
                    )
            }) && ["清流", "温蒂", "冬时"]
                .iter()
                .all(|n| r.operators.iter().any(|o| o.name == *n))
        });
        assert!(
            gold_room.is_some(),
            "无森蚺时金线应补冬时，实际制造编制: {:?}",
            peak.rooms
                .iter()
                .filter(|r| {
                    blueprint
                        .rooms
                        .iter()
                        .any(|b| b.id == r.room_id && b.kind == FacilityKind::Factory)
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn assign_snhunt_control_gets_monhun_ops_when_owned() {
        let blueprint =
            BaseBlueprint::load(&crate::skill_table::data_path("layout/snhunt.json").unwrap())
                .unwrap();
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("火龙S黑角") || !operbox.owns("麒麟R夜刀") {
            return;
        }
        // 怪猎评估布局：须本班有调查团 consumer，木天蓼才计入中枢正分。
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "trade_1",
            vec![AssignedOperator::new(MATATABI_CONSUMER_NAME, 2)],
        );
        let assignment = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &seed,
        )
        .unwrap();
        let control = assignment.control_operators();
        let names: HashSet<_> = control.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains("火龙S黑角"));
        assert!(names.contains("麒麟R夜刀"));
    }
}
