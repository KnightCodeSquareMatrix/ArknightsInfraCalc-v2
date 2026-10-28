use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::{
    assignment_operator_names, AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind,
    RoomProduct,
};
use crate::operbox::OperBox;
use crate::schedule::{
    fiammetta_return_priority, DroneAction, TeamLabel, TeamRotationReport, TimedRotationProfile,
    FIAMMETTA_RETURN_PRIORITY,
};
use crate::trade::input::TradeOrderKind;
use crate::types::RecipeKind;

#[derive(Debug, Clone)]
pub struct MaaExportOptions {
    pub title: String,
    pub description: Option<String>,
    /// 作者信息，写入导出 JSON 的 `author` 字段。
    pub author: Option<String>,
    /// 排班方案 ID（一图流格式），写入 `id` 字段。
    pub id: Option<u64>,
    /// 基建布局类型（如 243），写入 `buildingType` 字段。
    pub building_type: Option<u32>,
    /// 换班次数说明（如 `"3班"`），写入 `planTimes` 字段。
    pub plan_times: Option<String>,
    /// 菲亚梅塔换班优先级清单（按优先级从高到低排列的干员名列表）。
    ///
    /// 每个 plan 生成时，从当班 assignment 中找清单里第一个在岗干员作为换班目标；
    /// 找到则 `enable: true`，找不到或清单为空则 `enable: false`。
    pub fiammetta_priority: Vec<String>,
}

impl MaaExportOptions {
    pub fn for_blueprint(blueprint: &BaseBlueprint) -> Self {
        let template = blueprint
            .template
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("custom");
        Self {
            title: format!("{template} 基建排班"),
            description: Some("由 ArknightsInfraCalc 生成；可导入 MAA 自定义基建换班。".into()),
            author: None,
            id: None,
            building_type: None,
            plan_times: None,
            fiammetta_priority: Vec::new(),
        }
    }

    /// 启用公孙长乐确认的菲亚梅塔常规目标顺序。
    pub fn enable_gongsun_fiammetta_priority(&mut self) {
        self.fiammetta_priority = FIAMMETTA_RETURN_PRIORITY
            .into_iter()
            .map(str::to_owned)
            .collect();
    }

    pub fn enable_fiammetta_priority_for_blueprint(&mut self, blueprint: &BaseBlueprint) {
        self.fiammetta_priority = fiammetta_return_priority(blueprint)
            .into_iter()
            .map(str::to_owned)
            .collect();
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MaaSchedule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(rename = "buildingType", skip_serializing_if = "Option::is_none")]
    pub building_type: Option<u32>,
    #[serde(rename = "planTimes", skip_serializing_if = "Option::is_none")]
    pub plan_times: Option<String>,
    pub plans: Vec<MaaPlan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaaPlan {
    pub name: String,
    pub description: String,
    #[serde(rename = "Fiammetta")]
    pub fiammetta: MaaFiammetta,
    pub drones: MaaDrones,
    pub rooms: MaaRooms,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaaFiammetta {
    pub enable: bool,
    pub target: String,
    pub order: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaaDrones {
    pub enable: bool,
    pub room: &'static str,
    pub index: u8,
    pub order: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MaaRooms {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trading: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manufacture: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub power: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dormitory: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meeting: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hire: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processing: Vec<MaaRoomSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recycle: Vec<MaaRecycleSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaaRoomSlot {
    pub skip: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<&'static str>,
    pub operators: Vec<String>,
    pub sort: bool,
    pub autofill: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaaRecycleSlot {
    pub operators: Vec<String>,
}

struct PlanInput<'a> {
    assignment: &'a BaseAssignment,
    name: String,
    description: String,
    dorm_autofill: bool,
    fiammetta_target: Option<&'a str>,
    fiammetta_priority: &'a [String],
    drone: Option<&'a crate::schedule::DroneAction>,
}

impl MaaSchedule {
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| Error::msg(format!("maa schedule serialize: {e}")))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    Error::msg(format!("maa schedule mkdir {}: {e}", parent.display()))
                })?;
            }
        }
        std::fs::write(path, json)
            .map_err(|e| Error::msg(format!("maa schedule write {}: {e}", path.display())))
    }
}

