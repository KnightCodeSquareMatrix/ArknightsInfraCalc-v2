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
use crate::layout::tier::OperatorTier;
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
    pub tier: OperatorTier,
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
    /// 跨站 / 同站 / 工具人三层分类。
    #[serde(default)]
    pub tier: Option<OperatorTier>,
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
    pub pick_one: Vec<PickOneCandidate>,
    #[serde(default)]
    pub elite: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PickOneCandidate {
    Name(String),
    Spec(PickOneCandidateSpec),
}

#[derive(Debug, Clone, Deserialize)]
pub struct PickOneCandidateSpec {
    pub name: String,
    #[serde(default)]
    pub elite: Option<u8>,
    #[serde(default)]
    pub max_elite: Option<u8>,
}

impl PickOneCandidate {
    fn name(&self) -> &str {
        match self {
            PickOneCandidate::Name(name) => name,
            PickOneCandidate::Spec(spec) => &spec.name,
        }
    }

    fn elite_requirement(&self, default: u8) -> u8 {
        match self {
            PickOneCandidate::Name(_) => default,
            PickOneCandidate::Spec(spec) => spec.elite.unwrap_or(default),
        }
    }

    fn max_elite(&self) -> Option<u8> {
        match self {
            PickOneCandidate::Name(_) => None,
            PickOneCandidate::Spec(spec) => spec.max_elite,
        }
    }
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

const DOCUS_SYRACUSA_SYSTEM: &str = "docus_syracusa";
const BLACKKEY_CLOSURE_SYSTEM: &str = "blackkey_closure";
const ROSEMARY: &str = "迷迭香";
const BLACKKEY: &str = "黑键";
const CLOSURE: &str = "可露希尔";
const JIXING: &str = "吉星";
const MIN_PERCEPTION_SOURCES: &[&str] = &["絮雨", "八幡海铃", "焰狐龙梓兰"];

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
    for candidate in &pick.pick_one {
        let name = candidate.name();
        if used.contains(name) {
            continue;
        }
        let Some(elite) = operbox.elite_of(name) else {
            continue;
        };
        if elite >= candidate.elite_requirement(pick.elite)
            && !candidate.max_elite().is_some_and(|max| elite > max)
        {
            return Some(ResolvedOperator {
                name: name.to_string(),
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

/// 按 tier → priority 贪心选型：先跨站、后同站（不调 solve）。
/// 两轮共享 `claimed_groups`，跨站认领的 `exclusive_group` 在同站轮可见。
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

    // Phase 1: 跨站体系优先
    select_tier(
        OperatorTier::CrossStation,
        cache,
        blueprint,
        operbox,
        mode,
        &mut scratch,
        &mut scratch_used,
        &mut claimed_groups,
        skip_system_ids,
        &mut selected,
    );

    // Phase 2: 同站组合居次
    select_tier(
        OperatorTier::SameStation,
        cache,
        blueprint,
        operbox,
        mode,
        &mut scratch,
        &mut scratch_used,
        &mut claimed_groups,
        skip_system_ids,
        &mut selected,
    );

    selected
}

/// 在单个 tier 内按 priority 贪心。
fn select_tier(
    tier: OperatorTier,
    cache: &BaseSystemsCache,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    scratch: &mut BaseAssignment,
    scratch_used: &mut HashSet<String>,
    claimed_groups: &mut HashSet<String>,
    skip_system_ids: &HashSet<String>,
    out: &mut Vec<RegistrySystemClaim>,
) {
    for system in systems_for_tier(cache, tier, operbox, out) {
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
        let Some(claim) = plan_registry_system(blueprint, operbox, scratch, scratch_used, system)
        else {
            continue;
        };
        apply_registry_claim_to_assignment(&claim, scratch, scratch_used);
        if let Some(group) = system.exclusive_group.clone() {
            claimed_groups.insert(group);
        }
        out.push(claim);
    }
}

fn systems_by_tier_and_priority(
    cache: &BaseSystemsCache,
    tier: OperatorTier,
) -> Vec<&BaseSystemDef> {
    let mut list: Vec<_> = cache
        .systems
        .iter()
        .filter(|s| s.tier == Some(tier))
        .collect();
    list.sort_by(|a, b| b.priority.cmp(&a.priority));
    list
}

fn systems_for_tier<'a>(
    cache: &'a BaseSystemsCache,
    tier: OperatorTier,
    operbox: &OperBox,
    selected: &[RegistrySystemClaim],
) -> Vec<&'a BaseSystemDef> {
    let mut list = systems_by_tier_and_priority(cache, tier);
    if tier == OperatorTier::SameStation && docus_closure_long_shift_active(operbox, selected) {
        list.sort_by(|a, b| {
            contextual_same_station_priority(b, true)
                .cmp(&contextual_same_station_priority(a, true))
        });
    }
    list
}

fn contextual_same_station_priority(system: &BaseSystemDef, docus_closure_long_shift: bool) -> i32 {
    if docus_closure_long_shift && system.id == BLACKKEY_CLOSURE_SYSTEM {
        return 17;
    }
    system.priority
}

/// Legacy registry 兼容路径：若直接调用 `claim_base_systems`，且但书链长班已认领、
/// 迷迭香/黑键绑定链可用，则旧 fixed registry 会优先认领可露希尔+黑键+吉星。
///
/// 主路径 `assign_shift` 已跳过这些 role-managed 贸易 registry，由 `trade_segments.roles`
/// 执行但书 -> 可露希尔 -> 巫恋的核心优先策略。
fn docus_closure_long_shift_active(operbox: &OperBox, selected: &[RegistrySystemClaim]) -> bool {
    selected
        .iter()
        .any(|claim| claim.system_id == DOCUS_SYRACUSA_SYSTEM)
        && owns_e2(operbox, ROSEMARY)
        && owns_e2(operbox, BLACKKEY)
        && owns_e2(operbox, CLOSURE)
        && owns_e2(operbox, JIXING)
        && MIN_PERCEPTION_SOURCES
            .iter()
            .any(|name| operbox.elite_of(name).is_some_and(|elite| elite >= 2))
}

fn owns_e2(operbox: &OperBox, name: &str) -> bool {
    operbox.elite_of(name).is_some_and(|elite| elite >= 2)
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
    let selected =
        select_registry_systems(blueprint, operbox, mode, assignment, used, skip_system_ids);
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
        tier: system.tier.unwrap_or(OperatorTier::Standalone),
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
        assert!(ids.contains("blackkey_closure"));
        assert!(ids.contains("pinus_sylvestris"));
        assert!(ids.contains("lungmen_manu_pair"));
        assert!(ids.contains("gongsun_greyy2_power_line"));
        assert!(ids.contains("automation_group"), "自动化组应已注册");
        assert!(ids.contains("standardization_mizuki"), "标准化组应已注册");
        assert!(
            !ids.contains("abyssal_hunters"),
            "深海链只在三班轮换 S2 短班入口尝试，不进入普通 base_systems registry"
        );
    }

    #[test]
    fn claim_standardization_mizuki_same_station() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(vec![
            crate::operbox::OperBoxEntry {
                id: "mizuki".into(),
                name: "水月".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
            crate::operbox::OperBoxEntry {
                id: "steward".into(),
                name: "史都华德".into(),
                elite: 1,
                level: 55,
                own: true,
                potential: 1,
                rarity: 3,
            },
            crate::operbox::OperBoxEntry {
                id: "jessica".into(),
                name: "杰西卡".into(),
                elite: 2,
                level: 70,
                own: true,
                potential: 1,
                rarity: 4,
            },
        ]);
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

        let manu_4 = assignment.operators_in(&RoomId::from("manu_4"));
        let names: HashSet<_> = manu_4.iter().map(|op| op.name.as_str()).collect();
        assert_eq!(manu_4.len(), 3, "manu_4: {manu_4:?}");
        assert!(names.contains("水月"), "manu_4: {manu_4:?}");
        assert!(names.contains("史都华德"), "manu_4: {manu_4:?}");
        assert!(names.contains("杰西卡"), "manu_4: {manu_4:?}");
        assert!(used.contains("水月"));
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
    fn legacy_claim_docus_long_shift_prefers_blackkey_closure_over_witch() {
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

        let closure_room = assignment.rooms.iter().find(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == CLOSURE)
                && r.operators.iter().any(|o| o.name == BLACKKEY)
                && r.operators.iter().any(|o| o.name == JIXING)
        });
        assert!(
            closure_room.is_some(),
            "legacy registry 兼容路径应认领可露希尔黑键站"
        );
        assert!(
            !used.contains("巫恋") && !used.contains("龙舌兰"),
            "legacy registry 兼容路径下不应让龙巫挤掉黑键长班"
        );
        let docus_room = assignment
            .rooms
            .iter()
            .find(|r| r.operators.iter().any(|o| o.name == "但书"));
        assert!(docus_room.is_some(), "但书链应认领贸易站");
        assert_ne!(
            closure_room.map(|r| &r.room_id),
            docus_room.map(|r| &r.room_id),
            "可露希尔黑键站与但书链应分占不同贸站"
        );
    }

