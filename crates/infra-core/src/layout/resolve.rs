use crate::control::{apply_control_to_layout, ControlOperator};
use std::sync::Arc;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId, RoomProduct};
use crate::layout::workforce::WorkforceIndex;
use crate::manufacture::input::ManuOperator;
use crate::pool::compile_operator_atoms;
use crate::power::{apply_power_to_layout, PowerOperator};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::global_resource::GlobalResourceKey;
use crate::layout::context::{LayoutContext, DEFAULT_DORM_OCCUPANT_COUNT};
use crate::trade::input::{TradeOperator, TradeOrderKind};
use crate::types::RecipeKind;

#[derive(Debug, Clone)]
pub struct ResolvedTradeRoom {
    pub id: RoomId,
    pub level: u8,
    pub order: TradeOrderKind,
    pub operators: Vec<TradeOperator>,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone)]
pub struct ResolvedManuRoom {
    pub id: RoomId,
    pub level: u8,
    pub recipe: RecipeKind,
    pub operators: Vec<ManuOperator>,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone)]
pub struct ResolvedPowerRoom {
    pub id: RoomId,
    pub operator: PowerOperator,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone)]
pub struct ResolvedBase {
    pub blueprint: BaseBlueprint,
    pub assignment: BaseAssignment,
    pub layout: LayoutContext,
    pub trade_rooms: Vec<ResolvedTradeRoom>,
    pub manu_rooms: Vec<ResolvedManuRoom>,
    pub power_rooms: Vec<ResolvedPowerRoom>,
}

impl ResolvedBase {
    pub fn gold_manu_line_count(&self) -> u32 {
        self.manu_rooms
            .iter()
            .filter(|r| r.recipe == RecipeKind::Gold)
            .count() as u32
    }

    pub fn layout_snapshot(&self) -> LayoutContext {
        self.layout.clone()
    }
}

