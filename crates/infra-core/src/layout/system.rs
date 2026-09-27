//! 尚未迁移体系的兼容 registry：小目录 + 贪心认领（`claim_base_systems`）。
//!
//! 数据：`data/base_systems.json`（来源：公孙长乐工具人表等固定组合）。
//! 在高优先声明式 Rule 之后、late competitive Rule 之前认领。

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId, RoomProduct};
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
    #[serde(default, skip_serializing_if = "SlotFillMode::is_default")]
    pub fill: SlotFillMode,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub required_count: u8,
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

/// `base_systems.json` 中已选体系的完整落位计划。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegistrySystemClaim {
    pub system_id: String,
    pub priority: i32,
    pub tier: OperatorTier,
    pub bind_all: bool,
    pub on_shifts: u8,
    pub off_shifts: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_allowed_operators: Vec<String>,
    pub slots: Vec<RegistrySlotClaim>,
}

fn default_on_shifts() -> u8 {
    2
}

fn default_off_shifts() -> u8 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SlotFillMode {
    #[default]
    Fixed,
    Search,
}

impl SlotFillMode {
    fn is_default(value: &Self) -> bool {
        *value == Self::Fixed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemExplainStatus {
    Selected,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SystemExplainReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SystemSlotExplain {
    pub facility: String,
    pub room_id: Option<String>,
    pub optional: bool,
    pub status: SystemExplainStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<SystemExplainReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_room_id: Option<RoomId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SystemExplainEntry {
    pub system_id: String,
    pub label: String,
    pub priority: i32,
    pub tier: OperatorTier,
    pub status: SystemExplainStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<SystemExplainReason>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<SystemSlotExplain>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SystemExplainReport {
    pub mode: AssignShiftMode,
    pub systems: Vec<SystemExplainEntry>,
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
    pub control_allowed_operators: Vec<String>,
    #[serde(default)]
    pub control_exclude_when_inactive: Vec<String>,
    #[serde(default)]
    pub shift_modes: Vec<String>,
    #[serde(default)]
    pub bind_all: bool,
    #[serde(default = "default_on_shifts")]
    pub on_shifts: u8,
    #[serde(default = "default_off_shifts")]
    pub off_shifts: u8,
    pub slots: Vec<SystemSlotDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemSlotDef {
    pub facility: String,
    #[serde(default)]
    pub room_id: Option<String>,
    /// Optional factory recipe constraint. Used by production-line systems whose room ids differ
    /// across frontend-generated layouts.
    #[serde(default)]
    pub recipe: Option<crate::types::RecipeKind>,
    #[serde(default)]
    pub trade_role: Option<String>,
    #[serde(default)]
    pub fill: SlotFillMode,
    /// Search slot 中至少须由实际 solver 选中的候选数；不按列表顺序预选。
    #[serde(default)]
    pub required_count: u8,
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
    /// 最低等级要求；用于 1–2 星按等级（30 级）解锁、精英字段无法表达的技能准入。
    #[serde(default)]
    pub min_level: Option<u32>,
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

impl SystemExplainReason {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedOperator {
    name: String,
    progress: crate::roster::OperatorProgress,
}

struct SlotPlan {
    claim: RegistrySlotClaim,
    explain: SystemSlotExplain,
}

struct SystemPlan {
    claim: RegistrySystemClaim,
    slots: Vec<SystemSlotExplain>,
}

struct SystemPlanError {
    reason: SystemExplainReason,
    slots: Vec<SystemSlotExplain>,
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
        let Some(progress) = operbox.progress_of(name) else {
            continue;
        };
        if progress.elite >= candidate.elite_requirement(pick.elite)
            && !candidate
                .max_elite()
                .is_some_and(|max| progress.elite > max)
        {
            return Some(ResolvedOperator {
                name: name.to_string(),
                progress,
            });
        }
    }
    None
}

fn explain_slot_operators(
    operbox: &OperBox,
    slot: &SystemSlotDef,
    used: &HashSet<String>,
) -> std::result::Result<Vec<ResolvedOperator>, SystemExplainReason> {
    let mut resolved = Vec::with_capacity(slot.operators.len());
    let mut slot_used: HashSet<String> = used.clone();
    for spec in &slot.operators {
        match spec {
            SystemOperatorSpec::Fixed(fixed) => {
                if slot_used.contains(&fixed.name) {
                    if slot.fill == SlotFillMode::Search {
                        continue;
                    }
                    return Err(SystemExplainReason::new(
                        "operator_already_used",
                        format!("{} is already assigned", fixed.name),
                    ));
                }
                let Some(progress) = operbox.progress_of(&fixed.name) else {
                    if slot.fill == SlotFillMode::Search {
                        continue;
                    }
                    return Err(SystemExplainReason::new(
                        "missing_operator",
                        format!("{} is not owned", fixed.name),
                    ));
                };
                if progress.elite < fixed.elite {
                    if slot.fill == SlotFillMode::Search {
                        continue;
                    }
                    return Err(SystemExplainReason::new(
                        "elite_requirement",
                        format!(
                            "{} elite {} < required {}",
                            fixed.name, progress.elite, fixed.elite
                        ),
                    ));
                }
                if fixed.max_elite.is_some_and(|max| progress.elite > max) {
                    if slot.fill == SlotFillMode::Search {
                        continue;
                    }
                    return Err(SystemExplainReason::new(
                        "elite_requirement",
                        format!(
                            "{} elite {} > max {}",
                            fixed.name,
                            progress.elite,
                            fixed.max_elite.unwrap_or_default()
                        ),
                    ));
                }
                if fixed.min_level.is_some_and(|min| progress.level < min) {
                    if slot.fill == SlotFillMode::Search {
                        continue;
                    }
                    return Err(SystemExplainReason::new(
                        "level_requirement",
                        format!(
                            "{} level {} < required {}",
                            fixed.name,
                            progress.level,
                            fixed.min_level.unwrap_or_default()
                        ),
                    ));
                }
                slot_used.insert(fixed.name.clone());
                resolved.push(ResolvedOperator {
                    name: fixed.name.clone(),
                    progress,
                });
            }
            SystemOperatorSpec::PickOne(pick) => {
                let Some(op) = resolve_pick_one(operbox, pick, &slot_used) else {
                    if slot.fill == SlotFillMode::Search {
                        continue;
                    }
                    let names: Vec<_> = pick.pick_one.iter().map(|c| c.name()).collect();
                    return Err(SystemExplainReason::new(
                        "missing_operator",
                        format!("no pick_one candidate is available: {}", names.join(", ")),
                    ));
                };
                slot_used.insert(op.name.clone());
                resolved.push(op);
            }
        }
    }
    Ok(resolved)
}

fn explain_slot_room<'a>(
    blueprint: &'a BaseBlueprint,
    assignment: &BaseAssignment,
    slot: &SystemSlotDef,
) -> std::result::Result<&'a crate::layout::blueprint::RoomBlueprint, SystemExplainReason> {
    let Some(kind) = facility_kind(&slot.facility) else {
        return Err(SystemExplainReason::new(
            "unknown_facility",
            format!("unknown facility {}", slot.facility),
        ));
    };
    if let Some(id) = slot.room_id.as_deref() {
        let Some(room) = blueprint.rooms.iter().find(|r| r.id.0 == id) else {
            return Err(SystemExplainReason::new(
                "room_unavailable",
                format!("room {id} does not exist"),
            ));
        };
        if room.kind != kind {
            return Err(SystemExplainReason::new(
                "room_unavailable",
                format!("room {id} is {:?}, not {:?}", room.kind, kind),
            ));
        }
        if !slot_matches_room_product(slot, room) {
            return Err(SystemExplainReason::new(
                "product_mismatch",
                format!("room {id} does not match requested recipe/product"),
            ));
        }
        if slot.fill == SlotFillMode::Search {
            if assignment.operators_in(&room.id).len() >= room.operator_capacity() {
                return Err(SystemExplainReason::new(
                    "room_occupied",
                    format!("room {id} has no remaining capacity"),
                ));
            }
        } else if !assignment.operators_in(&room.id).is_empty() {
            return Err(SystemExplainReason::new(
                "room_occupied",
                format!("room {id} is already occupied"),
            ));
        }
        return Ok(room);
    }

    let mut has_facility = false;
    let mut has_product = false;
    let mut blocked_by_capacity = false;
    let mut rooms: Vec<_> = blueprint.rooms.iter().collect();
    prioritize_registry_slot_rooms(&mut rooms, slot);
    for room in rooms {
        if room.kind != kind {
            continue;
        }
        has_facility = true;
        if !slot_matches_room_product(slot, room) {
            continue;
        }
        has_product = true;
        if kind == FacilityKind::ControlCenter || slot.fill == SlotFillMode::Search {
            if assignment.operators_in(&room.id).len() < room.operator_capacity() {
                return Ok(room);
            }
        } else if assignment.operators_in(&room.id).is_empty() {
            return Ok(room);
        }
        blocked_by_capacity = true;
    }

    if !has_facility {
        return Err(SystemExplainReason::new(
            "room_unavailable",
            format!("no {:?} room exists", kind),
        ));
    }
    if !has_product {
        return Err(SystemExplainReason::new(
            "product_mismatch",
            format!("no {:?} room matches requested recipe/product", kind),
        ));
    }
    if blocked_by_capacity {
        return Err(SystemExplainReason::new(
            "room_occupied",
            format!("all matching {:?} rooms are occupied or full", kind),
        ));
    }
    Err(SystemExplainReason::new(
        "room_unavailable",
        format!("no available {:?} room", kind),
    ))
}

fn prioritize_registry_slot_rooms<'a>(
    rooms: &mut Vec<&'a crate::layout::blueprint::RoomBlueprint>,
    slot: &SystemSlotDef,
) {
    let Some(role) = slot.trade_role.as_deref() else {
        return;
    };
    rooms.sort_by_key(|room| {
        let is_gold_trade = room.kind == FacilityKind::TradePost
            && matches!(
                room.product,
                Some(RoomProduct::Trade {
                    order: crate::trade::input::TradeOrderKind::Gold
                })
            );
        let is_preferred = match role {
            "meta_docus" => is_gold_trade && room.level == 2,
            "karlan" => is_gold_trade,
            _ => false,
        };
        (!is_preferred, std::cmp::Reverse(room.level))
    });
}

fn slot_matches_room_product(
    slot: &SystemSlotDef,
    room: &crate::layout::blueprint::RoomBlueprint,
) -> bool {
    match slot.recipe {
        Some(recipe) => matches!(
            room.product,
            Some(RoomProduct::Factory {
                recipe: room_recipe
            }) if room_recipe == recipe
        ),
        None => true,
    }
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
    inherited_exclusive_groups: &HashSet<String>,
) -> Vec<RegistrySystemClaim> {
    let Some(cache) = base_systems_cache() else {
        return Vec::new();
    };

    let mut scratch = assignment.clone();
    let mut scratch_used = used.clone();
    let mut claimed_groups = inherited_exclusive_groups.clone();
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

/// 解释每条 registry system 在当前蓝图 / 练度 / 已占用状态下的选型结果。
/// 该函数只做旁路观测，不调用 trade/manu/power solve，也不修改传入编制。
pub fn explain_registry_systems(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    skip_system_ids: &HashSet<String>,
) -> SystemExplainReport {
    let Some(cache) = base_systems_cache() else {
        return SystemExplainReport {
            mode,
            systems: Vec::new(),
        };
    };

    let mut scratch = assignment.clone();
    let mut scratch_used = used.clone();
    let mut claimed_groups: HashSet<String> = HashSet::new();
    let mut selected = Vec::new();
    let mut systems = Vec::new();

    explain_tier(
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
        &mut systems,
    );
    explain_tier(
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
        &mut systems,
    );

    SystemExplainReport { mode, systems }
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
    for system in systems_by_tier_and_priority(cache, tier) {
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
        let Ok(plan) =
            plan_registry_system_detailed(blueprint, operbox, scratch, scratch_used, system)
        else {
            continue;
        };
        apply_registry_claim_to_assignment(&plan.claim, scratch, scratch_used);
        if let Some(group) = system.exclusive_group.clone() {
            claimed_groups.insert(group);
        }
        out.push(plan.claim);
    }
}

fn explain_tier(
    tier: OperatorTier,
    cache: &BaseSystemsCache,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    scratch: &mut BaseAssignment,
    scratch_used: &mut HashSet<String>,
    claimed_groups: &mut HashSet<String>,
    skip_system_ids: &HashSet<String>,
    selected: &mut Vec<RegistrySystemClaim>,
    out: &mut Vec<SystemExplainEntry>,
) {
    for system in systems_by_tier_and_priority(cache, tier) {
        if skip_system_ids.contains(&system.id) {
            out.push(system_explain_entry(
                system,
                SystemExplainStatus::Skipped,
                Some(SystemExplainReason::new(
                    "skipped_by_policy",
                    "system is skipped by assignment policy",
                )),
                Vec::new(),
            ));
            continue;
        }
        if !mode_allowed(system, mode) {
            out.push(system_explain_entry(
                system,
                SystemExplainStatus::Skipped,
                Some(SystemExplainReason::new(
                    "shift_mode_not_allowed",
                    format!("{mode:?} shift is not allowed for this system"),
                )),
                Vec::new(),
            ));
            continue;
        }
        if let Some(group) = system.exclusive_group.as_deref() {
            if claimed_groups.contains(group) {
                out.push(system_explain_entry(
                    system,
                    SystemExplainStatus::Skipped,
                    Some(SystemExplainReason::new(
                        "exclusive_group_claimed",
                        format!("exclusive group {group} is already claimed"),
                    )),
                    Vec::new(),
                ));
                continue;
            }
        }

        match plan_registry_system_detailed(blueprint, operbox, scratch, scratch_used, system) {
            Ok(plan) => {
                apply_registry_claim_to_assignment(&plan.claim, scratch, scratch_used);
                if let Some(group) = system.exclusive_group.clone() {
                    claimed_groups.insert(group);
                }
                selected.push(plan.claim);
                out.push(system_explain_entry(
                    system,
                    SystemExplainStatus::Selected,
                    None,
                    plan.slots,
                ));
            }
            Err(err) => {
                out.push(system_explain_entry(
                    system,
                    SystemExplainStatus::Skipped,
                    Some(err.reason),
                    err.slots,
                ));
            }
        }
    }
}

fn system_explain_entry(
    system: &BaseSystemDef,
    status: SystemExplainStatus,
    reason: Option<SystemExplainReason>,
    slots: Vec<SystemSlotExplain>,
) -> SystemExplainEntry {
    SystemExplainEntry {
        system_id: system.id.clone(),
        label: system.label.clone(),
        priority: system.priority,
        tier: system.tier.unwrap_or(OperatorTier::Standalone),
        status,
        exclusive_group: system.exclusive_group.clone(),
        reason,
        slots,
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

/// 将 `RegistrySystemClaim` 写入编制。
pub fn apply_registry_system_claim(
    blueprint: &BaseBlueprint,
    claim: &RegistrySystemClaim,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for slot in &claim.slots {
        if slot.fill == SlotFillMode::Search {
            continue;
        }
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
                Ok(op.clone())
            })
            .collect::<Result<Vec<_>>>()?;

        if slot.facility == FacilityKind::ControlCenter {
            let mut existing = assignment.control_operators();
            if existing.len() + ops.len() > 5 {
                return Err(Error::msg(format!(
                    "system {} control capacity exceeded",
                    claim.system_id
                )));
            }
            existing.extend(ops);
            assignment.set_room(RoomId::from("control"), existing);
        } else {
            let room = blueprint.room(&slot.room_id).ok_or_else(|| {
                Error::msg(format!(
                    "system {} room {} not in blueprint",
                    claim.system_id, slot.room_id.0
                ))
            })?;
            let mut existing = assignment.operators_in(&slot.room_id).to_vec();
            if existing.len() + ops.len() > room.operator_capacity() {
                return Err(Error::msg(format!(
                    "system {} room {} capacity exceeded",
                    claim.system_id, slot.room_id.0
                )));
            }
            existing.extend(ops);
            assignment.set_room(slot.room_id.clone(), existing);
        }
    }
    Ok(())
}

/// 按 `priority` 认领可行成套方案；写入 `assignment` 与 `used`。
/// `skip_system_ids`：已由声明式 Rule 处理或按规则互斥关闭的兼容体系 id。
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
        &HashSet::new(),
    );
    for claim in selected {
        apply_registry_system_claim(blueprint, &claim, assignment, used)?;
    }
    Ok(())
}

fn plan_registry_system_detailed(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    system: &BaseSystemDef,
) -> std::result::Result<SystemPlan, SystemPlanError> {
    let mut scratch = assignment.clone();
    let mut scratch_used = used.clone();
    let mut slots = Vec::new();
    let mut slot_explains = Vec::new();
    for slot_def in &system.slots {
        let slot = match plan_registry_slot(blueprint, operbox, &scratch, &scratch_used, slot_def) {
            Ok(slot) => slot,
            Err(reason) if slot_def.optional => {
                slot_explains.push(SystemSlotExplain {
                    facility: slot_def.facility.clone(),
                    room_id: slot_def.room_id.clone(),
                    optional: true,
                    status: SystemExplainStatus::Skipped,
                    reason: Some(reason),
                    resolved_room_id: None,
                    operators: Vec::new(),
                });
                continue;
            }
            Err(reason) => {
                slot_explains.push(SystemSlotExplain {
                    facility: slot_def.facility.clone(),
                    room_id: slot_def.room_id.clone(),
                    optional: false,
                    status: SystemExplainStatus::Skipped,
                    reason: Some(reason.clone()),
                    resolved_room_id: None,
                    operators: Vec::new(),
                });
                return Err(SystemPlanError {
                    reason,
                    slots: slot_explains,
                });
            }
        };
        let facility = slot.claim.facility;
        let room_id = slot.claim.room_id.clone();
        let operators = slot.claim.operators.clone();
        let fill = slot.claim.fill;
        let required_count = slot.claim.required_count;
        slot_explains.push(slot.explain);
        slots.push(slot.claim);

        // 同房多 slot（如三间发电站）须顺序占用房间，与 `claim_system` 一致。
        for op in &operators {
            scratch_used.insert(op.name.clone());
        }
        if facility == FacilityKind::ControlCenter {
            let mut existing = scratch.control_operators();
            if fill == SlotFillMode::Search {
                existing.extend((0..required_count).map(|index| {
                    AssignedOperator::new(format!("__control_reservation_{index}"), 0)
                }));
            } else {
                existing.extend(operators);
            }
            scratch.set_room(RoomId::from("control"), existing);
        } else {
            scratch.set_room(room_id, operators);
        }
    }
    Ok(SystemPlan {
        claim: RegistrySystemClaim {
            system_id: system.id.clone(),
            priority: system.priority,
            tier: system.tier.unwrap_or(OperatorTier::Standalone),
            bind_all: system.bind_all,
            on_shifts: system.on_shifts,
            off_shifts: system.off_shifts,
            exclusive_group: system.exclusive_group.clone(),
            control_allowed_operators: system.control_allowed_operators.clone(),
            slots,
        },
        slots: slot_explains,
    })
}

/// Path-scoped control admission: an inactive registry path cannot leak its
/// restricted candidates into generic control fill.
pub fn control_exclusions_for_unclaimed_systems(claims: &[RegistrySystemClaim]) -> HashSet<String> {
    let claimed: HashSet<_> = claims
        .iter()
        .map(|claim| claim.system_id.as_str())
        .collect();
    base_systems_cache()
        .into_iter()
        .flat_map(|cache| cache.systems.iter())
        .filter(|system| !claimed.contains(system.id.as_str()))
        .flat_map(|system| system.control_exclude_when_inactive.iter().cloned())
        .collect()
}

fn plan_registry_slot(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    slot_def: &SystemSlotDef,
) -> std::result::Result<SlotPlan, SystemExplainReason> {
    let room = explain_slot_room(blueprint, assignment, slot_def)?;
    let resolved = explain_slot_operators(operbox, slot_def, used)?;
    if slot_def.required_count > 0 && resolved.len() < usize::from(slot_def.required_count) {
        return Err(SystemExplainReason::new(
            "missing_required_candidates",
            format!(
                "search slot has {} available candidates, requires {}",
                resolved.len(),
                slot_def.required_count
            ),
        ));
    }
    let facility = facility_kind(&slot_def.facility).ok_or_else(|| {
        SystemExplainReason::new(
            "unknown_facility",
            format!("unknown facility {}", slot_def.facility),
        )
    })?;
    if slot_def.facility == "control" {
        let current = assignment.control_operators().len();
        let required_capacity = if slot_def.fill == SlotFillMode::Search {
            usize::from(slot_def.required_count)
        } else {
            resolved.len()
        };
        if current + required_capacity > 5 {
            return Err(SystemExplainReason::new(
                "control_capacity",
                format!(
                    "control capacity exceeded: current {} + slot {} > 5",
                    current, required_capacity
                ),
            ));
        }
    } else {
        let current = assignment.operators_in(&room.id).len();
        let capacity = room.operator_capacity();
        if current + resolved.len() > capacity {
            return Err(SystemExplainReason::new(
                "room_capacity",
                format!(
                    "room {} capacity exceeded: current {} + slot {} > {}",
                    room.id.0,
                    current,
                    resolved.len(),
                    capacity
                ),
            ));
        }
    }
    let room_id = if slot_def.facility == "control" {
        RoomId::from("control")
    } else {
        room.id.clone()
    };
    let operators: Vec<AssignedOperator> = resolved
        .iter()
        .map(|op| AssignedOperator::from_progress(&op.name, op.progress))
        .collect();
    let operator_names = operators.iter().map(|op| op.name.clone()).collect();
    Ok(SlotPlan {
        claim: RegistrySlotClaim {
            room_id: room_id.clone(),
            facility,
            operators,
            fill: slot_def.fill,
            required_count: slot_def.required_count,
        },
        explain: SystemSlotExplain {
            facility: slot_def.facility.clone(),
            room_id: slot_def.room_id.clone(),
            optional: slot_def.optional,
            status: SystemExplainStatus::Selected,
            reason: None,
            resolved_room_id: Some(room_id),
            operators: operator_names,
        },
    })
}

fn apply_registry_claim_to_assignment(
    claim: &RegistrySystemClaim,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    for slot in &claim.slots {
        if slot.fill == SlotFillMode::Search {
            if slot.facility == FacilityKind::ControlCenter && slot.required_count > 0 {
                let mut existing = assignment.control_operators();
                existing.extend((0..slot.required_count).map(|index| {
                    AssignedOperator::new(
                        format!("__control_reservation_{}_{}", claim.system_id, index),
                        0,
                    )
                }));
                assignment.set_room(RoomId::from("control"), existing);
            }
            continue;
        }
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
        for forbidden in ["docus_syracusa", "syracusa_pair", "syracusa_cross_station"] {
            assert!(
                !ids.contains(forbidden),
                "叙拉古是自然中枢-贸易效率关系，不应注册历史固定体系 {forbidden}"
            );
        }
        assert!(
            !ids.contains("rosemary_perception"),
            "迷迭香感知链走 orchestration_rules，不进 registry"
        );
        assert!(ids.contains("witch_long_beta"));
        assert!(ids.contains("blackkey_closure"));
        assert!(
            !ids.contains("pinus_sylvestris"),
            "红松林走 orchestration_rules，不进 registry"
        );
        assert!(ids.contains("lungmen_manu_pair"));
        assert!(ids.contains("ling_jie_karlan"));
        assert!(
            !ids.contains("gongsun_greyy2_power_line") && !ids.contains("automation_group"),
            "自动化组及发电线已迁入 orchestration_rules，不得保留 legacy registry"
        );
        assert!(
            !ids.contains("standardization_mizuki"),
            "标准化组合由制造候选池和效率搜索自然产生，不进入 registry"
        );
        assert!(
            !ids.contains("abyssal_hunters"),
            "深海链只在三班轮换 S2 短班入口尝试，不进入普通 base_systems registry"
        );
    }

    #[test]
    fn registry_does_not_claim_docus_trade_teammates() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let _instances =
            crate::instances::OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        for name in ["八幡海铃", "伺夜", "贝洛内"] {
            assert!(operbox.owns(name), "ideal E2 fixture must own {name}");
        }

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

        for name in ["八幡海铃", "伺夜", "贝洛内"] {
            assert!(
                !used.contains(name),
                "叙拉古成员不得由 registry 强制认领: {name}"
            );
        }
    }

    #[test]
    fn registry_claims_ling_jie_karlan_trade_pair() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(
            ideal_e2_operbox()
                .entries
                .into_iter()
                .filter(|entry| ["灵知", "孑", "银灰"].contains(&entry.name.as_str()))
                .collect(),
        )
        .unwrap();
        let claims = select_registry_systems(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        let ling = claims
            .iter()
            .find(|claim| claim.system_id == "ling_jie_karlan")
            .expect("灵知 E2 应由 registry 认领中枢位");
        assert_eq!(ling.slots.len(), 2);
        assert_eq!(ling.slots[0].facility, FacilityKind::ControlCenter);
        assert_eq!(
            ling.slots[0]
                .operators
                .iter()
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>(),
            vec!["灵知"]
        );
        assert_eq!(ling.slots[1].facility, FacilityKind::TradePost);
        assert_eq!(
            ling.slots[1]
                .operators
                .iter()
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>(),
            vec!["孑", "银灰"]
        );
    }

    #[test]
    fn registry_does_not_claim_ling_without_karlan_trade_pair() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::from_entries(
            ideal_e2_operbox()
                .entries
                .into_iter()
                .filter(|entry| entry.name == "灵知")
                .collect(),
        )
        .unwrap();
        let claims = select_registry_systems(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(!claims
            .iter()
            .any(|claim| claim.system_id == "ling_jie_karlan"));
    }

    #[test]
    fn registry_does_not_claim_ling_with_unpromoted_jaye() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut entries: Vec<_> = ideal_e2_operbox()
            .entries
            .into_iter()
            .filter(|entry| ["灵知", "孑", "银灰"].contains(&entry.name.as_str()))
            .collect();
        let jaye = entries.iter_mut().find(|entry| entry.name == "孑").unwrap();
        jaye.elite = 0;
        jaye.level = 45;
        let operbox = OperBox::from_entries(entries).unwrap();
        let claims = select_registry_systems(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(!claims
            .iter()
            .any(|claim| claim.system_id == "ling_jie_karlan"));
    }

    #[test]
    fn pinus_is_not_claimed_by_legacy_registry() {
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

        assert!(!pinus_claimed(&assignment));
    }

    #[test]
    fn explain_registry_systems_reports_selected_and_skipped_reasons() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = ideal_e2_operbox();
        let mut skip = HashSet::new();
        skip.insert("blackkey_closure".to_string());

        let report = explain_registry_systems(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
            &skip,
        );

        assert!(report
            .systems
            .iter()
            .all(|entry| entry.system_id != "pinus_sylvestris"));

        let blackkey = report
            .systems
            .iter()
            .find(|entry| entry.system_id == "blackkey_closure")
            .expect("blackkey_closure should be explained");
        assert_eq!(blackkey.status, SystemExplainStatus::Skipped);
        assert_eq!(
            blackkey.reason.as_ref().map(|r| r.code.as_str()),
            Some("skipped_by_policy")
        );
    }

    #[test]
    fn legacy_automation_systems_are_absent_from_registry() {
        let cache = base_systems_cache().expect("base_systems loaded");
        let ids: HashSet<_> = cache
            .systems
            .iter()
            .map(|system| system.id.as_str())
            .collect();
        assert!(!ids.contains("automation_group"));
        assert!(!ids.contains("gongsun_greyy2_power_line"));
    }
}
