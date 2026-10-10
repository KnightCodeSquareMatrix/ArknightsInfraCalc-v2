use std::collections::HashMap;

use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};
use crate::layout::context::trade_station_tagged_gte_key;
use crate::layout::LayoutContext;

/// 真言「精英小队」/风絮「孺子可教」等：会客室（活动室）不计入进驻设施。
fn facility_counts_for_layout_stats(kind: FacilityKind) -> bool {
    !matches!(kind, FacilityKind::MeetingRoom)
}

/// 副手不计入「该设施是否进驻精英干员」判定；控制中枢仅首领位。
fn operators_for_facility_stat(
    kind: FacilityKind,
    ops: &[AssignedOperator],
) -> &[AssignedOperator] {
    match kind {
        FacilityKind::ControlCenter => ops.get(..1).unwrap_or(&[]),
        _ => ops,
    }
}

/// 游戏术语「精英干员」：迷迭香、煌、逻各斯、烛煌、电弧、真言（非精英化练度）。
pub const TAG_ELITE_OPERATOR: &str = "cc.g.elite_op";

const ELITE_OPERATOR_NAMES: &[&str] = &["迷迭香", "煌", "逻各斯", "烛煌", "电弧", "真言"];

/// 游戏内作业平台（机器人）干员名。
const PLATFORM_OPERATOR_NAMES: &[&str] =
    &["Lancet-2", "Castle-3", "PhonoR-0", "THRM-EX", "正义骑士号"];

const TAG_LATERANO: &str = "cc.g.laterano";
pub const TAG_RHINE: &str = "cc.g.rhine";
pub const TAG_DURIN: &str = "cc.g.durin";
pub const TAG_KARLAN: &str = "cc.g.karlan";
pub const TAG_GLASGOW: &str = "cc.g.glasgow";
pub const TAG_SIRACUSA: &str = "cc.g.siracusa";
pub const TAG_BLACKSTEEL: &str = "cc.g.blacksteel";
pub const TAG_KNIGHT: &str = "cc.g.knight";
pub const TAG_PINUS: &str = "cc.g.pinus";
pub const TAG_ABYSSAL: &str = "cc.g.abyssal";

const CROSS_FACILITY_TAGS: &[&str] = &[
    TAG_KARLAN,
    TAG_GLASGOW,
    TAG_SIRACUSA,
    TAG_BLACKSTEEL,
    TAG_KNIGHT,
    TAG_PINUS,
    TAG_ABYSSAL,
];

#[derive(Debug, Clone)]
pub struct PowerStationEntry {
    pub room_id: RoomId,
    pub operator: AssignedOperator,
}

#[derive(Debug, Clone)]
pub struct WorkforceIndex {
    pub by_room: HashMap<String, Vec<AssignedOperator>>,
    pub power_stations: Vec<PowerStationEntry>,
    pub power_workforce: Vec<String>,
    pub control_workforce: Vec<String>,
    pub control_operators: Vec<(String, u8)>,
    pub platform_rooms: Vec<RoomId>,
    pub all_base_names: Vec<String>,
    /// 有效“在基建内”presence：排除会客室使用者和训练副手。
    pub effective_base_presence: Vec<String>,
    pub rhine_life_in_base: u8,
    /// 基建内杜林族干员数（际崖居民；不含副手，至多 4）。
    pub durin_in_base: u8,
    pub training_assist: Vec<String>,
    pub trade_workforce: Vec<String>,
    pub manu_workforce: Vec<String>,
}