pub fn build_from_team_rotation(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
    opts: &MaaExportOptions,
) -> Result<MaaSchedule> {
    let plans = report
        .shifts
        .iter()
        .map(|shift| {
            let active: Vec<&str> = shift
                .active_teams
                .iter()
                .map(|team| team_label_zh(report.profile, *team))
                .collect();
            let fiammetta_action_note = shift
                .fiammetta
                .as_ref()
                .filter(|_| {
                    report.profile == crate::schedule::TimedRotationProfile::MainBackup24_24
                })
                .map(|action| format!("；本班菲亚贸易站目标：{}", action.target))
                .unwrap_or_default();
            PlanInput {
                assignment: &shift.assignment,
                name: format!(
                    "Shift {} · {:.0}h · {}",
                    shift.index + 1,
                    shift.duration_hours,
                    active.join("+")
                ),
                description: format!(
                    "本班 {:.0} 小时；下次在 {:.0} 小时后执行下一计划；休息 {} 队{}{}",
                    shift.duration_hours,
                    shift.duration_hours,
                    team_label_zh(report.profile, shift.resting_team),
                    if report.profile.maa_uses_rest_team_dorm_autofill() {
                        "；宿舍已设为自动填充，请换班后手动把休息队伍倒入宿舍"
                    } else {
                        ""
                    },
                    fiammetta_action_note
                ),
                // 二班没有逐班的宿舍恢复推演：休息队伍的住宿由 MAA 自动填充，
                // 其他 profile 只在床位不足 warning 时才请求填充。
                dorm_autofill: report.profile.maa_uses_rest_team_dorm_autofill()
                    || report.warnings.iter().any(|warning| {
                        warning.starts_with(&format!("shift {} 宿舍普通休息候选", shift.index + 1))
                    }),
                fiammetta_target: shift
                    .fiammetta
                    .as_ref()
                    .map(|action| action.target.as_str()),
                // ABC 主路径只执行 schedule 已确认的回岗动作；没有动作就保持关闭。
                fiammetta_priority: &[],
                drone: shift.drone.as_ref(),
            }
        })
        .map(|input| build_plan(blueprint, &input))
        .collect();
    Ok(wrap_schedule(
        opts,
        plans,
        Some(report.profile.plan_times()),
    ))
}

fn wrap_schedule(
    opts: &MaaExportOptions,
    plans: Vec<MaaPlan>,
    default_plan_times: Option<&str>,
) -> MaaSchedule {
    MaaSchedule {
        author: opts.author.clone(),
        title: Some(opts.title.clone()),
        description: opts.description.clone(),
        id: opts.id,
        building_type: opts.building_type,
        plan_times: opts
            .plan_times
            .clone()
            .or_else(|| default_plan_times.map(str::to_owned)),
        plans,
    }
}

fn build_plan(blueprint: &BaseBlueprint, input: &PlanInput) -> MaaPlan {
    let mut rooms = build_rooms(blueprint, input.assignment);
    if input.dorm_autofill {
        for slot in &mut rooms.dormitory {
            slot.skip = false;
            slot.autofill = true;
        }
    }
    MaaPlan {
        name: input.name.clone(),
        description: input.description.clone(),
        fiammetta: input
            .fiammetta_target
            .map(resolve_scheduled_fiammetta)
            .unwrap_or_else(|| resolve_fiammetta(input.fiammetta_priority, input.assignment)),
        drones: resolve_drones(blueprint, input.drone),
        rooms,
    }
}

/// 把 schedule 层已决策的无人机动作映射为 MAA 下标。
///
/// schedule 只在班次有合法赤金制造房且非菲亚-only 时给出 `DroneAction`；
/// 这里只做 room_id → MAA `index` 的转换，不再自行选择目标。
fn resolve_drones(blueprint: &BaseBlueprint, drone: Option<&DroneAction>) -> MaaDrones {
    let Some(drone) = drone else {
        return MaaDrones {
            enable: false,
            room: "manufacture",
            index: 1,
            order: "pre",
        };
    };
    let index = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Factory)
        .position(|room| room.id == drone.room_id)
        .map(|i| (i + 1) as u8)
        .unwrap_or(1);
    MaaDrones {
        enable: true,
        room: "manufacture",
        index,
        order: drone.order,
    }
}

fn resolve_scheduled_fiammetta(target: &str) -> MaaFiammetta {
    MaaFiammetta {
        enable: true,
        target: target.to_string(),
        order: "pre",
    }
}

