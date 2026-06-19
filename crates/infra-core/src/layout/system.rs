//! 跨设施成套方案：小目录 + 贪心认领（`claim_base_systems`）。
//!
//! 数据：`data/base_systems.json`（来源：公孙长乐工具人表等固定组合）。
//! 在 `assign_shift` 开头认领，已占房间由后续设施贪心跳过。

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};
use crate::operbox::OperBox;
use crate::skill_table::{data_path, SkillTable};

use crate::layout::shift::AssignShiftMode;

/// 单个 registry slot 的已解析落位（`select_registry_systems` 产出）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegistrySlotClaim {
    pub room_id: RoomId,
    pub facility: FacilityKind,
    pub operators: Vec<AssignedOperator>,
}

/// `base_systems.json` 中已选体系的完整落位计划。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegistrySystemClaim {
    pub system_id: String,
    pub priority: i32,
    pub slots: Vec<RegistrySlotClaim>,
}

#[derive(Debug, Clone, Deserialize)]
struct BaseSystemsFile {
    #[serde(default)]
    control_manu_injectors: Vec<ControlManuInjectorDef>,
    systems: Vec<BaseSystemDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ControlManuInjectorDef {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub manu_all_pct: f64,
    pub operators: Vec<SystemOperatorSpec>,
    #[serde(default)]
    pub requires_monhun_peer: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BaseSystemDef {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub segment_id: Option<String>,
    #[serde(default)]
    pub exclusive_group: Option<String>,
    #[serde(default)]
    pub shift_modes: Vec<String>,
    pub slots: Vec<SystemSlotDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemSlotDef {
    pub facility: String,
    #[serde(default)]
    pub room_id: Option<String>,
    #[serde(default)]
    pub trade_role: Option<String>,
    pub operators: Vec<SystemOperatorSpec>,
    /// 可选 slot：缺房 / 缺干员时静默跳过，不导致整链不可行。
    /// 用于感知 producer（夕中枢、絮雨办公室、爱丽丝/车尔尼宿舍）等非核心位，
    /// 以及蓝图无该设施（如无办公室）的情形。核心位（黑键/迷迭香）保持必需。
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SystemOperatorSpec {
    Fixed(SystemOperatorFixed),
    PickOne(SystemOperatorPickOne),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOperatorFixed {
    pub name: String,
    #[serde(default)]
    pub elite: u8,
    /// 德克萨斯 E0/E2 企鹅物流分叉：精英上限（含）。
    #[serde(default)]
    pub max_elite: Option<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOperatorPickOne {
    pub pick_one: Vec<String>,
    #[serde(default)]
    pub elite: u8,
}

#[derive(Debug, Clone)]
struct ResolvedOperator {
    name: String,
    elite: u8,
}

struct BaseSystemsCache {
    systems: Vec<BaseSystemDef>,
}

static BASE_SYSTEMS_CACHE: OnceLock<Option<BaseSystemsCache>> = OnceLock::new();

pub fn load_base_systems(path: &Path) -> Result<BaseSystemsFile> {
    let raw = std::fs::read_to_string(path)?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::msg(format!("base_systems parse {}: {e}", path.display())))
}

pub fn default_base_systems_path() -> Result<std::path::PathBuf> {
    data_path("base_systems.json")
}

fn base_systems_cache() -> Option<&'static BaseSystemsCache> {
    BASE_SYSTEMS_CACHE
        .get_or_init(|| {
            let path = default_base_systems_path().ok()?;
            let file = load_base_systems(&path).ok()?;
            Some(BaseSystemsCache {
                systems: file.systems,
            })
        })
        .as_ref()
}

fn systems_by_priority(cache: &BaseSystemsCache) -> Vec<&BaseSystemDef> {
    let mut list: Vec<_> = cache.systems.iter().collect();
    list.sort_by(|a, b| b.priority.cmp(&a.priority));
    list
}

fn mode_allowed(system: &BaseSystemDef, mode: AssignShiftMode) -> bool {
    if system.shift_modes.is_empty() {
        return mode == AssignShiftMode::Peak;
    }
    let want = match mode {
        AssignShiftMode::Peak => "peak",
        AssignShiftMode::Recovery => "recovery",
    };
    system.shift_modes.iter().any(|m| m == want)
}

fn facility_kind(raw: &str) -> Option<FacilityKind> {
    match raw {
        "control" => Some(FacilityKind::ControlCenter),
        "trade_post" => Some(FacilityKind::TradePost),
        "factory" => Some(FacilityKind::Factory),
        "power_plant" => Some(FacilityKind::PowerPlant),
        "dormitory" => Some(FacilityKind::Dormitory),
        "office" => Some(FacilityKind::Office),
        _ => None,
    }
}

fn resolve_pick_one(
    operbox: &OperBox,
    pick: &SystemOperatorPickOne,
    used: &HashSet<String>,
) -> Option<ResolvedOperator> {
    for name in &pick.pick_one {
        if used.contains(name) {
            continue;
        }
        let Some(elite) = operbox.elite_of(name) else {
            continue;
        };
        if elite >= pick.elite {
            return Some(ResolvedOperator {
                name: name.clone(),
                elite,
            });
        }
    }
    None
}

fn resolve_slot_operators(
    operbox: &OperBox,
    slot: &SystemSlotDef,
    used: &HashSet<String>,
) -> Option<Vec<ResolvedOperator>> {
    let mut resolved = Vec::with_capacity(slot.operators.len());
    let mut slot_used: HashSet<String> = used.clone();
    for spec in &slot.operators {
        match spec {
            SystemOperatorSpec::Fixed(fixed) => {
                let elite = operbox.elite_of(&fixed.name)?;
                if elite < fixed.elite || slot_used.contains(&fixed.name) {
                    return None;
                }
                if fixed.max_elite.is_some_and(|max| elite > max) {
                    return None;
                }
                slot_used.insert(fixed.name.clone());
                resolved.push(ResolvedOperator {
                    name: fixed.name.clone(),
                    elite,
                });
            }
            SystemOperatorSpec::PickOne(pick) => {
                let op = resolve_pick_one(operbox, pick, &slot_used)?;
                slot_used.insert(op.name.clone());
                resolved.push(op);
            }
        }
    }
    Some(resolved)
}

fn resolve_slot_room<'a>(
    blueprint: &'a BaseBlueprint,
    assignment: &BaseAssignment,
    slot: &SystemSlotDef,
) -> Option<&'a crate::layout::blueprint::RoomBlueprint> {
    if let Some(id) = slot.room_id.as_deref() {
        let room = blueprint.rooms.iter().find(|r| r.id.0 == id)?;
        if !assignment.operators_in(&room.id).is_empty() {
            return None;
        }
        return Some(room);
    }
    let kind = facility_kind(&slot.facility)?;
    blueprint.rooms.iter().find(|r| {
        if r.kind != kind {
            return false;
        }
        if kind == FacilityKind::ControlCenter {
            assignment.operators_in(&r.id).len() < 5
        } else {
            assignment.operators_in(&r.id).is_empty()
        }
    })
}

/// 按 `priority` 贪心选型：返回将认领的 registry 体系（不调 solve）。
pub fn select_registry_systems(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    skip_system_ids: &HashSet<String>,
) -> Vec<RegistrySystemClaim> {
    let Some(cache) = base_systems_cache() else {
        return Vec::new();
    };

    let mut scratch = assignment.clone();
    let mut scratch_used = used.clone();
    let mut claimed_groups: HashSet<String> = HashSet::new();
    let mut selected = Vec::new();

    for system in systems_by_priority(cache) {
        if skip_system_ids.contains(&system.id) {
            continue;
        }
        if !mode_allowed(system, mode) {
            continue;
        }
        if let Some(group) = system.exclusive_group.as_deref() {
            if claimed_groups.contains(group) {
                continue;
            }
        }
        let Some(claim) = plan_registry_system(blueprint, operbox, &scratch, &scratch_used, system)
        else {
            continue;
        };
        apply_registry_claim_to_assignment(&claim, &mut scratch, &mut scratch_used);
        selected.push(claim);
        if let Some(group) = system.exclusive_group.clone() {
            claimed_groups.insert(group);
        }
    }
    selected
}

/// 将 `RegistrySystemClaim` 写入编制。
pub fn apply_registry_system_claim(
    claim: &RegistrySystemClaim,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for slot in &claim.slots {
        let ops: Vec<AssignedOperator> = slot
            .operators
            .iter()
            .map(|op| {
                if !used.insert(op.name.clone()) {
                    return Err(Error::msg(format!(
                        "system {} duplicate {}",
                        claim.system_id, op.name
                    )));
                }
                Ok(AssignedOperator::new(&op.name, op.elite))
            })
            .collect::<Result<Vec<_>>>()?;

        if slot.facility == FacilityKind::ControlCenter {
            let mut existing = assignment.control_operators();
            existing.extend(ops);
            assignment.set_room(RoomId::from("control"), existing);
        } else {
            assignment.set_room(slot.room_id.clone(), ops);
        }
    }
    Ok(())
}

/// 按 `priority` 认领可行成套方案；写入 `assignment` 与 `used`。
/// `skip_system_ids`：已由 `system_integrity` 等路径处理的体系 id（如迷迭香链）。
pub fn claim_base_systems(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    _table: &SkillTable,
    mode: AssignShiftMode,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    skip_system_ids: &HashSet<String>,
) -> Result<()> {
    let selected = select_registry_systems(
        blueprint,
        operbox,
        mode,
        assignment,
        used,
        skip_system_ids,
    );
    for claim in selected {
        apply_registry_system_claim(&claim, assignment, used)?;
    }
    Ok(())
}

fn plan_registry_system(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    system: &BaseSystemDef,
) -> Option<RegistrySystemClaim> {
    if !system_feasible(blueprint, operbox, assignment, used, system) {
        return None;
    }
    let mut scratch = assignment.clone();
    let mut scratch_used = used.clone();
    let mut slots = Vec::new();
    for slot_def in &system.slots {
        if slot_def.optional
            && !slot_resolvable(blueprint, operbox, &scratch, &scratch_used, slot_def)
        {
            continue;
        }
        let room = resolve_slot_room(blueprint, &scratch, slot_def)?;
        let resolved = resolve_slot_operators(operbox, slot_def, &scratch_used)?;
        let facility = facility_kind(&slot_def.facility)?;
        let room_id = if slot_def.facility == "control" {
            RoomId::from("control")
        } else {
            room.id.clone()
        };
        let operators: Vec<AssignedOperator> = resolved
            .iter()
            .map(|op| AssignedOperator::new(&op.name, op.elite))
            .collect();
        slots.push(RegistrySlotClaim {
            room_id: room_id.clone(),
            facility,
            operators: operators.clone(),
        });
        // 同房多 slot（如三间发电站）须顺序占用房间，与 `claim_system` 一致。
        for op in &operators {
            scratch_used.insert(op.name.clone());
        }
        if facility == FacilityKind::ControlCenter {
            let mut existing = scratch.control_operators();
            existing.extend(operators);
            scratch.set_room(RoomId::from("control"), existing);
        } else {
            scratch.set_room(room_id, operators);
        }
    }
    Some(RegistrySystemClaim {
        system_id: system.id.clone(),
        priority: system.priority,
        slots,
    })
}

fn apply_registry_claim_to_assignment(
    claim: &RegistrySystemClaim,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    for slot in &claim.slots {
        for op in &slot.operators {
            used.insert(op.name.clone());
        }
        if slot.facility == FacilityKind::ControlCenter {
            let mut existing = assignment.control_operators();
            existing.extend(slot.operators.clone());
            assignment.set_room(RoomId::from("control"), existing);
        } else {
            assignment.set_room(slot.room_id.clone(), slot.operators.clone());
        }
    }
}

/// slot 能否在当前蓝图 / operbox / used 下落位（房 + 干员均可解）。
fn slot_resolvable(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    slot: &SystemSlotDef,
) -> bool {
    if facility_kind(&slot.facility).is_none() {
        return false;
    }
    if resolve_slot_room(blueprint, assignment, slot).is_none() {
        return false;
    }
    let resolved = match resolve_slot_operators(operbox, slot, used) {
        Some(ops) => ops,
        None => return false,
    };
    if slot.facility == "control" {
        let current = assignment.control_operators().len();
        if current + resolved.len() > 5 {
            return false;
        }
    }
    true
}

/// 系统是否可认领：所有**非可选** slot 都能落位即可（可选 slot 缺失只会被裁剪）。
fn system_feasible(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    system: &BaseSystemDef,
) -> bool {
    system
        .slots
        .iter()
        .filter(|slot| !slot.optional)
        .all(|slot| slot_resolvable(blueprint, operbox, assignment, used, slot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::layout::shift::AssignShiftMode;
    use crate::layout::BaseBlueprint;
    use crate::skill_table::default_skill_table_path;

    fn ideal_e2_operbox() -> OperBox {
        let path = crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap();
        OperBox::load(&path).unwrap()
    }

    fn operbox_without_names(base: &OperBox, exclude: &[&str]) -> OperBox {
        let exclude: HashSet<_> = exclude.iter().copied().collect();
        let entries: Vec<_> = base
            .entries
            .iter()
            .filter(|e| !exclude.contains(e.name.as_str()))
            .cloned()
            .collect();
        OperBox::from_entries(entries)
    }

    fn pinus_claimed(assignment: &BaseAssignment) -> bool {
        assignment
            .control_operators()
            .iter()
            .any(|o| o.name == "焰尾")
            && assignment
                .control_operators()
                .iter()
                .any(|o| o.name == "薇薇安娜")
    }

    #[test]
    fn base_systems_registry_loads_curated_groups() {
        let cache = base_systems_cache().expect("base_systems loaded");
        let ids: HashSet<_> = cache.systems.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains("docus_syracusa"));
        assert!(!ids.contains("rosemary_perception"));
        assert!(ids.contains("witch_long_beta"));
        assert!(ids.contains("pinus_sylvestris"));
        assert!(ids.contains("lungmen_manu_pair"));
        assert!(ids.contains("gongsun_greyy2_power_line"));
    }

    #[test]
    fn claim_docus_syracusa_on_ideal_e2_peak() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let _instances =
            crate::instances::OperatorInstances::load(&default_instances_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        let control: HashSet<_> = assignment
            .control_operators()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert!(control.contains("八幡海铃"));
        assert!(
            control.contains("斩业星熊") && control.contains("诗怀雅"),
            "龙门制造中枢应与叙拉古中枢同室认领: {:?}",
            control
        );

        let docus_room = assignment.rooms.iter().any(|r| {
            r.operators.iter().any(|o| o.name == "但书")
                && r.operators.iter().any(|o| o.name == "伺夜")
                && r.operators.iter().any(|o| o.name == "贝洛内")
        });
        assert!(docus_room, "但书三人组应认领一个贸易站");
    }

    #[test]
    fn exclusive_meta_chain_prefers_docus_over_ling_jie() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        assert!(!used.contains("灵知"));
        let has_docus_trade = assignment.rooms.iter().any(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "但书")
        });
        assert!(has_docus_trade, "但书链应认领某一贸易站");
    }