impl WorkforceIndex {
    pub fn build(
        blueprint: &BaseBlueprint,
        assignment: &BaseAssignment,
        instances: Option<&OperatorInstances>,
    ) -> Self {
        let mut by_room: HashMap<String, Vec<AssignedOperator>> = HashMap::new();
        let mut power_stations = Vec::new();
        let mut power_workforce = Vec::new();
        let mut control_workforce = Vec::new();
        let mut control_operators = Vec::new();
        let mut platform_rooms = Vec::new();
        let mut trade_workforce = Vec::new();
        let mut manu_workforce = Vec::new();

        for room in &blueprint.rooms {
            let ops = assignment.operators_in(&room.id);
            if !ops.is_empty() {
                by_room.insert(room.id.0.clone(), ops.to_vec());
            }
            if room.kind == FacilityKind::TradePost {
                for op in ops {
                    trade_workforce.push(op.name.clone());
                }
            }
            if room.kind == FacilityKind::Factory {
                for op in ops {
                    manu_workforce.push(op.name.clone());
                }
            }
            if room.kind == FacilityKind::ControlCenter {
                for op in ops {
                    control_workforce.push(op.name.clone());
                    control_operators.push((op.name.clone(), op.elite));
                }
            }
            if room.kind == FacilityKind::PowerPlant {
                for op in ops {
                    power_workforce.push(op.name.clone());
                    power_stations.push(PowerStationEntry {
                        room_id: room.id.clone(),
                        operator: op.clone(),
                    });
                    // 显式 0 心情的 Lancet-2 仍在发电站、可触发中枢森蚺，
                    // 但不作为“其他发电站作业平台”阻断晨曦。
                    if is_platform_operator(&op.name) && op.work_mood != Some(0) {
                        platform_rooms.push(room.id.clone());
                    }
                }
            }
        }

        let mut all_base_names: Vec<String> = if assignment.base_workforce.is_empty() {
            blueprint.scenario.base_workforce.clone()
        } else {
            assignment.base_workforce.clone()
        };
        let training_assist = assignment
            .training_assist
            .as_ref()
            .map(|a| vec![a.name.clone()])
            .unwrap_or_default();
        let meeting_names: std::collections::HashSet<String> = blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::MeetingRoom)
            .flat_map(|room| assignment.operators_in(&room.id))
            .map(|operator| operator.name.clone())
            .collect();
        let mut effective_base_presence = all_base_names
            .iter()
            .filter(|name| !training_assist.iter().any(|assist| assist == *name))
            .filter(|name| !meeting_names.contains(*name))
            .cloned()
            .collect::<Vec<_>>();
        for ops in by_room.values() {
            for op in ops {
                if !all_base_names.iter().any(|n| n == &op.name) {
                    all_base_names.push(op.name.clone());
                }
            }
        }
        for room in &blueprint.rooms {
            if room.kind == FacilityKind::MeetingRoom {
                continue;
            }
            for op in assignment.operators_in(&room.id) {
                if !training_assist.iter().any(|assist| assist == &op.name)
                    && !meeting_names.contains(&op.name)
                    && !effective_base_presence.iter().any(|name| name == &op.name)
                {
                    effective_base_presence.push(op.name.clone());
                }
            }
        }

        // 莱茵技能的“基建内”排除会客室使用者和训练副手；
        // effective_base_presence 已完成这两个排除。
        let rhine_life_in_base =
            count_tagged_in_base_excluding(instances, &effective_base_presence, &[], TAG_RHINE, 6);

        let assigned_presence: Vec<_> = blueprint
            .rooms
            .iter()
            .filter(|room| room.kind != FacilityKind::MeetingRoom)
            .flat_map(|room| assignment.operators_in(&room.id))
            .filter(|operator| !training_assist.contains(&operator.name))
            .map(|operator| operator.name.clone())
            .collect();
        let durin_in_base =
            count_tagged_in_base_excluding(instances, &assigned_presence, &[], TAG_DURIN, 4);