fn build_rooms(blueprint: &BaseBlueprint, assignment: &BaseAssignment) -> MaaRooms {
    MaaRooms {
        trading: blueprint
            .rooms_of(FacilityKind::TradePost)
            .into_iter()
            .map(|room| {
                let product = room.product.as_ref().and_then(|p| match p {
                    RoomProduct::Trade { order } => Some(maa_trade_product(*order)),
                    _ => None,
                });
                production_slot(assignment, &room.id.0, product)
            })
            .collect(),
        manufacture: blueprint
            .rooms_of(FacilityKind::Factory)
            .into_iter()
            .map(|room| {
                let product = room.product.as_ref().and_then(|p| match p {
                    RoomProduct::Factory { recipe } => Some(maa_factory_product(*recipe)),
                    _ => None,
                });
                production_slot(assignment, &room.id.0, product)
            })
            .collect(),
        power: blueprint
            .rooms_of(FacilityKind::PowerPlant)
            .into_iter()
            .map(|room| production_slot(assignment, &room.id.0, None))
            .collect(),
        dormitory: build_dorm_slots(blueprint, assignment),
        control: if blueprint.count_facility(FacilityKind::ControlCenter) > 0 {
            vec![shared_slot(assignment, "control")]
        } else {
            Vec::new()
        },
        meeting: blueprint
            .rooms_of(FacilityKind::MeetingRoom)
            .into_iter()
            .map(|room| shared_slot(assignment, &room.id.0))
            .collect(),
        hire: blueprint
            .rooms_of(FacilityKind::Office)
            .into_iter()
            .map(|room| shared_slot(assignment, &room.id.0))
            .collect(),
        processing: blueprint
            .rooms_of(FacilityKind::Workshop)
            .into_iter()
            .enumerate()
            .map(|(index, room)| {
                let _ = index;
                shared_slot(assignment, &room.id.0)
            })
            .collect(),
        recycle: blueprint
            .rooms_of(FacilityKind::Recycle)
            .into_iter()
            .map(|room| MaaRecycleSlot {
                operators: assignment
                    .operators_in(&room.id)
                    .iter()
                    .map(|operator| operator.name.clone())
                    .collect(),
            })
            .collect(),
    }
}

fn production_slot(
    assignment: &BaseAssignment,
    room_id: &str,
    product: Option<&'static str>,
) -> MaaRoomSlot {
    let operators = operator_names(assignment, room_id);
    MaaRoomSlot {
        skip: false,
        product,
        operators,
        sort: false,
        autofill: false,
    }
}

fn shared_slot(assignment: &BaseAssignment, room_id: &str) -> MaaRoomSlot {
    let operators = operator_names(assignment, room_id);
    MaaRoomSlot {
        skip: false,
        product: None,
        operators,
        sort: false,
        autofill: false,
    }
}

fn build_dorm_slots(blueprint: &BaseBlueprint, assignment: &BaseAssignment) -> Vec<MaaRoomSlot> {
    blueprint
        .rooms_of(FacilityKind::Dormitory)
        .into_iter()
        .map(|room| {
            let operators = operator_names(assignment, &room.id.0);
            if !operators.is_empty() {
                return MaaRoomSlot {
                    skip: false,
                    product: None,
                    operators,
                    sort: false,
                    autofill: false,
                };
            }
            MaaRoomSlot {
                skip: true,
                product: None,
                operators: Vec::new(),
                sort: false,
                autofill: false,
            }
        })
        .collect()
}

fn operator_names(assignment: &BaseAssignment, room_id: &str) -> Vec<String> {
    assignment
        .operators_in(&room_id.into())
        .iter()
        .map(|op| op.name.clone())
        .collect()
}

/// 菲亚梅塔换班目标解析。
///
/// 从 `priority`（公孙长乐提供的优先级清单，从高到低）里，找第一个
/// 出现在当班 `assignment` 里的干员作为换班目标；找到则 `enable: true`。
/// 优先级清单为空时退化为旧行为（`enable: false`，不换班）。
fn resolve_fiammetta(priority: &[String], assignment: &BaseAssignment) -> MaaFiammetta {
    if priority.is_empty() {
        return MaaFiammetta {
            enable: false,
            target: String::new(),
            order: "pre",
        };
    }
    let assigned: HashSet<_> = assignment_operator_names(assignment);
    let target = priority
        .iter()
        .find(|name| assigned.contains(name.as_str()))
        .cloned()
        .unwrap_or_default();
    MaaFiammetta {
        enable: !target.is_empty(),
        target,
        order: "pre",
    }
}