pub fn resolve_base(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    mood: f64,
    durin_dorm_planning: Option<u8>,
) -> Result<ResolvedBase> {
    blueprint.validate()?;

    let workforce = WorkforceIndex::build(blueprint, assignment, instances);

    let mut layout = LayoutContext {
        meeting_max_level: blueprint.meeting_max_level(),
        training_room_level: blueprint.training_room_max_level(),
        dorm_level_sum: blueprint.dorm_level_sum(),
        facility_level_sum_excl_meeting: blueprint.facility_level_sum_excl_meeting(),
        manu_recipe_kinds: blueprint.manu_recipe_kinds(),
        elite_facility_count: effective_elite_facility_count(
            blueprint,
            assignment,
            &workforce,
            instances,
        ),
        sui_facility_count: blueprint.scenario.sui_facility_count.unwrap_or(0),
        dorm_occupant_count: effective_dorm_occupants(blueprint, assignment),
        trade_station_count: blueprint.count_facility(FacilityKind::TradePost),
        power_station_count: blueprint.count_facility(FacilityKind::PowerPlant),
        manufacture_station_count: blueprint.count_facility(FacilityKind::Factory),
        global: blueprint.initial_global_pool(),
        drone_cap: blueprint.drone_cap,
        ..Default::default()
    };
    workforce.apply_to_layout(&mut layout);
    if let Some(instances) = instances {
        workforce.apply_cross_facility_stats(&mut layout, blueprint, Some(instances));
    }
    if let Some(count) = durin_dorm_planning {
        layout.apply_durin_dorm_planning(count);
    }
    layout.gold_manu_line_count = blueprint.gold_manu_line_count();
    apply_wuyou_human_fireworks_baseline(&workforce, blueprint, &mut layout);
    if let Some(instances) = instances {
        apply_senxi_monster_cuisine_from_assignment(blueprint, assignment, instances, &mut layout);
        apply_perception_producers(blueprint, assignment, instances, &mut layout);
    }

    if let (Some(instances), Some(table)) = (instances, table) {
        let power_ops: Vec<(RoomId, PowerOperator)> = workforce
            .power_stations
            .iter()
            .map(|entry| {
                Ok((
                    entry.room_id.clone(),
                    to_power_operator(Some(instances), &entry.operator)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        apply_power_to_layout(
            &mut layout,
            &power_ops,
            table,
            mood,
            24.0,
            &workforce,
            instances,
        )?;

        let control_ops: Vec<ControlOperator> = assignment
            .control_operators()
            .iter()
            .map(|op| to_control_operator(instances, op))
            .collect::<Result<Vec<_>>>()?;
        if !control_ops.is_empty() {
            apply_control_to_layout(&mut layout, &control_ops, table, mood);
        }

        crate::office::apply_office_to_layout(
            &mut layout,
            blueprint,
            assignment,
            instances,
            table,
            mood,
        );
    }

    layout.global.run_conversions();

    let trade_rooms = build_trade_rooms(blueprint, assignment, instances, table, &layout);
    let manu_rooms = build_manu_rooms(blueprint, assignment, instances, &layout);
    let power_rooms = build_power_rooms(&workforce, instances, &layout);

    Ok(ResolvedBase {
        blueprint: blueprint.clone(),
        assignment: assignment.clone(),
        layout,
        trade_rooms,
        manu_rooms,
        power_rooms,
    })
}

pub fn resolve_search_baseline_layout() -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_243_use_this()?;
    let assignment = BaseAssignment::default();
    Ok(resolve_base(&blueprint, &assignment, None, None, 24.0, None)?.layout)
}

/// 怪猎联动搜索/评估基准：243c 物理布局 + 中枢怪猎双人 → 木天蓼 12。
pub fn snhunt_default_assignment() -> BaseAssignment {
    snhunt_control_assignment(0)
}

/// `elite`：0 = 精0 木天蓼链；2 = 精2 追加全贸易 +7% / 全制造 +2%（需双人同中枢）。
pub fn snhunt_control_assignment(elite: u8) -> BaseAssignment {
    let mut assignment = BaseAssignment::default();
    assignment.set_room(
        "control",
        vec![
            AssignedOperator::new("火龙S黑角", elite),
            AssignedOperator::new("麒麟R夜刀", elite),
        ],
    );
    assignment
}

pub fn resolve_snhunt_elite2_baseline_layout() -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_snhunt()?;
    let assignment = snhunt_control_assignment(2);
    let instances = OperatorInstances::load(&crate::instances::default_instances_path()?)?;
    let table = SkillTable::load(&crate::skill_table::default_skill_table_path()?)?;
    Ok(
        resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )?
        .layout,
    )
}

pub fn resolve_snhunt_baseline_layout() -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_snhunt()?;
    let assignment = snhunt_default_assignment();
    let instances = OperatorInstances::load(&crate::instances::default_instances_path()?)?;
    let table = SkillTable::load(&crate::skill_table::default_skill_table_path()?)?;
    Ok(
        resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )?
        .layout,
    )
}

pub fn resolve_automation_group_1_layout(
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_252_auto1()?;
    let mut assignment = BaseAssignment::default();
    assignment.set_power_operator("power_1", AssignedOperator::new("Lancet-2", 0));
    assignment.set_room("control", vec![AssignedOperator::new("森蚺", 2)]);
    Ok(resolve_base(
        &blueprint,
        &assignment,
        Some(instances),
        Some(table),
        24.0,
        None,
    )?
    .layout)
}

fn effective_elite_facility_count(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    workforce: &WorkforceIndex,
    instances: Option<&OperatorInstances>,
) -> u8 {
    if assignment.has_room_staffing() {
        return workforce.elite_facility_count(blueprint, instances);
    }
    blueprint
        .scenario
        .elite_facility_count
        .unwrap_or(0)
}

fn effective_dorm_occupants(blueprint: &BaseBlueprint, _assignment: &BaseAssignment) -> u8 {
    // 宿舍 producer 上岗 ≠ 全基建休息人数；黑键/迷迭香/乌有链按蓝图规划人数（默认 20）。
    blueprint
        .scenario
        .dorm_occupant_count
        .unwrap_or(DEFAULT_DORM_OCCUPANT_COUNT)
}

const WUYOU_OPERATOR_NAME: &str = "乌有";
const SENXI_DORM_CUISINE_BUFF: &str = "dorm_rec_bd_dungeon[000]";