        Self {
            by_room,
            power_stations,
            power_workforce,
            control_workforce,
            control_operators,
            platform_rooms,
            all_base_names,
            effective_base_presence,
            rhine_life_in_base,
            durin_in_base,
            training_assist,
            trade_workforce,
            manu_workforce,
        }
    }

    pub fn platform_count_in_power(&self) -> u8 {
        self.platform_rooms.len().min(255) as u8
    }

    pub fn other_power_has_platform(&self, excluding_room: &RoomId) -> bool {
        self.platform_rooms.iter().any(|id| id != excluding_room)
    }

    pub fn other_platform_in_power(&self, viewing_room: &RoomId, viewing_name: &str) -> bool {
        if is_platform_operator(viewing_name) {
            return false;
        }
        self.power_stations.iter().any(|entry| {
            entry.room_id != *viewing_room && is_platform_operator(&entry.operator.name)
        })
    }

    pub fn other_laterano_in_power(
        &self,
        instances: Option<&OperatorInstances>,
        viewing_room: &RoomId,
        viewing_name: &str,
    ) -> bool {
        self.power_stations.iter().any(|entry| {
            entry.room_id != *viewing_room
                && entry.operator.name != viewing_name
                && operator_has_tag(instances, &entry.operator, TAG_LATERANO)
        })
    }

    /// 真言「精英小队」：基建内（不含副手、活动室）每有一间进驻精英干员的设施 +2%（至多 10 间）。
    pub fn elite_facility_count(
        &self,
        blueprint: &BaseBlueprint,
        instances: Option<&OperatorInstances>,
    ) -> u8 {
        blueprint
            .rooms
            .iter()
            .filter(|room| facility_counts_for_layout_stats(room.kind))
            .filter(|room| {
                let ops = self
                    .by_room
                    .get(&room.id.0)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                operators_for_facility_stat(room.kind, ops)
                    .iter()
                    .any(|op| is_elite_operator(instances, op))
            })
            .count()
            .min(255) as u8
    }

    pub fn apply_to_layout(&self, layout: &mut LayoutContext) {
        layout.power_workforce = self.power_workforce.clone();
        layout.control_workforce = self.control_workforce.clone();
        layout.control_operators = self.control_operators.clone();
        layout.base_workforce = self.effective_base_presence.clone();
        layout.training_assist = self.training_assist.clone();
        layout.rhine_life_in_base = self.rhine_life_in_base;
        layout.durin_in_base = self.durin_in_base;
        layout.platform_count_in_power = self.platform_count_in_power();
        layout.trade_workforce = self.trade_workforce.clone();
        layout.manu_workforce = self.manu_workforce.clone();
    }

    pub fn apply_cross_facility_stats(
        &self,
        layout: &mut LayoutContext,
        blueprint: &BaseBlueprint,
        instances: Option<&OperatorInstances>,
    ) {
        let mut trade_tagged_count_sum = HashMap::new();
        let mut trade_stations_tagged_gte = HashMap::new();
        let mut manu_tagged_count_sum = HashMap::new();

        for tag in CROSS_FACILITY_TAGS {
            let trade_sum = sum_tagged_in_facility_rooms(
                instances,
                blueprint,
                &self.by_room,
                FacilityKind::TradePost,
                tag,
            );
            if trade_sum > 0 {
                trade_tagged_count_sum.insert((*tag).to_string(), trade_sum);
            }
            let stations_gte3 = count_facility_rooms_tagged_gte(
                instances,
                blueprint,
                &self.by_room,
                FacilityKind::TradePost,
                tag,
                3,
            );
            if stations_gte3 > 0 {
                trade_stations_tagged_gte
                    .insert(trade_station_tagged_gte_key(tag, 3), stations_gte3);
            }
            let manu_sum = sum_tagged_in_facility_rooms(
                instances,
                blueprint,
                &self.by_room,
                FacilityKind::Factory,
                tag,
            );
            if manu_sum > 0 {
                manu_tagged_count_sum.insert((*tag).to_string(), manu_sum);
            }
        }

        layout.trade_tagged_count_sum = trade_tagged_count_sum;
        layout.trade_stations_tagged_gte = trade_stations_tagged_gte;
        layout.manu_tagged_count_sum = manu_tagged_count_sum;
    }

    pub fn layout_for_power_room(
        &self,
        base: &LayoutContext,
        room_id: &RoomId,
        operator_name: &str,
        instances: Option<&OperatorInstances>,
    ) -> LayoutContext {
        crate::profile::record_resolve_construction(0, 1, 0, 0, 1);
        let mut layout = base.clone();
        layout.other_power_has_platform = self.other_power_has_platform(room_id);
        layout.other_platform_in_power = self.other_platform_in_power(room_id, operator_name);
        layout.other_laterano_in_power =
            self.other_laterano_in_power(instances, room_id, operator_name);
        layout
    }
}

pub fn is_platform_operator(name: &str) -> bool {
    PLATFORM_OPERATOR_NAMES.contains(&name)
}