    #[test]
    fn claim_witch_long_beta_coexists_with_docus_on_dual_trade() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        let trade_posts: Vec<_> = blueprint
            .rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::TradePost)
            .collect();
        assert_eq!(trade_posts.len(), 2, "243 夹具应为双贸");

        let witch_room = assignment.rooms.iter().find(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "巫恋")
                && r.operators.iter().any(|o| o.name == "龙舌兰")
        });
        assert!(witch_room.is_some(), "巫恋+龙舌兰应认领另一贸易站");
        let docus_room = assignment.rooms.iter().find(|r| {
            r.operators.iter().any(|o| o.name == "但书")
        });
        assert!(docus_room.is_some(), "但书链应认领贸易站");
        assert_ne!(
            witch_room.map(|r| &r.room_id),
            docus_room.map(|r| &r.room_id),
            "巫恋组与但书链应分占不同贸站"
        );
    }

    #[test]
    fn claim_pinus_sylvestris_on_ideal_e2_peak() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        let control_ops = assignment.control_operators();
        let control: HashSet<_> = control_ops.iter().map(|o| o.name.as_str()).collect();
        assert!(control.contains("焰尾"), "control: {:?}", control);
        assert!(control.contains("薇薇安娜"), "control: {:?}", control);

        let manu_1 = assignment.operators_in(&RoomId::from("manu_1"));
        assert_eq!(manu_1.len(), 3);
        assert!(manu_1.iter().any(|o| o.name == "灰毫"));
        assert!(manu_1.iter().any(|o| o.name == "远牙"));
        assert!(manu_1.iter().any(|o| o.name == "野鬃"));

        let manu_4 = assignment.operators_in(&RoomId::from("manu_4"));
        assert!(
            manu_4.iter().any(|o| o.name == "砾"),
            "manu_4: {:?}",
            manu_4.iter().map(|o| &o.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn claim_pinus_sylvestris_substitutes_shisibao_when_one_pinus_missing() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_without_names(&ideal_e2_operbox(), &["灰毫"]);
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        assert!(pinus_claimed(&assignment));
        let manu_1 = assignment.operators_in(&RoomId::from("manu_1"));
        assert_eq!(manu_1.len(), 3);
        assert!(manu_1.iter().any(|o| o.name == "远牙"));
        assert!(manu_1.iter().any(|o| o.name == "野鬃"));
        assert!(
            manu_1.iter().any(|o| o.name == "食铁兽"),
            "缺 1 红松应食铁兽替补: {:?}",
            manu_1.iter().map(|o| &o.name).collect::<Vec<_>>()
        );
        assert!(!manu_1.iter().any(|o| o.name == "灰毫"));
    }

    #[test]
    fn claim_pinus_sylvestris_skipped_without_viviana_or_yanwei() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        for exclude in ["薇薇安娜", "焰尾"] {
            let operbox = operbox_without_names(&ideal_e2_operbox(), &[exclude]);
            let mut assignment = BaseAssignment::default();
            let mut used = HashSet::new();
            claim_base_systems(
                &blueprint,
                &operbox,
                &table,
                AssignShiftMode::Peak,
                &mut assignment,
                &mut used,
                &HashSet::new(),
            )
            .unwrap();
            assert!(
                !pinus_claimed(&assignment),
                "缺 {exclude} 时不应认领红松林"
            );
        }
    }

    #[test]
    fn claim_pinus_sylvestris_skipped_with_only_one_pinus_member() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_without_names(&ideal_e2_operbox(), &["远牙", "野鬃"]);
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        assert!(
            !pinus_claimed(&assignment),
            "仅 1 名红松制造核时不应开启"
        );
    }

    #[test]
    fn claim_gongsun_greyy2_power_line_on_ideal_e2_peak() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &HashSet::new(),
        )
        .unwrap();

        let power_ops: Vec<String> = blueprint
            .rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::PowerPlant)
            .flat_map(|r| {
                assignment
                    .operators_in(&r.id)
                    .iter()
                    .map(|o| o.name.clone())
            })
            .collect();
        assert!(
            power_ops.contains(&"承曦格雷伊".to_string()),
            "发电组应认领承曦格雷伊: {:?}",
            power_ops
        );
        assert!(power_ops.contains(&"格雷伊".to_string()), "{power_ops:?}");
        assert!(
            power_ops.iter().any(|n| n == "布丁" || n == "炎熔"),
            "第三发电位: {power_ops:?}"
        );
    }
}