/// 黑键·乐感（贸易站宿舍人数 → 感知信息）。
const BLACKKEY_PERCEPTION_BUFF: &str = "trade_ord_spd_bd_n1[000]";
/// 迷迭香·超感（制造站宿舍人数 → 感知信息）。
const ROSEMARY_PERCEPTION_BUFF: &str = "manu_prod_spd_bd_n1[000]";
/// 宿舍感知 producer（爱丽丝·梦境呓语 / 车尔尼·琴键漫步；每级 +1 感知，精2 生效）。
/// `skill_table` 未建模其感知子效果（主效果是心情恢复，属非目标），故按名在 resolve 注入。
const DORM_PERCEPTION_PRODUCERS: &[&str] = &["爱丽丝", "车尔尼"];
/// 办公室感知 producer（絮雨·追忆：记忆碎片 → 感知）。
/// 产出按办公室等级：Lv3 → 20，Lv2 → 10，Lv1 → 0，即 `(level-1)*10`（@公孙长乐 2026-06-15）。
/// 与爱丽丝/车尔尼同款：主效果（招募）属非目标，故不入 `skill_table`，按名在 resolve 注入。
const OFFICE_PERCEPTION_PRODUCER: &str = "絮雨";

/// 感知信息（全基建共享）汇总：所有 producer 写入 `layout.global`，
/// 由黑键/迷迭香各自读全量。中枢令/夕的感知已在 `apply_control_to_layout` 写回，这里补齐贸易/制造/宿舍侧。
///
/// 转化设施（黑键贸易 / 迷迭香制造）自身那份在房内 `state_produce` 重复计入，
/// 故在 `room_layout_for_*` 扣回 `dorm_occupant_count`（与乌有人间烟火同款）。
fn apply_perception_producers(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    layout: &mut LayoutContext,
) {
    let dorm = f64::from(layout.dorm_occupant_count);
    for room in &blueprint.rooms {
        for op in assignment.operators_in(&room.id) {
            let tier = PromotionTier::from_elite(op.elite);
            match room.kind {
                FacilityKind::TradePost => {
                    if instances
                        .resolve_trade_buff_ids(&op.name, tier)
                        .iter()
                        .any(|b| b == BLACKKEY_PERCEPTION_BUFF)
                    {
                        layout.global.add(GlobalResourceKey::Perception, dorm);
                    }
                }
                FacilityKind::Factory => {
                    if instances
                        .resolve_manufacture_buff_ids(&op.name, tier)
                        .iter()
                        .any(|b| b == ROSEMARY_PERCEPTION_BUFF)
                    {
                        layout.global.add(GlobalResourceKey::Perception, dorm);
                    }
                }
                FacilityKind::Dormitory => {
                    if op.elite >= 2 && DORM_PERCEPTION_PRODUCERS.contains(&op.name.as_str()) {
                        layout
                            .global
                            .add(GlobalResourceKey::Perception, f64::from(room.level));
                    }
                }
                FacilityKind::Office => {
                    if op.elite >= 2 && op.name == OFFICE_PERCEPTION_PRODUCER {
                        let amount = f64::from(room.level.saturating_sub(1)) * 10.0;
                        layout.global.add(GlobalResourceKey::Perception, amount);
                    }
                }
                _ => {}
            }
        }
    }
}

/// 森西精2「森西大食堂」：进驻宿舍时，该宿舍每级 → 1 层魔物料理（写入全局池）。
fn apply_senxi_monster_cuisine_from_assignment(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    layout: &mut LayoutContext,
) {
    let mut layers = 0.0f64;
    for room in &blueprint.rooms {
        if room.kind != FacilityKind::Dormitory {
            continue;
        }
        for op in assignment.operators_in(&room.id) {
            if op.elite < 2 {
                continue;
            }
            let tier = PromotionTier::from_elite(op.elite);
            let buffs = instances.resolve_dorm_buff_ids(&op.name, tier);
            if buffs.iter().any(|b| b == SENXI_DORM_CUISINE_BUFF) {
                layers += f64::from(room.level);
            }
        }
    }
    if layers > 0.0 {
        layout
            .global
            .set(GlobalResourceKey::MonsterCuisine, layers);
    }
}

/// 乌有在任意贸易站上岗时，将其产出的人间烟火注入全局池（供铎铃等 consumer 读取）。
fn apply_wuyou_human_fireworks_baseline(
    workforce: &WorkforceIndex,
    blueprint: &BaseBlueprint,
    layout: &mut LayoutContext,
) {
    if !wuyou_in_trade_workforce(workforce, blueprint) {
        return;
    }
    layout.global.add(
        GlobalResourceKey::HumanFireworks,
        f64::from(layout.dorm_occupant_count),
    );
}