pub fn is_elite_operator(instances: Option<&OperatorInstances>, op: &AssignedOperator) -> bool {
    if operator_has_tag(instances, op, TAG_ELITE_OPERATOR) {
        return true;
    }
    if let Some(instances) = instances {
        for tier in [
            crate::tier::PromotionTier::Tier0,
            crate::tier::PromotionTier::TierUp,
        ] {
            if let Some(inst) = instances.get(&op.name, tier) {
                if inst.tags.iter().any(|t| t == TAG_ELITE_OPERATOR) {
                    return true;
                }
            }
        }
    }
    ELITE_OPERATOR_NAMES.contains(&op.name.as_str())
}

fn operator_has_tag(
    instances: Option<&OperatorInstances>,
    op: &AssignedOperator,
    tag: &str,
) -> bool {
    let Some(instances) = instances else {
        return false;
    };
    instances
        .tags_for(&op.name, op.tier())
        .iter()
        .any(|t| t == tag)
}

fn count_tagged_in_base_excluding(
    instances: Option<&OperatorInstances>,
    names: &[String],
    exclude: &[String],
    tag: &str,
    cap: u8,
) -> u8 {
    let Some(instances) = instances else {
        return 0;
    };
    names
        .iter()
        .filter(|name| !exclude.iter().any(|e| e == *name))
        .filter(|name| {
            for tier in [
                crate::tier::PromotionTier::Tier0,
                crate::tier::PromotionTier::TierUp,
            ] {
                if let Some(inst) = instances.get(name, tier) {
                    if inst.tags.iter().any(|t| t == tag) {
                        return true;
                    }
                }
            }
            false
        })
        .count()
        .min(cap as usize) as u8
}

fn sum_tagged_in_facility_rooms(
    instances: Option<&OperatorInstances>,
    blueprint: &BaseBlueprint,
    by_room: &HashMap<String, Vec<AssignedOperator>>,
    kind: FacilityKind,
    tag: &str,
) -> u8 {
    blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == kind)
        .map(|room| {
            let ops = by_room.get(&room.id.0).map(|v| v.as_slice()).unwrap_or(&[]);
            ops.iter()
                .filter(|op| operator_has_tag(instances, op, tag))
                .count() as u8
        })
        .sum()
}

