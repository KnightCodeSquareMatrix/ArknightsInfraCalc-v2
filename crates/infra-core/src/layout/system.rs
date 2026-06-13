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
        let elite = operbox.elite_of(name)?;
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
    for spec in &slot.operators {
        match spec {
            SystemOperatorSpec::Fixed(fixed) => {
                let elite = operbox.elite_of(&fixed.name)?;
                if elite < fixed.elite || used.contains(&fixed.name) {
                    return None;
                }
                resolved.push(ResolvedOperator {
                    name: fixed.name.clone(),
                    elite,
                });
            }
            SystemOperatorSpec::PickOne(pick) => {
                resolved.push(resolve_pick_one(operbox, pick, used)?);
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

/// 按 `priority` 认领可行成套方案；写入 `assignment` 与 `used`。
pub fn claim_base_systems(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    _table: &SkillTable,
    mode: AssignShiftMode,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let Some(cache) = base_systems_cache() else {
        return Ok(());
    };

    let mut claimed_groups: HashSet<String> = HashSet::new();

    for system in systems_by_priority(cache) {
        if !mode_allowed(system, mode) {
            continue;
        }
        if let Some(group) = system.exclusive_group.as_deref() {
            if claimed_groups.contains(group) {
                continue;
            }
        }
        if !system_feasible(blueprint, operbox, assignment, used, system) {
            continue;
        }
        claim_system(blueprint, operbox, assignment, used, system)?;
        if let Some(group) = system.exclusive_group.clone() {
            claimed_groups.insert(group);
        }
    }
    Ok(())
}

fn system_feasible(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    system: &BaseSystemDef,
) -> bool {
    for slot in &system.slots {
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
    }
    true
}

fn claim_system(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    system: &BaseSystemDef,
) -> Result<()> {
    for slot in &system.slots {
        let room = resolve_slot_room(blueprint, assignment, slot)
            .ok_or_else(|| Error::msg(format!("system {} slot room vanished", system.id)))?;
        let resolved = resolve_slot_operators(operbox, slot, used)
            .ok_or_else(|| Error::msg(format!("system {} slot operators vanished", system.id)))?;
        let ops: Vec<AssignedOperator> = resolved
            .iter()
            .map(|op| {
                if !used.insert(op.name.clone()) {
                    return Err(Error::msg(format!(
                        "system {} duplicate {}",
                        system.id, op.name
                    )));
                }
                Ok(AssignedOperator::new(&op.name, op.elite))
            })
            .collect::<Result<Vec<_>>>()?;

        if slot.facility == "control" {
            let mut existing: Vec<AssignedOperator> = assignment.control_operators();
            existing.extend(ops);
            assignment.set_room(RoomId::from("control"), existing);
        } else {
            assignment.set_room(room.id.clone(), ops);
        }
    }
    Ok(())
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

    #[test]
    fn base_systems_registry_loads_curated_groups() {
        let cache = base_systems_cache().expect("base_systems loaded");
        let ids: HashSet<_> = cache.systems.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains("docus_syracusa"));
        assert!(ids.contains("rosemary_perception"));
        assert!(ids.contains("witch_long_beta"));
        assert!(ids.contains("lungmen_manu_pair"));
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
        )
        .unwrap();

        let control: HashSet<_> = assignment
            .control_operators()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert!(control.contains("八幡海铃"));
        assert!(control.contains("夕"));
        assert!(
            control.contains("斩业星熊") && control.contains("诗怀雅"),
            "龙门制造中枢应与叙拉古中枢同室认领: {:?}",
            control
        );

        let trade_1: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_1"))
            .iter()
            .map(|o| o.name.clone())
            .collect();
        assert!(trade_1.contains("黑键"));
        assert!(trade_1.contains("吉星"));
        assert!(trade_1.contains("可露希尔"));

        let trade_2: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_2"))
            .iter()
            .map(|o| o.name.clone())
            .collect();
        assert!(trade_2.contains("但书"));
        assert!(trade_2.contains("伺夜"));
        assert!(trade_2.contains("贝洛内"));

        let manu_4: HashSet<_> = assignment
            .operators_in(&RoomId::from("manu_4"))
            .iter()
            .map(|o| o.name.clone())
            .collect();
        assert!(manu_4.contains("迷迭香"));
        assert!(manu_4.contains("阿罗玛"));
        assert!(manu_4.contains("砾"));
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
        )
        .unwrap();

        assert!(!used.contains("灵知"));
        let trade_2: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_2"))
            .iter()
            .map(|o| o.name.clone())
            .collect();
        assert!(trade_2.contains("但书"));
    }
}