fn wuyou_in_trade_workforce(workforce: &WorkforceIndex, blueprint: &BaseBlueprint) -> bool {
    blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::TradePost)
        .any(|room| {
            workforce
                .by_room
                .get(&room.id.0)
                .is_some_and(|ops| ops.iter().any(|op| op.name == WUYOU_OPERATOR_NAME))
        })
}

fn room_layout_for_trade(
    layout: &LayoutContext,
    operators: &[TradeOperator],
) -> LayoutContext {
    let mut room_layout = layout.clone();
    if operators_include_wuyou_producer(operators) {
        room_layout.global.add(
            GlobalResourceKey::HumanFireworks,
            -f64::from(layout.dorm_occupant_count),
        );
    }
    // 黑键自产那份感知已在 `apply_perception_producers` 计入全局，房内 atom 会再产一次，故扣回。
    if operators
        .iter()
        .any(|op| op.buff_ids.iter().any(|b| b == BLACKKEY_PERCEPTION_BUFF))
    {
        room_layout.global.add(
            GlobalResourceKey::Perception,
            -f64::from(layout.dorm_occupant_count),
        );
    }
    room_layout
}

fn operators_include_wuyou_producer(operators: &[TradeOperator]) -> bool {
    operators
        .iter()
        .any(|op| op.name == WUYOU_OPERATOR_NAME && op.elite >= 2)
}

/// 迷迭香制造房：扣回其自产感知（与 `room_layout_for_trade` 的黑键处理对称）。
fn room_layout_for_manu(layout: &LayoutContext, operators: &[ManuOperator]) -> LayoutContext {
    let mut room_layout = layout.clone();
    if operators
        .iter()
        .any(|op| op.buff_ids.iter().any(|b| b == ROSEMARY_PERCEPTION_BUFF))
    {
        room_layout.global.add(
            GlobalResourceKey::Perception,
            -f64::from(layout.dorm_occupant_count),
        );
    }
    room_layout
}

fn build_trade_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    layout: &LayoutContext,
) -> Vec<ResolvedTradeRoom> {
    blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .filter_map(|room| {
            let RoomProduct::Trade { order } = room.product.as_ref()? else {
                return None;
            };
            let operators: Vec<TradeOperator> = assignment
                .operators_in(&room.id)
                .iter()
                .filter_map(|op| to_trade_operator(instances, table, op).ok())
                .collect();
            Some(ResolvedTradeRoom {
                id: room.id.clone(),
                level: room.level,
                order: *order,
                layout: room_layout_for_trade(layout, &operators),
                operators,
            })
        })
        .collect()
}

fn build_manu_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    layout: &LayoutContext,
) -> Vec<ResolvedManuRoom> {
    blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::Factory)
        .filter_map(|room| {
            let RoomProduct::Factory { recipe } = room.product.as_ref()? else {
                return None;
            };
            let operators: Vec<ManuOperator> = assignment
                .operators_in(&room.id)
                .iter()
                .filter_map(|op| to_manu_operator(instances, op).ok())
                .collect();
            Some(ResolvedManuRoom {
                id: room.id.clone(),
                level: room.level,
                recipe: *recipe,
                layout: room_layout_for_manu(layout, &operators),
                operators,
            })
        })
        .collect()
}

fn build_power_rooms(
    workforce: &WorkforceIndex,
    instances: Option<&OperatorInstances>,
    layout: &LayoutContext,
) -> Vec<ResolvedPowerRoom> {
    workforce
        .power_stations
        .iter()
        .filter_map(|entry| {
            let op = to_power_operator(instances, &entry.operator).ok()?;
            let room_layout = workforce.layout_for_power_room(
                layout,
                &entry.room_id,
                &entry.operator.name,
                instances,
            );
            Some(ResolvedPowerRoom {
                id: entry.room_id.clone(),
                operator: op,
                layout: room_layout,
            })
        })
        .collect()
}

fn to_trade_operator(
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    op: &AssignedOperator,
) -> Result<TradeOperator> {
    let tier = PromotionTier::from_elite(op.elite);
    let buff_ids = resolve_buff_ids(instances, op, "trade")?;
    let compiled_atoms = table
        .map(|t| compile_operator_atoms(&buff_ids, t))
        .unwrap_or_else(|| Arc::from([]));
    Ok(TradeOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
        tags: instance_tags(instances, &op.name, tier),
        compiled_atoms,
    })
}

