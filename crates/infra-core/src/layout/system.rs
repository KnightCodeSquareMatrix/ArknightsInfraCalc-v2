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
}

/// `base_systems.json` 中已选体系的完整落位计划。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegistrySystemClaim {
    pub system_id: String,
    pub priority: i32,
    pub tier: OperatorTier,
    pub slots: Vec<RegistrySlotClaim>,
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
    pub shift_modes: Vec<String>,
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

const SYRACUSA_PAIR_SYSTEM: &str = "syracusa_pair";
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
                    return Err(SystemExplainReason::new(
                        "operator_already_used",
                        format!("{} is already assigned", fixed.name),
                    ));
                }
                let Some(progress) = operbox.progress_of(&fixed.name) else {
                    return Err(SystemExplainReason::new(
                        "missing_operator",
                        format!("{} is not owned", fixed.name),
                    ));
                };
                if progress.elite < fixed.elite {
                    return Err(SystemExplainReason::new(
                        "elite_requirement",
                        format!(
                            "{} elite {} < required {}",
                            fixed.name, progress.elite, fixed.elite
                        ),
                    ));
                }
                if fixed.max_elite.is_some_and(|max| progress.elite > max) {
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
                slot_used.insert(fixed.name.clone());
                resolved.push(ResolvedOperator {
                    name: fixed.name.clone(),
                    progress,
                });
            }
            SystemOperatorSpec::PickOne(pick) => {
                let Some(op) = resolve_pick_one(operbox, pick, &slot_used) else {
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
        if !assignment.operators_in(&room.id).is_empty() {
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
        if kind == FacilityKind::ControlCenter {
            if assignment.operators_in(&room.id).len() < 5 {
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
    if slot.trade_role.as_deref() != Some("meta_docus") {
        return;
    }
    rooms.sort_by_key(|room| {
        let is_lv2_gold_trade = room.kind == FacilityKind::TradePost
            && room.level == 2
            && matches!(
                room.product,
                Some(RoomProduct::Trade {
                    order: crate::trade::input::TradeOrderKind::Gold
                })
            );
        if is_lv2_gold_trade {
            0
        } else {
            1
        }
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
    for system in systems_for_tier(cache, tier, operbox, selected) {
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

/// Legacy registry 兼容路径：若直接调用 `claim_base_systems`，且叙拉古同站 meta 已认领、
/// 迷迭香/黑键绑定链可用，则旧 fixed registry 会优先认领可露希尔+黑键+吉星。
///
/// 主路径 `assign_shift` 已跳过这些 role-managed 贸易 registry，由 `trade_segments.roles`
/// 执行但书 -> 可露希尔 -> 巫恋的核心优先策略。
fn docus_closure_long_shift_active(operbox: &OperBox, selected: &[RegistrySystemClaim]) -> bool {
    selected
        .iter()
        .any(|claim| claim.system_id == SYRACUSA_PAIR_SYSTEM)
        && owns_e2(operbox, "但书")
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
    blueprint: &BaseBlueprint,
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
        slot_explains.push(slot.explain);
        slots.push(slot.claim);

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
    Ok(SystemPlan {
        claim: RegistrySystemClaim {
            system_id: system.id.clone(),
            priority: system.priority,
            tier: system.tier.unwrap_or(OperatorTier::Standalone),
            slots,
        },
        slots: slot_explains,
    })
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
    let facility = facility_kind(&slot_def.facility).ok_or_else(|| {
        SystemExplainReason::new(
            "unknown_facility",
            format!("unknown facility {}", slot_def.facility),
        )
    })?;
    if slot_def.facility == "control" {
        let current = assignment.control_operators().len();
        if current + resolved.len() > 5 {
            return Err(SystemExplainReason::new(
                "control_capacity",
                format!(
                    "control capacity exceeded: current {} + slot {} > 5",
                    current,
                    resolved.len()
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
    use crate::layout::{BaseBlueprint, RoomProduct};
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
        assert!(ids.contains("syracusa_pair"));
        assert!(
            !ids.contains("rosemary_perception"),
            "迷迭香感知链走代码化体系层（system_integrity），不进 registry"
        );
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
    fn claim_syracusa_pair_on_ideal_e2_peak() {
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

        let syracusa_room = assignment.rooms.iter().any(|r| {
            !r.operators.iter().any(|o| o.name == "但书")
                && r.operators.iter().any(|o| o.name == "伺夜")
                && r.operators.iter().any(|o| o.name == "贝洛内")
        });
        assert!(syracusa_room, "伺夜+贝洛内应作为叙拉古同站 meta 被认领");
    }

    #[test]
    fn exclusive_meta_chain_prefers_syracusa_over_ling_jie() {
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
        let has_syracusa_trade = assignment.rooms.iter().any(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "伺夜")
                && r.operators.iter().any(|o| o.name == "贝洛内")
        });
        assert!(has_syracusa_trade, "叙拉古同站 meta 应认领某一贸易站");
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
        assert!(
            !manu_1.iter().any(|o| ["灰毫", "远牙", "野鬃"].contains(&o.name.as_str())),
            "红松制造干员不应由 registry 固定同站，交给制造搜索分布: {:?}",
            manu_1.iter().map(|o| &o.name).collect::<Vec<_>>()
        );

        let gold_room_has_gravel = blueprint.rooms.iter().any(|room| {
            matches!(
                room.product,
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold
                })
            ) && assignment
                .operators_in(&room.id)
                .iter()
                .any(|op| op.name == "砾")
        });
        assert!(gold_room_has_gravel, "砾应落在赤金产线");
    }

    #[test]
    fn claim_pinus_sylvestris_when_battle_record_room_is_manu4() {
        let mut blueprint = BaseBlueprint::template_243_use_this().unwrap();
        for room in &mut blueprint.rooms {
            if room.id.0 == "manu_1" {
                room.product = Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold,
                });
            } else if room.id.0 == "manu_4" {
                room.product = Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::BattleRecord,
                });
            }
        }
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

        let system_ids: Vec<_> = select_registry_systems(
            &blueprint,
            &operbox,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .into_iter()
        .map(|claim| claim.system_id)
        .collect();
        assert!(
            system_ids
                .iter()
                .any(|id| id == "pinus_sylvestris_battle_manu4"),
            "manu_4 经验布局应选择红松林变体: {system_ids:?}"
        );

        let manu_4 = assignment.operators_in(&RoomId::from("manu_4"));
        assert!(
            !manu_4.iter().any(|o| ["灰毫", "远牙", "野鬃"].contains(&o.name.as_str())),
            "红松制造干员不应由 registry 固定到 manu_4: {:?}",
            manu_4.iter().map(|o| &o.name).collect::<Vec<_>>()
        );

        let gold_room_has_gravel = blueprint.rooms.iter().any(|room| {
            matches!(
                room.product,
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold
                })
            ) && assignment
                .operators_in(&room.id)
                .iter()
                .any(|op| op.name == "砾")
        });
        assert!(gold_room_has_gravel, "砾应落在赤金产线");
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

        let pinus = report
            .systems
            .iter()
            .find(|entry| entry.system_id == "pinus_sylvestris")
            .expect("pinus_sylvestris should be explained");
        assert_eq!(pinus.status, SystemExplainStatus::Selected);
        assert!(
            pinus
                .slots
                .iter()
                .any(|slot| slot.operators.iter().any(|name| name == "焰尾")),
            "selected pinus slots should include resolved operators: {:?}",
            pinus.slots
        );

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
    fn claim_pinus_sylvestris_does_not_registry_pin_manu_when_one_pinus_missing() {
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
        assert!(
            !manu_1.iter().any(|o| ["远牙", "野鬃", "食铁兽"].contains(&o.name.as_str())),
            "缺 1 红松时 registry 仍不固定制造同站，交给制造搜索分布: {:?}",
            manu_1.iter().map(|o| &o.name).collect::<Vec<_>>()
        );
        assert!(!manu_1.iter().any(|o| o.name == "灰毫"));
    }

    #[test]
    fn claim_pinus_robust_when_manu4_gold_teammates_unavailable() {
        // 回归：manu_4 赤金槽只钉砾（队友由制造贪心补齐），红松制造干员也交回
        // 制造搜索分布。即使赤金队友全缺，红松林仍应认领焰尾/薇薇安娜中枢。
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox =
            operbox_without_names(&ideal_e2_operbox(), &["迷迭香", "阿罗玛", "断罪者", "槐琥"]);
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
            pinus_claimed(&assignment),
            "缺 manu_4 赤金队友时红松林仍应认领（焰尾+薇薇安娜进中枢）: control={:?}",
            assignment
                .control_operators()
                .iter()
                .map(|o| &o.name)
                .collect::<Vec<_>>()
        );
        let manu_1 = assignment.operators_in(&RoomId::from("manu_1"));
        assert!(
            !manu_1.iter().any(|o| ["灰毫", "远牙", "野鬃"].contains(&o.name.as_str())),
            "红松制造干员不应由 registry 固定同站: {:?}",
            manu_1.iter().map(|o| &o.name).collect::<Vec<_>>()
        );
        // manu_4 只钉砾，队友交给制造贪心（不再硬编码）。
        let gold_room_has_gravel = blueprint.rooms.iter().any(|room| {
            matches!(
                room.product,
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold
                })
            ) && assignment
                .operators_in(&room.id)
                .iter()
                .any(|op| op.name == "砾")
        });
        assert!(gold_room_has_gravel, "砾应落在赤金产线");
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
    fn claim_pinus_sylvestris_claims_control_with_only_one_pinus_member() {
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
            pinus_claimed(&assignment),
            "红松不再要求制造同站，仅 1 名红松制造成员时仍可认领中枢"
        );
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
    fn claim_automation_group_uses_gold_factory_when_room_ids_shift() {
        let mut blueprint = BaseBlueprint::template_243_use_this().unwrap();
        for room in &mut blueprint.rooms {
            if room.id.0 == "manu_3" {
                room.product = Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::BattleRecord,
                });
            } else if room.id.0 == "manu_1" {
                room.product = Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold,
                });
            }
        }
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

        let manu_1 = assignment.operators_in(&RoomId::from("manu_1"));
        assert!(
            manu_1.iter().any(|op| op.name == "清流") && manu_1.iter().any(|op| op.name == "温蒂"),
            "自动化制造位应跟随赤金产线，而不是固定 manu_3: {manu_1:?}"
        );
        assert!(
            assignment
                .operators_in(&RoomId::from("manu_3"))
                .iter()
                .all(|op| op.name != "清流" && op.name != "温蒂"),
            "manu_3 已是经验站，不应承接自动化组"
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