fn count_facility_rooms_tagged_gte(
    instances: Option<&OperatorInstances>,
    blueprint: &BaseBlueprint,
    by_room: &HashMap<String, Vec<AssignedOperator>>,
    kind: FacilityKind,
    tag: &str,
    min: u8,
) -> u8 {
    blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == kind)
        .filter(|room| {
            let ops = by_room.get(&room.id.0).map(|v| v.as_slice()).unwrap_or(&[]);
            let n = ops
                .iter()
                .filter(|op| operator_has_tag(instances, op, tag))
                .count() as u8;
            n >= min
        })
        .count()
        .min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assignment::BaseAssignment;
    use crate::layout::LayoutContext;

    #[test]
    fn elite_facility_count_from_assignment() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("电弧", 2)]);
        assignment.set_power_operator("power_1", AssignedOperator::new("Lancet-2", 0));
        assignment.set_room(
            "trade_1",
            vec![
                AssignedOperator::new("能天使", 2),
                AssignedOperator::new("德克萨斯", 2),
                AssignedOperator::new("真言", 2),
            ],
        );
        let idx = WorkforceIndex::build(&bp, &assignment, None);
        assert_eq!(idx.elite_facility_count(&bp, None), 2);
    }

    #[test]
    fn trade_workforce_from_assignment() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("trade_1", vec![AssignedOperator::new("古米", 0)]);
        let idx = WorkforceIndex::build(&bp, &assignment, None);
        assert!(idx.trade_workforce.iter().any(|n| n == "古米"));
        let mut layout = LayoutContext::default();
        idx.apply_to_layout(&mut layout);
        assert!(layout.trade_workforce.iter().any(|n| n == "古米"));
    }

    #[test]
    fn zero_mood_platform_stays_in_power_but_does_not_block_other_power() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_power_operator(
            "power_1",
            AssignedOperator::new("Lancet-2", 0).with_work_mood(Some(0)),
        );
        assignment.set_power_operator("power_2", AssignedOperator::new("承曦格雷伊", 2));
        let idx = WorkforceIndex::build(&bp, &assignment, None);
        assert!(idx.power_workforce.iter().any(|name| name == "Lancet-2"));
        assert!(!idx.other_power_has_platform(&RoomId::from("power_2")));

        let mut normal = BaseAssignment::default();
        normal.set_power_operator("power_1", AssignedOperator::new("Lancet-2", 0));
        normal.set_power_operator("power_2", AssignedOperator::new("承曦格雷伊", 2));
        let normal_idx = WorkforceIndex::build(&bp, &normal, None);
        assert!(normal_idx.other_power_has_platform(&RoomId::from("power_2")));
    }

    #[test]
    fn elite_facility_count_ignores_promoted_non_elite_operators() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("阿米娅", 2)]);
        let idx = WorkforceIndex::build(&bp, &assignment, None);
        assert_eq!(idx.elite_facility_count(&bp, None), 0);
    }

    #[test]
    fn durin_in_base_caps_at_four_and_excludes_training_assist() {
        let instances = OperatorInstances::load(
            &crate::instances::default_instances_path().expect("instances"),
        )
        .unwrap();
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = vec![
            "杜林".into(),
            "桃金娘".into(),
            "至简".into(),
            "褐果".into(),
            "杜林".into(),
        ];
        assignment.training_assist = Some(AssignedOperator::new("褐果", 0));
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.durin_in_base, 4);
    }

    #[test]
    fn rhine_life_in_base_counts_tagged_workforce_capped() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = [
            "缪尔赛思",
            "多萝西",
            "娜斯提",
            "淬羽赫默",
            "赫默",
            "白面鸮",
            "星源",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assignment.training_assist = Some(AssignedOperator::new("流明", 2));
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.rhine_life_in_base, 6);
    }

    #[test]
    fn rhine_life_in_base_includes_training_lead_but_excludes_assist() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = ["缪尔赛思", "多萝西", "娜斯提"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assignment.set_room("training", vec![AssignedOperator::new("星源", 2)]);
        assignment.training_assist = Some(AssignedOperator::new("赫默", 2));
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.rhine_life_in_base, 4);
    }

    #[test]
    fn rhine_life_in_base_includes_dormitory_operator() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("dorm_1", vec![AssignedOperator::new("星源", 2)]);
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.rhine_life_in_base, 1);
    }

    #[test]
    fn rhine_life_in_base_ignores_non_rhine_training_assist() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = ["缪尔赛思", "多萝西"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assignment.training_assist = Some(AssignedOperator::new("流明", 2));
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.rhine_life_in_base, 2);
    }

    #[test]
    fn rhine_life_in_base_excludes_meeting_room_operator() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = ["缪尔赛思", "多萝西", "赫默"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assignment.set_room("meeting", vec![AssignedOperator::new("赫默", 2)]);
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.rhine_life_in_base, 2);
    }

    #[test]
    fn rhine_life_in_base_dedups_duplicate_workforce_names() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.base_workforce = ["缪尔赛思", "缪尔赛思", "多萝西"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let idx = WorkforceIndex::build(&bp, &assignment, Some(&instances));
        assert_eq!(idx.rhine_life_in_base, 2);
    }

    #[test]
    fn elite_facility_count_ignores_meeting_room() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("meeting", vec![AssignedOperator::new("真言", 2)]);
        let idx = WorkforceIndex::build(&bp, &assignment, None);
        assert_eq!(idx.elite_facility_count(&bp, None), 0);
    }

    #[test]
    fn platform_detection_and_other_flags() {
        let bp = BaseBlueprint::template_252_auto1().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_power_operator("power_1", AssignedOperator::new("Lancet-2", 0));
        assignment.set_power_operator("power_2", AssignedOperator::new("承曦格雷伊", 2));
        let idx = WorkforceIndex::build(&bp, &assignment, None);
        assert_eq!(idx.platform_count_in_power(), 1);
        assert!(idx.other_power_has_platform(&RoomId::from("power_2")));
        assert!(!idx.other_power_has_platform(&RoomId::from("power_1")));
        assert!(idx.other_platform_in_power(&RoomId::from("power_2"), "承曦格雷伊"));
    }
}