fn to_manu_operator(
    instances: Option<&OperatorInstances>,
    op: &AssignedOperator,
) -> Result<ManuOperator> {
    let tier = PromotionTier::from_elite(op.elite);
    let buff_ids = resolve_buff_ids(instances, op, "manufacture")?;
    let tags = instance_tags(instances, &op.name, tier);
    Ok(ManuOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
        tags,
    })
}

fn to_power_operator(
    instances: Option<&OperatorInstances>,
    op: &AssignedOperator,
) -> Result<PowerOperator> {
    let buff_ids = resolve_buff_ids(instances, op, "power")?;
    Ok(PowerOperator::new(op.name.clone(), op.elite, buff_ids))
}

fn to_control_operator(
    instances: &OperatorInstances,
    op: &AssignedOperator,
) -> Result<ControlOperator> {
    let tier = PromotionTier::from_elite(op.elite);
    let buff_ids = instances.resolve_control_buff_ids(&op.name, tier);
    let tags = instances
        .get(&op.name, tier)
        .map(|i| i.tags.clone())
        .unwrap_or_default();
    Ok(ControlOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
        tags,
    })
}

fn instance_tags(
    instances: Option<&OperatorInstances>,
    name: &str,
    tier: PromotionTier,
) -> Vec<String> {
    instances
        .and_then(|inst| inst.get(name, tier))
        .map(|i| i.tags.clone())
        .unwrap_or_default()
}

