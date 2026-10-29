//! 排班模式的候选准入；不修改技能，也不参与技能结算。
use std::collections::{BTreeMap, BTreeSet};

use crate::instances::OperatorInstances;
use crate::layout::{AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, LayoutContext};
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, Selector};

/// 真实房间成员，用于候选准入和 memo 身份。定时轮换不启用这项限制。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StationPresence {
    pub room: String,
    pub kind: FacilityKind,
    pub level: u8,
    pub facility: String,
    pub members: Vec<(String, u8, Vec<String>, Vec<String>)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub support_dependencies: Vec<crate::support_facility::SupportMemberDependency>,
}

pub(crate) fn facility_id(kind: FacilityKind) -> &'static str {
    match kind {
        FacilityKind::TradePost => "trade",
        FacilityKind::Factory => "manufacture",
        FacilityKind::PowerPlant => "power",
        FacilityKind::ControlCenter => "control",
        FacilityKind::Dormitory => "dorm",
        FacilityKind::Office => "office",
        FacilityKind::MeetingRoom => "meeting",
        FacilityKind::TrainingRoom => "training",
        _ => "other",
    }
}

pub(crate) fn presence(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
) -> Vec<StationPresence> {
    blueprint
        .rooms
        .iter()
        .map(|room| StationPresence {
            room: room.id.0.clone(),
            kind: room.kind,
            level: if room.kind == FacilityKind::Dormitory {
                room.dorm_skill_level()
            } else {
                room.level
            },
            facility: facility_id(room.kind).into(),
            support_dependencies: match room.kind {
                FacilityKind::Office | FacilityKind::MeetingRoom => {
                    crate::support_facility::member_dependencies(
                        if room.kind == FacilityKind::Office {
                            crate::support_facility::SupportFacility::Office
                        } else {
                            crate::support_facility::SupportFacility::Meeting
                        },
                        assignment
                            .operators_in(&room.id)
                            .iter()
                            .map(|op| crate::support_facility::SupportOperator {
                                name: op.name.clone(),
                                elite: op.elite,
                                rarity: op.rarity,
                                level: op.level,
                                mood: 24.0,
                            })
                            .collect(),
                    )
                    .expect("validated embedded support registry")
                }
                _ => Vec::new(),
            },
            members: assignment
                .operators_in(&room.id)
                .iter()
                .map(|op| {
                    (
                        op.name.clone(),
                        op.elite,
                        buffs(instances, op, room.kind),
                        instances.tags_for(&op.name, op.tier()),
                    )
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn buffs(
    instances: &OperatorInstances,
    op: &AssignedOperator,
    kind: FacilityKind,
) -> Vec<String> {
    match kind {
        FacilityKind::TradePost => instances.resolve_trade_buff_ids(&op.name, op.tier()),
        FacilityKind::Factory => instances.resolve_manufacture_buff_ids(&op.name, op.tier()),
        FacilityKind::PowerPlant => instances.resolve_power_buff_ids(&op.name, op.tier()),
        FacilityKind::ControlCenter => instances.resolve_control_buff_ids(&op.name, op.tier()),
        _ => match kind {
            FacilityKind::Dormitory => instances.resolve_dorm_buff_ids(&op.name, op.tier()),
            FacilityKind::Office => instances.resolve_office_buff_ids(&op.name, op.tier()),
            FacilityKind::Recycle => instances.resolve_recycle_buff_ids(&op.name, op.tier()),
            _ => Vec::new(),
        },
    }
}

pub(crate) fn assignment_admitted(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> bool {
    let stations = presence(blueprint, assignment, instances);
    let layout = super::resolve::layout_from_blueprint(blueprint);
    admitted(&stations, table, &layout)
        && mixed_station_pairs_admitted(&stations, &assignment.auto_main_presence, table, &layout)
}

/// 将候选成员从真实快照迁入候选站，再检查全部站点。
/// 生产填充只搜索空站或保留全部必需成员；不允许跨站移动已占用成员。
pub(crate) fn candidate_admitted(
    layout: &LayoutContext,
    facility: &str,
    members: Vec<(String, u8, Vec<String>, Vec<String>)>,
    table: &SkillTable,
) -> bool {
    if !layout.auto_rotation {
        return true;
    }
    let mut stations = layout.station_presence.clone();
    let room = layout.candidate_room.clone().or_else(|| {
        let own = stations
            .iter()
            .filter(|station| {
                station.facility == facility
                    && station
                        .members
                        .iter()
                        .any(|member| members.iter().any(|candidate| member.0 == candidate.0))
            })
            .collect::<Vec<_>>();
        if own.len() == 1 {
            Some(own[0].room.clone())
        } else {
            stations
                .iter()
                .find(|station| station.facility == facility && station.members.is_empty())
                .map(|station| station.room.clone())
        }
    });
    let room = room.unwrap_or_else(|| format!("candidate_{facility}"));
    if let Some(station) = stations.iter_mut().find(|station| station.room == room) {
        station.members = members;
        station.support_dependencies.clear();
    } else {
        stations.push(StationPresence {
            room: room.clone(),
            kind: match facility {
                "trade" => FacilityKind::TradePost,
                "manufacture" => FacilityKind::Factory,
                "power" => FacilityKind::PowerPlant,
                "control" => FacilityKind::ControlCenter,
                _ => return false,
            },
            level: 3,
            facility: facility.into(),
            members,
            support_dependencies: Vec::new(),
        });
    }
    admitted(&stations, table, layout)
        && mixed_candidate_admitted(&stations, &layout.auto_main_presence, &room, table, layout)
}

/// 独立换班的候选只与其他房可共存的两态比较；不会合并同房主备成员。
fn mixed_candidate_admitted(
    stations: &[StationPresence],
    main: &[StationPresence],
    room: &str,
    table: &SkillTable,
    layout: &LayoutContext,
) -> bool {
    if main.is_empty() {
        return true;
    }
    let dorms = stations
        .iter()
        .filter(|station| station.facility == "dorm")
        .cloned()
        .collect::<Vec<_>>();
    if !main_with_shared_dorm_admitted(main, &dorms, table, layout) {
        return false;
    }
    let Some(candidate) = stations.iter().find(|station| station.room == room) else {
        return false;
    };
    if candidate.facility == "dorm" {
        return mixed_station_pairs_admitted(stations, main, table, layout);
    }
    for other in main
        .iter()
        .chain(stations)
        .filter(|station| station.room != room && station.facility != "dorm")
    {
        let mut pair = dorms.clone();
        pair.push(candidate.clone());
        pair.push(other.clone());
        if !admitted(&pair, table, layout) {
            return false;
        }
    }
    true
}

fn main_with_shared_dorm_admitted(
    main: &[StationPresence],
    dorms: &[StationPresence],
    table: &SkillTable,
    layout: &LayoutContext,
) -> bool {
    let mut shared = main.to_vec();
    for dorm in dorms {
        if let Some(target) = shared.iter_mut().find(|station| station.room == dorm.room) {
            *target = dorm.clone();
        }
    }
    admitted(&shared, table, layout)
}

fn mixed_station_pairs_admitted(
    stations: &[StationPresence],
    main: &[StationPresence],
    table: &SkillTable,
    layout: &LayoutContext,
) -> bool {
    if main.is_empty() {
        return true;
    }
    let dorms = stations
        .iter()
        .filter(|station| station.facility == "dorm")
        .cloned()
        .collect::<Vec<_>>();
    if !main_with_shared_dorm_admitted(main, &dorms, table, layout) {
        return false;
    }
    let states = main
        .iter()
        .chain(stations)
        .filter(|station| station.facility != "dorm" && !station.members.is_empty())
        .collect::<Vec<_>>();
    for (index, left) in states.iter().enumerate() {
        for right in states
            .iter()
            .skip(index + 1)
            .filter(|right| right.room != left.room)
        {
            let mut pair = dorms.clone();
            pair.push((*left).clone());
            pair.push((*right).clone());
            if !admitted(&pair, table, layout) {
                return false;
            }
        }
    }
    true
}

/// 最终态精确复查所有逐房主备组合；不依赖候选两房见证的充分性。
pub(crate) fn all_station_states_admitted(
    main: &[StationPresence],
    backup: &[StationPresence],
    table: &SkillTable,
    layout: &LayoutContext,
) -> bool {
    fn check(
        index: usize,
        main: &[StationPresence],
        backup: &[StationPresence],
        selected: &mut Vec<StationPresence>,
        table: &SkillTable,
        layout: &LayoutContext,
    ) -> bool {
        if index == main.len() {
            return admitted(selected, table, layout);
        }
        selected.push(main[index].clone());
        if !check(index + 1, main, backup, selected, table, layout) {
            return false;
        }
        selected.pop();
        if main[index].facility != "dorm" {
            let Some(other) = backup
                .iter()
                .find(|station| station.room == main[index].room)
            else {
                return false;
            };
            if other.members != main[index].members
                || other.support_dependencies != main[index].support_dependencies
            {
                selected.push(other.clone());
                if !check(index + 1, main, backup, selected, table, layout) {
                    return false;
                }
                selected.pop();
            }
        }
        true
    }
    check(0, main, backup, &mut Vec::new(), table, layout)
}

fn normalized(key: &str) -> String {
    key.to_ascii_lowercase().replace('_', "")
}

fn resource_reads(atom: &crate::types::EffectAtom) -> Vec<String> {
    crate::response_dependency::read_resources(atom)
        .into_iter()
        .map(|key| normalized(&key))
        .collect()
}

/// 来源身份来自既有解释器的实际写入；零净值不会成为外站提供者。
fn resource_providers(
    stations: &[StationPresence],
    table: &SkillTable,
    base: &LayoutContext,
) -> BTreeMap<String, BTreeSet<(usize, String)>> {
    use crate::global_resource::GlobalResourceKey;
    use crate::types::{AtomScope, Phase};
    let mut layout = base.clone();
    // 准入的成员快照可能是候选或逐房混合态，不能继承原态的成员资源与计数。
    layout.global = base
        .auto_initial_global
        .clone()
        .unwrap_or_else(|| base.global.clone());
    layout.platform_count_in_power = stations
        .iter()
        .filter(|room| room.facility == "power")
        .filter(|room| {
            room.members
                .iter()
                .any(|member| member.3.iter().any(|tag| tag == "cc.g.platform"))
        })
        .count()
        .min(255) as u8;
    layout.base_workforce = stations
        .iter()
        .flat_map(|room| super::workforce::operators_for_base_presence(room.kind, &room.members))
        .map(|member| member.0.clone())
        .collect();
    layout.power_workforce = stations
        .iter()
        .filter(|room| room.facility == "power")
        .flat_map(|room| &room.members)
        .map(|member| member.0.clone())
        .collect();
    layout.elite_facility_count = stations
        .iter()
        .filter(|room| has_facility_stat_tag(room, crate::layout::TAG_ELITE_OPERATOR))
        .count()
        .min(255) as u8;
    let mut contributions: BTreeMap<(String, usize, String), f64> = BTreeMap::new();
    let mut global_atoms = Vec::new();
    let mut active_buffs = std::collections::HashSet::new();
    for (index, station) in stations.iter().enumerate() {
        let has_state_producer = station.members.iter().any(|member| {
            member.2.iter().any(|buff| {
                table.get(buff).is_some_and(|skill| {
                    skill.facility == station.facility
                        && skill
                            .atoms
                            .iter()
                            .any(|atom| matches!(atom.action, Action::StateProduce { .. }))
                })
            })
        });
        if station.facility == "control" && has_state_producer {
            let operators = station
                .members
                .iter()
                .map(|member| crate::control::ControlOperator {
                    name: member.0.clone(),
                    elite: member.1,
                    buff_ids: member.2.clone(),
                    tags: member.3.clone(),
                })
                .collect();
            let (result, trace) = crate::control::solve_control_with_trace(
                &crate::control::ControlRoomInput {
                    operators,
                    mood: 24.0,
                    layout: layout.clone(),
                },
                table,
            );
            layout.global = result.global;
            for event in trace.events {
                if let crate::control::ControlTraceOutcome::StateWrite { key, delta, .. } =
                    event.outcome
                {
                    *contributions
                        .entry((normalized(key.id()), index, event.source_operator))
                        .or_default() += delta;
                }
            }
        }
        if station.facility == "power" && has_state_producer {
            for member in &station.members {
                let mut room_layout = layout.clone();
                room_layout.other_power_has_platform =
                    stations.iter().enumerate().any(|(other, room)| {
                        other != index
                            && room.facility == "power"
                            && room
                                .members
                                .iter()
                                .any(|member| member.3.iter().any(|tag| tag == "cc.g.platform"))
                    });
                if let Ok(result) = crate::power::solve_power(
                    &crate::power::PowerRoomInput {
                        operator: crate::power::PowerOperator {
                            name: member.0.clone(),
                            elite: member.1,
                            buff_ids: member.2.clone(),
                            tags: member.3.clone(),
                            work_mood: None,
                        },
                        mood: 24.0,
                        shift_hours: 36.0,
                        layout: room_layout,
                    },
                    table,
                ) {
                    if result.virtual_power_produced != 0.0 {
                        layout.global.add(
                            GlobalResourceKey::VirtualPower,
                            result.virtual_power_produced,
                        );
                        *contributions
                            .entry((
                                normalized(GlobalResourceKey::VirtualPower.id()),
                                index,
                                member.0.clone(),
                            ))
                            .or_default() += result.virtual_power_produced;
                    }
                }
            }
        }
        for member in &station.members {
            for buff in &member.2 {
                let Some(skill) = table
                    .get(buff)
                    .filter(|skill| skill.facility == station.facility)
                else {
                    continue;
                };
                for atom in &skill.atoms {
                    if atom.scope != AtomScope::Global || atom.phase != Phase::StateWrite {
                        continue;
                    }
                    if matches!(station.facility.as_str(), "dorm" | "office")
                        && member.1 < 2
                        && !matches!(&atom.action, Action::StateProduce {key, ..} if normalized(key) == "silentecho")
                    {
                        continue;
                    }
                    active_buffs.insert((member.0.clone(), buff.clone()));
                    global_atoms.push(crate::cross_facility::GlobalAtomEntry {
                        atom: atom.clone(),
                        buff_id: buff.clone(),
                        owner_name: member.0.clone(),
                        owner_elite: member.1,
                        owner_tags: member.3.clone(),
                        room_kind: station.kind,
                        room_level: station.level,
                        room_id: crate::layout::RoomId(station.room.clone()),
                    });
                }
            }
        }
    }
    global_atoms.sort_by_key(|entry| (entry.atom.phase.sort_key(), entry.atom.phase_order));
    let (snapshot, writes) = crate::cross_facility::orchestrate_global_atoms_with_writes(
        &global_atoms,
        &layout,
        layout.global.clone(),
    );
    let mut convert_sources = Vec::new();
    for (atom_index, key, delta, from) in writes {
        let entry = &global_atoms[atom_index];
        let Some(index) = stations
            .iter()
            .position(|station| station.room == entry.room_id.0)
        else {
            continue;
        };
        *contributions
            .entry((normalized(key.id()), index, entry.owner_name.clone()))
            .or_default() += delta;
        if let Some(from) = from {
            convert_sources.push((normalized(from.id()), normalized(key.id())));
        }
    }
    let mut providers: BTreeMap<String, BTreeSet<(usize, String)>> = BTreeMap::new();
    for ((key, index, name), delta) in contributions {
        if delta != 0.0 {
            providers.entry(key).or_default().insert((index, name));
        }
    }
    for (from, to) in convert_sources {
        let source = providers.get(&from).cloned().unwrap_or_default();
        providers.entry(to).or_default().extend(source);
    }
    let mut global = snapshot.global;
    for conversion in global.run_conversions_with_trace(&active_buffs) {
        // 预置资源没有成员来源，但实际启用转换仍依赖在场的provider/converter。
        let conversion_members = stations
            .iter()
            .enumerate()
            .flat_map(|(index, station)| {
                station
                    .members
                    .iter()
                    .filter(move |member| {
                        member.2.iter().any(|buff| {
                            buff == conversion.provider_buff_id
                                || buff == conversion.converter_buff_id
                        })
                    })
                    .map(move |member| (index, member.0.clone()))
            })
            .collect::<Vec<_>>();
        providers
            .entry(normalized(conversion.to.id()))
            .or_default()
            .extend(conversion_members);
        let source = providers
            .get(&normalized(conversion.from.id()))
            .cloned()
            .unwrap_or_default();
        providers
            .entry(normalized(conversion.to.id()))
            .or_default()
            .extend(source);
    }
    providers
}

fn admitted(stations: &[StationPresence], table: &SkillTable, layout: &LayoutContext) -> bool {
    if stations
        .iter()
        .flat_map(|station| &station.members)
        .any(|member| member.0 == "菲亚梅塔")
    {
        return false;
    }
    let providers = std::cell::OnceCell::new();
    for (index, station) in stations.iter().enumerate() {
        use crate::support_facility::SupportMemberDependency;
        for dependency in &station.support_dependencies {
            let forbidden = match dependency {
                SupportMemberDependency::ControlOperator { partner, .. } => {
                    stations.iter().enumerate().any(|(other, room)| {
                        other != index
                            && room.facility == "control"
                            && room.members.iter().any(|member| member.0 == *partner)
                    })
                }
                SupportMemberDependency::EliteFacilities { .. } => {
                    stations.iter().enumerate().any(|(other, room)| {
                        other != index
                            && has_facility_stat_tag(room, crate::layout::TAG_ELITE_OPERATOR)
                    })
                }
                SupportMemberDependency::Resource { key, .. } => providers
                    .get_or_init(|| resource_providers(stations, table, layout))
                    .get(&normalized(key))
                    .is_some_and(|sources| sources.iter().any(|(other, _)| *other != index)),
            };
            if forbidden {
                return false;
            }
        }
    }
    let mut dorm_receivers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (index, station) in stations.iter().enumerate() {
        if station.facility == "dorm" {
            continue;
        }
        for (name, _, buffs, _) in &station.members {
            for buff in buffs {
                let Some(skill) = table
                    .get(buff)
                    .filter(|skill| skill.facility == station.facility)
                else {
                    continue;
                };
                if buff == "control_mp_aegir2[010]"
                    && stations.iter().enumerate().any(|(other, room)| {
                        other != index
                            && room.facility != "dorm"
                            && room
                                .members
                                .iter()
                                .any(|member| member.3.iter().any(|tag| tag == "cc.g.abyssal"))
                    })
                {
                    return false;
                }
                if crate::trade::gold_flow::reads_virtual_lines(&[buff.clone()]) {
                    if stations.iter().enumerate().any(|(other, room)| {
                        other != index
                            && room.facility == "trade"
                            && room.members.iter().any(|member| {
                                crate::trade::gold_flow::virtual_line_yield(&member.2, 4, 4) > 0
                            })
                    }) {
                        return false;
                    }
                }
                if buff == "trade_ord_line_durin[010]"
                    && stations.iter().enumerate().any(|(other, room)| {
                        other != index
                            && super::workforce::operators_for_base_presence(
                                room.kind,
                                &room.members,
                            )
                            .iter()
                            .any(|member| {
                                member.3.iter().any(|tag| tag == crate::layout::TAG_DURIN)
                            })
                    })
                {
                    return false;
                }
                for atom in &skill.atoms {
                    let external_name = match &atom.condition {
                        Some(Condition::OperatorInBase { name }) => Some((name, None)),
                        Some(Condition::OperatorInTrade { name }) => Some((name, Some("trade"))),
                        Some(Condition::OperatorInPower { name }) => Some((name, Some("power"))),
                        Some(Condition::OperatorInTraining { name }) => {
                            Some((name, Some("training")))
                        }
                        _ => None,
                    };
                    if let Some((partner, facility)) = external_name {
                        let same_room = station.members.iter().any(|member| member.0 == *partner);
                        if !same_room {
                            for (other_index, other) in stations.iter().enumerate() {
                                let other_members = if facility.is_none() {
                                    super::workforce::operators_for_base_presence(
                                        other.kind,
                                        &other.members,
                                    )
                                } else {
                                    &other.members
                                };
                                if other_index == index
                                    || !facility.is_none_or(|facility| other.facility == facility)
                                    || !other_members.iter().any(|member| member.0 == *partner)
                                {
                                    continue;
                                }
                                if station.facility == "manufacture" && facility == Some("power") {
                                    continue;
                                }
                                if other.facility == "dorm"
                                    && matches!(station.facility.as_str(), "trade" | "manufacture")
                                {
                                    dorm_receivers
                                        .entry(partner.clone())
                                        .or_default()
                                        .insert(name.clone());
                                } else {
                                    return false;
                                }
                            }
                        }
                    }
                    if external_selector_active(atom.selector.as_ref(), index, stations) {
                        return false;
                    }
                    // 条件读取办公室成员速度，会阻止后续独立办公室补位；在中枢入口拒绝。
                    if matches!(atom.condition, Some(Condition::OfficeHireSpeedBelow { .. })) {
                        return false;
                    }
                    let platform_rooms = stations
                        .iter()
                        .enumerate()
                        .filter(|(other, room)| {
                            *other != index
                                && room.facility == "power"
                                && room
                                    .members
                                    .iter()
                                    .any(|member| member.3.iter().any(|tag| tag == "cc.g.platform"))
                        })
                        .count();
                    if match &atom.condition {
                        Some(Condition::PlatformCountGte { min }) => {
                            platform_rooms >= usize::from(*min)
                        }
                        Some(Condition::OtherPlatformInPower {}) => platform_rooms > 0,
                        _ => false,
                    } {
                        return false;
                    }
                    if matches!(atom.condition, Some(Condition::OtherLateranoInPower {}))
                        && stations.iter().enumerate().any(|(other, room)| {
                            other != index
                                && room.facility == "power"
                                && room
                                    .members
                                    .iter()
                                    .any(|member| member.3.iter().any(|tag| tag == "cc.g.laterano"))
                        })
                    {
                        return false;
                    }
                    if matches!(
                        atom.selector,
                        Some(Selector::RhineLifeInBase | Selector::RhineLifeInBaseExcludingSelf)
                    ) {
                        let external: Vec<_> = stations
                            .iter()
                            .enumerate()
                            .filter(|(other, _)| *other != index)
                            .flat_map(|(_, other)| {
                                super::workforce::operators_for_base_presence(
                                    other.kind,
                                    &other.members,
                                )
                                .iter()
                                .filter(|member| {
                                    member.3.iter().any(|tag| tag == crate::layout::TAG_RHINE)
                                })
                                .map(move |member| (other, member))
                            })
                            .collect();
                        if external.len() > 1
                            || external.iter().any(|(other, _)| other.facility != "dorm")
                        {
                            return false;
                        }
                        for (_, member) in external {
                            dorm_receivers
                                .entry(member.0.clone())
                                .or_default()
                                .insert(name.clone());
                        }
                    }
                    for resource in resource_reads(atom) {
                        for (other_index, provider) in providers
                            .get_or_init(|| resource_providers(stations, table, layout))
                            .get(&resource)
                            .into_iter()
                            .flatten()
                        {
                            if *other_index == index {
                                continue;
                            }
                            let other = &stations[*other_index];
                            if station.facility == "manufacture"
                                && other.facility == "power"
                                && resource == "virtualpower"
                            {
                                continue;
                            }
                            if other.facility != "dorm"
                                || !matches!(
                                    station.facility.as_str(),
                                    "trade" | "manufacture" | "power"
                                )
                            {
                                return false;
                            }
                            dorm_receivers
                                .entry(provider.clone())
                                .or_default()
                                .insert(name.clone());
                        }
                    }
                }
            }
        }
    }
    let mut receiver_supports: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (support, receivers) in &dorm_receivers {
        if receivers.len() > 1 {
            return false;
        }
        for receiver in receivers {
            receiver_supports
                .entry(receiver)
                .or_default()
                .insert(support);
        }
    }
    receiver_supports
        .values()
        .all(|supports| supports.len() <= 1)
}

fn external_selector_active(
    selector: Option<&Selector>,
    own: usize,
    stations: &[StationPresence],
) -> bool {
    let external_tag = |facility: Option<&str>, tag: &str| {
        stations.iter().enumerate().any(|(index, room)| {
            index != own
                && facility.is_none_or(|facility| room.facility == facility)
                && room
                    .members
                    .iter()
                    .any(|member| member.3.iter().any(|value| value == tag))
        })
    };
    match selector {
        Some(Selector::TaggedCountInControl { tag }) => external_tag(Some("control"), tag),
        Some(Selector::TaggedCountInTradeSum { tag }) => external_tag(Some("trade"), tag),
        Some(Selector::TaggedCountInCurrentTradeRoom { tag })
            if stations[own].facility == "control" =>
        {
            external_tag(Some("trade"), tag)
        }
        Some(Selector::TradeStationsWithTaggedGte { tag, min }) => {
            stations.iter().enumerate().any(|(index, room)| {
                index != own
                    && room.facility == "trade"
                    && room
                        .members
                        .iter()
                        .filter(|member| member.3.iter().any(|value| value == tag))
                        .count()
                        >= usize::from(*min)
            })
        }
        Some(Selector::TaggedCountInManuSum { tag }) => external_tag(Some("manufacture"), tag),
        Some(Selector::PlatformCountInPower) => external_tag(Some("power"), "cc.g.platform"),
        Some(Selector::EliteFacilityCount) => stations.iter().enumerate().any(|(index, room)| {
            index != own && has_facility_stat_tag(room, crate::layout::TAG_ELITE_OPERATOR)
        }),
        Some(Selector::SuiFacilityCount | Selector::CappedSuiFacilityCount { .. }) => {
            external_tag(None, "cc.g.sui")
        }
        _ => false,
    }
}

fn has_facility_stat_tag(station: &StationPresence, tag: &str) -> bool {
    super::workforce::operators_for_facility_stat(station.kind, &station.members)
        .iter()
        .any(|member| member.3.iter().any(|value| value == tag))
}