fn maa_trade_product(order: TradeOrderKind) -> &'static str {
    match order {
        TradeOrderKind::Gold => "LMD",
        TradeOrderKind::Originium => "Orundum",
    }
}

fn maa_factory_product(recipe: RecipeKind) -> &'static str {
    match recipe {
        RecipeKind::BattleRecord => "Battle Record",
        RecipeKind::Gold => "Pure Gold",
        RecipeKind::Originium => "Originium Shard",
        RecipeKind::All => "Pure Gold",
    }
}

fn team_label_zh(profile: TimedRotationProfile, label: TeamLabel) -> &'static str {
    if profile.is_two_team() {
        return match label {
            TeamLabel::Alpha => "主力",
            TeamLabel::Beta => "替补",
            TeamLabel::Gamma => "γ",
        };
    }
    match label {
        TeamLabel::Alpha => "α",
        TeamLabel::Beta => "β",
        TeamLabel::Gamma => "γ",
    }
}

// ── 一图流 / MAA 排班 JSON 导入 ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct MaaScheduleImport {
    #[serde(default)]
    pub title: Option<String>,
    pub plans: Vec<MaaPlanImport>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MaaPlanImport {
    #[serde(default)]
    pub name: String,
    pub rooms: MaaRoomsImport,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MaaRoomsImport {
    #[serde(default)]
    pub trading: Vec<MaaRoomSlotImport>,
    #[serde(default)]
    pub manufacture: Vec<MaaRoomSlotImport>,
    #[serde(default)]
    pub power: Vec<MaaRoomSlotImport>,
    #[serde(default)]
    pub dormitory: Vec<MaaRoomSlotImport>,
    #[serde(default)]
    pub control: Vec<MaaRoomSlotImport>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MaaRoomSlotImport {
    #[serde(default)]
    pub skip: bool,
    #[serde(default)]
    pub operators: Vec<String>,
}

pub fn load_maa_schedule(path: &Path) -> Result<MaaScheduleImport> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| Error::msg(format!("maa schedule read {}: {e}", path.display())))?;
    let mut raw = raw;
    if raw.starts_with('\u{feff}') {
        raw = raw.trim_start_matches('\u{feff}').to_string();
    }
    serde_json::from_str(&raw)
        .map_err(|e| Error::msg(format!("maa schedule parse {}: {e}", path.display())))
}

/// 将一图流 plan 转为 `BaseAssignment`；elite 取自 operbox 练度。
pub fn assignment_from_maa_plan(plan: &MaaPlanImport, operbox: &OperBox) -> BaseAssignment {
    let mut assignment = BaseAssignment::default();
    let rooms = &plan.rooms;

    for (i, slot) in rooms.trading.iter().enumerate() {
        if slot.skip {
            continue;
        }
        push_room_ops(&mut assignment, &format!("trade_{}", i + 1), slot, operbox);
    }
    for (i, slot) in rooms.manufacture.iter().enumerate() {
        if slot.skip {
            continue;
        }
        push_room_ops(&mut assignment, &format!("manu_{}", i + 1), slot, operbox);
    }
    for (i, slot) in rooms.power.iter().enumerate() {
        if slot.skip {
            continue;
        }
        push_room_ops(&mut assignment, &format!("power_{}", i + 1), slot, operbox);
    }
    for (i, slot) in rooms.dormitory.iter().enumerate() {
        if slot.skip {
            continue;
        }
        push_room_ops(&mut assignment, &format!("dorm_{}", i + 1), slot, operbox);
    }
    for slot in &rooms.control {
        if slot.skip {
            continue;
        }
        push_room_ops(&mut assignment, "control", slot, operbox);
    }

    assignment
}

fn push_room_ops(
    assignment: &mut BaseAssignment,
    room_id: &str,
    slot: &MaaRoomSlotImport,
    operbox: &OperBox,
) {
    let ops: Vec<AssignedOperator> = slot
        .operators
        .iter()
        .filter(|n| !n.is_empty())
        .map(|name| {
            let elite = operbox.elite_of(name).unwrap_or(0);
            AssignedOperator::new(name, elite)
        })
        .collect();
    if !ops.is_empty() {
        assignment.set_room(room_id, ops);
    }
}