fn resolve_buff_ids(
    instances: Option<&OperatorInstances>,
    op: &AssignedOperator,
    facility: &str,
) -> Result<Vec<String>> {
    let Some(instances) = instances else {
        return Ok(Vec::new());
    };
    let tier = PromotionTier::from_elite(op.elite);
    let ids = match facility {
        "trade" => instances.resolve_trade_buff_ids(&op.name, tier),
        "manufacture" => instances.resolve_manufacture_buff_ids(&op.name, tier),
        "power" => instances.resolve_power_buff_ids(&op.name, tier),
        _ => Vec::new(),
    };
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::assignment::{AssignedOperator, BaseAssignment};
    use crate::layout::blueprint::BaseBlueprint;
    use crate::operbox::{default_operbox_gongsun_path, OperBox};
    use crate::skill_table::{data_path, SkillTable};

    fn pair() -> (OperatorInstances, SkillTable) {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap())
            .unwrap();
        (instances, table)
    }

    #[test]
    fn perception_chain_aggregates_across_facilities() {
        use crate::manufacture::{solve_manufacture, ManuRoomInput};
        use crate::types::RecipeKind;

        let (instances, table) = pair();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("trade_1", vec![AssignedOperator::new("黑键", 2)]);
        assignment.set_room("manu_4", vec![AssignedOperator::new("迷迭香", 2)]);
        assignment.set_room("control", vec![AssignedOperator::new("夕", 2)]);
        assignment.set_room(
            "dorm_1",
            vec![
                AssignedOperator::new("爱丽丝", 2),
                AssignedOperator::new("车尔尼", 2),
            ],
        );

        let resolved = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap();

        // 全局感知 = 迷迭香20 + 黑键20 + 夕10(心情>12) + 爱丽丝3 + 车尔尼3（Lv3 宿舍每级+1）= 56。
        assert!(
            (resolved.layout.global.get(GlobalResourceKey::Perception) - 56.0).abs() < f64::EPSILON,
            "全基建共享感知应为 56, got {}",
            resolved.layout.global.get(GlobalResourceKey::Perception)
        );

        // 迷迭香制造房：扣回自产 20 → 房内快照 36，atom 再产 20 → 全量 56；意识实体 /1 = +56%。
        let manu = resolved
            .manu_rooms
            .iter()
            .find(|r| r.id.0 == "manu_4")
            .unwrap();
        assert!(
            (manu.layout.global.get(GlobalResourceKey::Perception) - 36.0).abs() < f64::EPSILON,
            "迷迭香房应扣回自产, got {}",
            manu.layout.global.get(GlobalResourceKey::Perception)
        );
        let manu_input = ManuRoomInput {
            level: manu.level,
            operators: manu.operators.clone(),
            active_recipe: RecipeKind::Gold,
            mood: 24.0,
            layout: std::sync::Arc::new(manu.layout.clone()),
        };
        let manu_result = solve_manufacture(&manu_input, &table).unwrap();
        assert!(
            (manu_result.prod_skill - 56.0).abs() < 0.01,
            "迷迭香跨设施感知 → +56% 生产力, got {}",
            manu_result.prod_skill
        );

        // 黑键贸易房同样扣回自产 → 读全量 56；怅惘和声 /2 = +28%。
        let trade = resolved
            .trade_rooms
            .iter()
            .find(|r| r.id.0 == "trade_1")
            .unwrap();
        assert!(
            (trade.layout.global.get(GlobalResourceKey::Perception) - 36.0).abs() < f64::EPSILON,
            "黑键房应扣回自产, got {}",
            trade.layout.global.get(GlobalResourceKey::Perception)
        );
    }

    #[test]
    fn office_xuyu_perception_chain_drives_rosemary_and_blackkey() {
        use crate::manufacture::{solve_manufacture, ManuRoomInput};
        use crate::types::RecipeKind;

        let (instances, table) = pair();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("trade_1", vec![AssignedOperator::new("黑键", 2)]);
        assignment.set_room("manu_4", vec![AssignedOperator::new("迷迭香", 2)]);
        assignment.set_room("control", vec![AssignedOperator::new("夕", 2)]);
        assignment.set_room(
            "dorm_1",
            vec![
                AssignedOperator::new("爱丽丝", 2),
                AssignedOperator::new("车尔尼", 2),
            ],
        );
        assignment.set_room("office_1", vec![AssignedOperator::new("絮雨", 2)]);

        let resolved = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap();

        // 三核心+夕+爱丽丝+车尔尼 = 56；絮雨办公室(Lv3 → (3-1)*10 = 20) → 76。
        assert!(
            (resolved.layout.global.get(GlobalResourceKey::Perception) - 76.0).abs() < f64::EPSILON,
            "絮雨办公室应 +20 感知 → 76, got {}",
            resolved.layout.global.get(GlobalResourceKey::Perception)
        );

        // 迷迭香房扣回自产 → 读全量 76；意识实体 /1 = +76% 生产力（文档 §7.1「整十」满配上限 ~75-80%）。
        let manu = resolved.manu_rooms.iter().find(|r| r.id.0 == "manu_4").unwrap();
        let manu_result = solve_manufacture(
            &ManuRoomInput {
                level: manu.level,
                operators: manu.operators.clone(),
                active_recipe: RecipeKind::Gold,
                mood: 24.0,
                layout: std::sync::Arc::new(manu.layout.clone()),
            },
            &table,
        )
        .unwrap();
        assert!(
            (manu_result.prod_skill - 76.0).abs() < 0.01,
            "迷迭香感知链 → +76% 生产力, got {}",
            manu_result.prod_skill
        );

        // 黑键房读全量 76；怅惘和声 /2 = +38% 贸易效率（文档 §7.2 ~37.5-40%）。
        let trade = resolved.trade_rooms.iter().find(|r| r.id.0 == "trade_1").unwrap();
        assert!(
            (trade.layout.global.get(GlobalResourceKey::Perception) - 56.0).abs() < f64::EPSILON,
            "黑键房应扣回自产 20 → 56, got {}",
            trade.layout.global.get(GlobalResourceKey::Perception)
        );
    }

    #[test]
    fn snhunt_baseline_produces_matatabi_from_control() {
        let layout = resolve_snhunt_baseline_layout().unwrap();
        assert_eq!(layout.trade_station_count, 3);
        assert_eq!(layout.dorm_occupant_count, 20);
        assert!(
            (layout.global.get(GlobalResourceKey::Matatabi) - 12.0).abs() < f64::EPSILON,
            "火龙S黑角 2×2 + 麒麟R夜刀 8 = 12, got {}",
            layout.global.get(GlobalResourceKey::Matatabi)
        );
    }

    #[test]
    fn snhunt_elite2_baseline_global_inject() {
        let layout = resolve_snhunt_elite2_baseline_layout().unwrap();
        assert!((layout.global.get(GlobalResourceKey::Matatabi) - 12.0).abs() < f64::EPSILON);
        assert!((layout.global_inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
        assert!(
            (layout.global_inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn search_baseline_matches_legacy_aggregates() {
        let layout = resolve_search_baseline_layout().unwrap();
        assert_eq!(layout.trade_station_count, 2);
        assert_eq!(layout.power_station_count, 3);
        assert_eq!(layout.manufacture_station_count, 4);
        assert_eq!(layout.manu_recipe_kinds, 2);
        assert_eq!(layout.meeting_max_level, 3);
        assert_eq!(layout.dorm_level_sum, 12);
        assert_eq!(layout.dorm_occupant_count, 20);
        assert_eq!(layout.elite_facility_count, 0);
        assert_eq!(layout.sui_facility_count, 2);
        assert_eq!(layout.drone_cap, 135);
        assert_eq!(layout.gold_manu_line_count, 2);
        assert_eq!(layout.durin_in_base, 0);
        assert_eq!(layout.facility_level_sum_excl_meeting, 45);
        assert!((layout.global.get(crate::global_resource::GlobalResourceKey::MonsterCuisine)
            - 3.0)
            .abs()
            < f64::EPSILON);
        assert!(layout.base_workforce.is_empty());
    }

    #[test]
    fn senxi_in_dorm_produces_monster_cuisine_from_room_level() {
        let (instances, table) = pair();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("dorm_1", vec![AssignedOperator::new("森西", 2)]);
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
            (layout.global.get(GlobalResourceKey::MonsterCuisine) - 3.0).abs() < f64::EPSILON,
            "Lv3 宿舍 + 森西精2 → 3 层魔物料理"
        );
        assert_eq!(
            layout.dorm_occupant_count, 20,
            "宿舍 producer 上岗不应把 dorm_occupant_count 压成上岗人数"
        );
    }

    #[test]
    fn durin_in_base_counts_workforce_excluding_training_assist() {
        let (instances, table) = pair();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = vec![
            "杜林".into(),
            "桃金娘".into(),
            "至简".into(),
            "褐果".into(),
        ];
        assignment.training_assist = Some(AssignedOperator::new("杜林", 0));
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
        assert_eq!(layout.durin_in_base, 3, "副手杜林不计入际崖居民");
        assert_eq!(layout.gold_manu_line_count, 2);
    }

    #[test]
    fn automation_group_1_elite_facility_only_for_elite_operators() {
        let (instances, table) = pair();
        let layout = resolve_automation_group_1_layout(&instances, &table).unwrap();
        assert_eq!(
            layout.elite_facility_count, 0,
            "森蚺不是游戏术语「精英干员」，不应计入"
        );

        let blueprint = BaseBlueprint::template_252_auto1().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("电弧", 2)]);
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
        assert_eq!(layout.elite_facility_count, 1);
    }

    #[test]
    fn durin_dorm_planning_from_operbox_floors_layout() {
        let (instances, table) = pair();
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let durin_plan = operbox.durin_dorm_planning_count(&instances);
        assert!(
            durin_plan >= 4,
            "gongsun operbox should own at least 4 durin-tagged operators"
        );
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let assignment = BaseAssignment::default();
        let layout = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            Some(durin_plan),
        )
        .unwrap()
        .layout;
        assert_eq!(layout.durin_in_base, 4);
    }

    #[test]
    fn automation_group_1_virtual_power_from_saria() {
        let (instances, table) = pair();
        let layout = resolve_automation_group_1_layout(&instances, &table).unwrap();
        assert_eq!(layout.trade_station_count, 2);
        assert_eq!(layout.power_station_count, 3);
        assert!(layout.power_workforce.iter().any(|n| n == "Lancet-2"));
        assert!(
            (layout.global.get(crate::global_resource::GlobalResourceKey::VirtualPower) - 2.0)
                .abs()
                < f64::EPSILON,
            "森蚺中枢 + Lancet-2 → VirtualPower +2"
        );
        assert_eq!(layout.effective_power_station_count(), 5);
    }

    #[test]
    fn ideal_e2_automation_trio_full_resolve_breakdown() {
        use crate::manufacture::input::ManuRoomInput;
        use crate::manufacture::solver::solve_manufacture;
        use crate::types::RecipeKind;

        let (instances, table) = pair();
        let blueprint =
            BaseBlueprint::load(&data_path("layout/243_use_this_.json").unwrap()).unwrap();
        let assignment =
            BaseAssignment::load(&data_path("schedule_243/assignment_automation_trio_e2.json").unwrap())
                .unwrap();
        let operbox = OperBox::load(&data_path("schedule_243/operbox_ideal_e2.json").unwrap()).unwrap();
        let durin_plan = operbox.durin_dorm_planning_count(&instances);
        let resolved = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            Some(durin_plan),
        )
        .unwrap();
        let layout = &resolved.layout;
        eprintln!(
            "trade={} eff_power={} virtual_power={} global_manu={}",
            layout.trade_station_count,
            layout.effective_power_station_count(),
            layout.global.get(crate::global_resource::GlobalResourceKey::VirtualPower),
            layout.global_inject.manu_eff_for(RecipeKind::Gold)
        );
        let manu = resolved
            .manu_rooms
            .iter()
            .find(|r| r.id.0 == "manu_3")
            .expect("manu_3");
        let input = ManuRoomInput {
            level: manu.level,
            operators: manu.operators.clone(),
            active_recipe: manu.recipe,
            mood: 24.0,
            layout: std::sync::Arc::new(manu.layout.clone()),
        };
        let result = solve_manufacture(&input, &table).unwrap();
        eprintln!(
            "ops={:?} buffs={:?}",
            input.operators.iter().map(|o| &o.name).collect::<Vec<_>>(),
            input
                .operators
                .iter()
                .map(|o| (&o.name, &o.buff_ids))
                .collect::<Vec<_>>()
        );
        eprintln!(
            "prod_base={} prod_skill={} prod_total={} storage={}",
            result.prod_base, result.prod_skill, result.prod_total, result.storage_limit
        );
        // 目标：冬时30 + 清流40 + 温蒂4×15=60 → prod_skill 130
        assert!(
            (result.prod_skill - 130.0).abs() < 1.0,
            "full resolve prod_skill={} prod_total={}",
            result.prod_skill,
            result.prod_total
        );
    }

    #[test]
    fn rainbow_usaut_from_control_reaches_fuse_storage() {
        use crate::manufacture::input::ManuRoomInput;
        use crate::manufacture::solver::solve_manufacture;
        use crate::types::RecipeKind;

        let (instances, table) = pair();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "control",
            vec![
                AssignedOperator::new("战车", 2),
                AssignedOperator::new("凛冬", 2),
            ],
        );
        assignment.set_room("manu_3", vec![AssignedOperator::new("导火索", 2)]);
        let resolved = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap();
        assert!(
            (resolved.layout.global.get(GlobalResourceKey::UsautDrink) - 1.0).abs()
                < f64::EPSILON
        );
        let manu = resolved
            .manu_rooms
            .iter()
            .find(|r| r.id.0 == "manu_3")
            .expect("manu_3");
        let input = ManuRoomInput {
            level: manu.level,
            operators: manu.operators.clone(),
            active_recipe: manu.recipe,
            mood: 24.0,
            layout: std::sync::Arc::new(manu.layout.clone()),
        };
        let result = solve_manufacture(&input, &table).unwrap();
        assert_eq!(result.storage_limit, 22, "20 base + 2 per UsautDrink");
        assert_eq!(manu.recipe, RecipeKind::Gold);
    }

    #[test]
    fn rainbow_intelligence_reaches_blitz_office_hire() {
        let (instances, table) = pair();
        // 243_use_this_ 已含 office_1 (Lv3)；此处直接复用，不再手动追加。
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "control",
            vec![
                AssignedOperator::new("灰烬", 2),
                AssignedOperator::new("霜华", 0),
                AssignedOperator::new("战车", 2),
                AssignedOperator::new("凛冬", 2),
            ],
        );
        assignment.set_room("office_1", vec![AssignedOperator::new("闪击", 2)]);
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
            (layout.global.get(GlobalResourceKey::IntelligenceReserve) - 3.0).abs()
                < f64::EPSILON,
            "灰烬+霜华+战车 → 3 情报储备, got {}",
            layout.global.get(GlobalResourceKey::IntelligenceReserve)
        );
        assert!(
            (layout.global.get(GlobalResourceKey::UsautDrink) - 1.0).abs() < f64::EPSILON
        );
        assert!(
            (layout.office_hire_spd_pct - 40.0).abs() < f64::EPSILON,
            "20 + 3×5 + 1×5 = 40, got {}",
            layout.office_hire_spd_pct
        );
    }
}