    #[test]
    fn legacy_claim_witch_long_beta_priority_without_docus() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_without_names(&ideal_e2_operbox(), &["但书"]);
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

        assert!(used.contains("巫恋") && used.contains("龙舌兰"));
        assert!(
            !used.contains(BLACKKEY) && !used.contains(CLOSURE),
            "无但书上下文时龙巫仍应优先于可露希尔站"
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
            assert!(!pinus_claimed(&assignment), "缺 {exclude} 时不应认领红松林");
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

        assert!(!pinus_claimed(&assignment), "仅 1 名红松制造核时不应开启");
    }

    #[test]
    fn claim_gongsun_greyy2_power_line_on_ideal_e2_peak() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        // 自动化组 (cross_station priority 20) 会先抢占承曦格雷伊
        let mut skip = HashSet::new();
        skip.insert("automation_group".to_string());
        claim_base_systems(
            &blueprint,
            &operbox,
            &table,
            AssignShiftMode::Peak,
            &mut assignment,
            &mut used,
            &skip,
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

    // ── automation_group ──

    fn automation_claimed(assignment: &BaseAssignment) -> bool {
        let power_has_chengxi = assignment
            .rooms
            .iter()
            .any(|r| r.operators.iter().any(|o| o.name == "承曦格雷伊"));
        let factory_has_qingliu_wendy = assignment.rooms.iter().any(|r| {
            r.operators.iter().any(|o| o.name == "清流")
                && r.operators.iter().any(|o| o.name == "温蒂")
        });
        power_has_chengxi && factory_has_qingliu_wendy
    }

    #[test]
    fn claim_automation_group_on_ideal_e2_peak() {
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

        assert!(
            automation_claimed(&assignment),
            "自动化组应认领承曦格雷伊(发电) + 清流+温蒂(制造)"
        );
    }

    #[test]
    fn claim_automation_group_skipped_without_weedy_e2() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_without_names(&ideal_e2_operbox(), &["温蒂"]);
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

        assert!(!automation_claimed(&assignment), "缺温蒂时不应认领自动化组");
    }

    #[test]
    fn claim_automation_group_skipped_without_chengxi_greyy() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_without_names(&ideal_e2_operbox(), &["承曦格雷伊"]);
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
            !automation_claimed(&assignment),
            "缺承曦格雷伊时不应认领自动化组"
        );
    }
}
