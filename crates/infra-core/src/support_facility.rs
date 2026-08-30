use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::global_resource::GlobalResourceKey;
use crate::layout::LayoutContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportFacility {
    Office,
    Meeting,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupportRegistry {
    pub version: u32,
    pub entries: Vec<SupportSkillEntry>,
}

static DEFAULT_SUPPORT_REGISTRY: OnceLock<Arc<SupportRegistry>> = OnceLock::new();
static DEFAULT_SUPPORT_REGISTRY_INIT: Mutex<()> = Mutex::new(());

impl SupportRegistry {
    pub fn load_default() -> Result<Self> {
        Self::load_default_shared().map(|registry| (*registry).clone())
    }

    pub fn load_default_shared() -> Result<Arc<Self>> {
        if let Some(registry) = DEFAULT_SUPPORT_REGISTRY.get() {
            crate::profile::record_support_registry_access();
            return Ok(Arc::clone(registry));
        }

        let _init = DEFAULT_SUPPORT_REGISTRY_INIT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(registry) = DEFAULT_SUPPORT_REGISTRY.get() {
            crate::profile::record_support_registry_access();
            return Ok(Arc::clone(registry));
        }

        let started = Instant::now();
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/support_skill_registry.json"
        ));
        let registry: Self = serde_json::from_str(raw)?;
        if registry.version != 1 {
            return Err(Error::msg(format!(
                "unsupported support skill registry version {}",
                registry.version
            )));
        }
        registry.validate()?;
        let registry = Arc::new(registry);
        DEFAULT_SUPPORT_REGISTRY
            .set(Arc::clone(&registry))
            .expect("registry initialization lock prevents duplicate support registry");
        crate::profile::record_support_registry_parse(started.elapsed());
        crate::profile::record_support_registry_access();
        Ok(registry)
    }

    fn validate(&self) -> Result<()> {
        let mut rows = HashSet::new();
        let mut bindings = HashSet::new();
        for entry in &self.entries {
            let expected_facility = if (172..=214).contains(&entry.row_id) {
                SupportFacility::Office
            } else if (324..=390).contains(&entry.row_id) {
                SupportFacility::Meeting
            } else {
                return Err(Error::msg(format!(
                    "support registry row {} is outside the bounded scope",
                    entry.row_id
                )));
            };
            if entry.facility != expected_facility {
                return Err(Error::msg(format!(
                    "support registry row {} has the wrong facility",
                    entry.row_id
                )));
            }
            rows.insert(entry.row_id);
            if !bindings.insert((
                entry.operator.as_str(),
                entry.facility,
                entry.slot,
                entry.min_elite,
                entry.min_level,
            )) {
                return Err(Error::msg(format!(
                    "ambiguous support binding for {} slot {} at e{} level {}",
                    entry.operator, entry.slot, entry.min_elite, entry.min_level
                )));
            }
        }
        if rows.len() != 110
            || !(172..=214).all(|row| rows.contains(&row))
            || !(324..=390).all(|row| rows.contains(&row))
        {
            return Err(Error::msg(
                "support registry does not cover the bounded 110 rows",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupportSkillEntry {
    pub row_id: u16,
    pub facility: SupportFacility,
    pub operator: String,
    pub skill: String,
    pub buff_id: String,
    pub slot: u8,
    pub min_elite: u8,
    pub min_level: u32,
    pub effects: Vec<SupportEffect>,
    pub ignored: Option<String>,
    pub unsupported: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupportEffect {
    SpeedFlat {
        value: f64,
    },
    SpeedPerExtraRecruitSlot {
        value: f64,
    },
    MeetingSpeedPerExtraRecruitSlot {
        value: f64,
    },
    SpeedPerDormLevel {
        value: f64,
    },
    SpeedPerEliteFacility {
        value: f64,
        cap: u8,
    },
    SpeedPerState {
        state: String,
        step: f64,
        value: f64,
    },
    SpeedIfSolo {
        value: f64,
    },
    SpeedIfPartner {
        operator: String,
        value: f64,
    },
    SpeedIfOperatorInControl {
        operator: String,
        value: f64,
    },
    SpeedRampAverage {
        initial: f64,
        per_hour: f64,
        cap: f64,
    },
    MoodDelta {
        value: f64,
    },
    MoodPerExtraRecruitSlot {
        value: f64,
    },
}

#[derive(Debug, Clone)]
pub struct SupportOperator {
    pub name: String,
    pub elite: u8,
    pub rarity: u8,
    pub level: u32,
    pub mood: f64,
}

#[derive(Debug, Clone)]
pub struct SupportRoomInput {
    pub facility: SupportFacility,
    pub operators: Vec<SupportOperator>,
    pub capacity: usize,
    pub extra_recruit_slots: u8,
    pub elapsed_hours: f64,
    pub external_speed_bonus_pct: f64,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SupportContribution {
    pub row_id: u16,
    pub buff_id: String,
    pub operator: String,
    pub skill: String,
    pub kind: String,
    pub value: f64,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SupportNotice {
    pub row_id: u16,
    pub buff_id: String,
    pub operator: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OperatorMoodDelta {
    pub operator: String,
    pub delta_per_hour: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SupportRoomResult {
    pub facility: SupportFacility,
    pub skill_speed_bonus_pct: f64,
    pub external_speed_bonus_pct: f64,
    pub total_speed_bonus_pct: f64,
    pub meeting_speed_inject_pct: f64,
    pub mood: Vec<OperatorMoodDelta>,
    pub contributions: Vec<SupportContribution>,
    pub ignored: Vec<SupportNotice>,
    pub unsupported: Vec<SupportNotice>,
}

pub fn evaluate_support_room(
    input: &SupportRoomInput,
    registry: &SupportRegistry,
) -> Result<SupportRoomResult> {
    if input.operators.is_empty() {
        return Err(Error::msg(
            "support facility evaluation requires at least one operator",
        ));
    }
    if input.operators.len() > input.capacity {
        return Err(Error::msg(format!(
            "{:?} capacity is {}, got {} operators",
            input.facility,
            input.capacity,
            input.operators.len()
        )));
    }
    let mut names = HashSet::new();
    if let Some(duplicate) = input
        .operators
        .iter()
        .map(|operator| operator.name.as_str())
        .find(|name| !names.insert(*name))
    {
        return Err(Error::msg(format!(
            "duplicate operator {duplicate} in support facility"
        )));
    }
    if !input.elapsed_hours.is_finite() || input.elapsed_hours <= 0.0 {
        return Err(Error::msg(
            "elapsed_hours must be finite and greater than zero",
        ));
    }

    let active = active_entries(input, registry);
    let mut skill_speed = if input.facility == SupportFacility::Meeting {
        input
            .operators
            .iter()
            .map(|operator| {
                let mood_bonus = if operator.mood > 0.0 { 5.0 } else { 0.0 };
                let rarity_bonus = match operator.rarity {
                    4 => 2.0,
                    5 => 4.0,
                    6 => 5.0,
                    _ => 0.0,
                };
                let elite_bonus = match operator.elite {
                    1 => 8.0,
                    2 => 16.0,
                    _ => 0.0,
                };
                mood_bonus + rarity_bonus + elite_bonus
            })
            .sum()
    } else {
        0.0
    };
    let mut meeting_inject = 0.0;
    let mut mood = input
        .operators
        .iter()
        .map(|op| (op.name.clone(), 0.0))
        .collect::<HashMap<_, _>>();
    let mut contributions = Vec::new();
    let mut ignored = Vec::new();
    let mut unsupported = Vec::new();

    for entry in active {
        if let Some(reason) = &entry.ignored {
            ignored.push(notice(entry, reason));
        }
        if let Some(reason) = &entry.unsupported {
            unsupported.push(notice(entry, reason));
        }
        for effect in &entry.effects {
            let (kind, value, target, active) = effect_value(effect, input)?;
            match target {
                EffectTarget::Speed => skill_speed += value,
                EffectTarget::MeetingInject => meeting_inject += value,
                EffectTarget::Mood => *mood.entry(entry.operator.clone()).or_default() += value,
            }
            contributions.push(SupportContribution {
                row_id: entry.row_id,
                buff_id: entry.buff_id.clone(),
                operator: entry.operator.clone(),
                skill: entry.skill.clone(),
                kind,
                value,
                active,
            });
        }
    }

    let mut mood = mood
        .into_iter()
        .map(|(operator, delta_per_hour)| OperatorMoodDelta {
            operator,
            delta_per_hour,
        })
        .collect::<Vec<_>>();
    mood.sort_by(|a, b| a.operator.cmp(&b.operator));

    Ok(SupportRoomResult {
        facility: input.facility,
        skill_speed_bonus_pct: skill_speed,
        external_speed_bonus_pct: input.external_speed_bonus_pct,
        total_speed_bonus_pct: skill_speed + input.external_speed_bonus_pct,
        meeting_speed_inject_pct: meeting_inject,
        mood,
        contributions,
        ignored,
        unsupported,
    })
}

fn active_entries<'a>(
    input: &SupportRoomInput,
    registry: &'a SupportRegistry,
) -> Vec<&'a SupportSkillEntry> {
    let mut active = Vec::new();
    for operator in &input.operators {
        let mut by_slot: HashMap<u8, &SupportSkillEntry> = HashMap::new();
        for entry in registry.entries.iter().filter(|entry| {
            entry.facility == input.facility
                && entry.operator == operator.name
                && unlock_met(operator, entry)
        }) {
            let replace = by_slot.get(&entry.slot).is_none_or(|current| {
                (entry.min_elite, entry.min_level) > (current.min_elite, current.min_level)
            });
            if replace {
                by_slot.insert(entry.slot, entry);
            }
        }
        active.extend(by_slot.into_values());
    }
    active.sort_by_key(|entry| (entry.row_id, entry.operator.as_str()));
    active
}

fn unlock_met(operator: &SupportOperator, entry: &SupportSkillEntry) -> bool {
    operator.elite > entry.min_elite
        || (operator.elite == entry.min_elite
            && (operator.level == 0 || operator.level >= entry.min_level))
}

#[derive(Debug, Clone, Copy)]
enum EffectTarget {
    Speed,
    MeetingInject,
    Mood,
}

fn effect_value(
    effect: &SupportEffect,
    input: &SupportRoomInput,
) -> Result<(String, f64, EffectTarget, bool)> {
    let (kind, value, target, active) = match effect {
        SupportEffect::SpeedFlat { value } => ("speed_flat", *value, EffectTarget::Speed, true),
        SupportEffect::SpeedPerExtraRecruitSlot { value } => (
            "speed_per_extra_recruit_slot",
            *value * f64::from(input.extra_recruit_slots),
            EffectTarget::Speed,
            true,
        ),
        SupportEffect::MeetingSpeedPerExtraRecruitSlot { value } => (
            "meeting_speed_per_extra_recruit_slot",
            *value * f64::from(input.extra_recruit_slots),
            EffectTarget::MeetingInject,
            true,
        ),
        SupportEffect::SpeedPerDormLevel { value } => (
            "speed_per_dorm_level",
            *value * f64::from(input.layout.dorm_level_sum),
            EffectTarget::Speed,
            true,
        ),
        SupportEffect::SpeedPerEliteFacility { value, cap } => (
            "speed_per_elite_facility",
            *value * f64::from(input.layout.elite_facility_count.min(*cap)),
            EffectTarget::Speed,
            true,
        ),
        SupportEffect::SpeedPerState { state, step, value } => {
            if *step <= 0.0 {
                return Err(Error::msg(format!(
                    "support state step must be positive: {state}"
                )));
            }
            let key = GlobalResourceKey::parse(state)
                .ok_or_else(|| Error::msg(format!("unknown support state {state}")))?;
            (
                "speed_per_state",
                (input.layout.global.get(key) / step).floor() * value,
                EffectTarget::Speed,
                true,
            )
        }
        SupportEffect::SpeedIfSolo { value } => {
            let active = input.operators.len() == 1;
            (
                "speed_if_solo",
                if active { *value } else { 0.0 },
                EffectTarget::Speed,
                active,
            )
        }
        SupportEffect::SpeedIfPartner { operator, value } => {
            let active = input.operators.iter().any(|op| op.name == *operator);
            (
                "speed_if_partner",
                if active { *value } else { 0.0 },
                EffectTarget::Speed,
                active,
            )
        }
        SupportEffect::SpeedIfOperatorInControl { operator, value } => {
            let active = input
                .layout
                .control_workforce
                .iter()
                .any(|name| name == operator);
            (
                "speed_if_operator_in_control",
                if active { *value } else { 0.0 },
                EffectTarget::Speed,
                active,
            )
        }
        SupportEffect::SpeedRampAverage {
            initial,
            per_hour,
            cap,
        } => (
            "speed_ramp_average",
            ramp_average(*initial, *per_hour, *cap, input.elapsed_hours),
            EffectTarget::Speed,
            true,
        ),
        SupportEffect::MoodDelta { value } => ("mood_delta", *value, EffectTarget::Mood, true),
        SupportEffect::MoodPerExtraRecruitSlot { value } => (
            "mood_per_extra_recruit_slot",
            *value * f64::from(input.extra_recruit_slots),
            EffectTarget::Mood,
            true,
        ),
    };
    Ok((kind.to_string(), value, target, active))
}

fn ramp_average(initial: f64, per_hour: f64, cap: f64, hours: f64) -> f64 {
    if per_hour <= 0.0 || initial >= cap {
        return initial.min(cap);
    }
    let ramp_hours = ((cap - initial) / per_hour).max(0.0);
    if hours <= ramp_hours {
        initial + per_hour * hours / 2.0
    } else {
        let ramp_area = initial * ramp_hours + per_hour * ramp_hours * ramp_hours / 2.0;
        (ramp_area + cap * (hours - ramp_hours)) / hours
    }
}

fn notice(entry: &SupportSkillEntry, reason: &str) -> SupportNotice {
    SupportNotice {
        row_id: entry.row_id,
        buff_id: entry.buff_id.clone(),
        operator: entry.operator.clone(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_resource::GlobalResourceKey;

    fn registry() -> SupportRegistry {
        SupportRegistry::load_default().unwrap()
    }

    fn op(name: &str, elite: u8) -> SupportOperator {
        SupportOperator {
            name: name.into(),
            elite,
            rarity: 0,
            level: 0,
            mood: 0.0,
        }
    }

    fn input(facility: SupportFacility, operators: Vec<SupportOperator>) -> SupportRoomInput {
        SupportRoomInput {
            facility,
            operators,
            capacity: match facility {
                SupportFacility::Office => 1,
                SupportFacility::Meeting => 2,
            },
            extra_recruit_slots: 2,
            elapsed_hours: 12.0,
            external_speed_bonus_pct: 0.0,
            layout: LayoutContext::default(),
        }
    }

    #[test]
    fn meeting_base_efficiency_adds_mood_rarity_and_elite() {
        let registry = registry();
        let result = evaluate_support_room(
            &input(
                SupportFacility::Meeting,
                vec![SupportOperator {
                    name: "基础测试干员".into(),
                    elite: 2,
                    rarity: 6,
                    level: 0,
                    mood: 24.0,
                }],
            ),
            &registry,
        )
        .unwrap();
        assert!((result.skill_speed_bonus_pct - 26.0).abs() < 1e-9);
    }

    #[test]
    fn meeting_base_efficiency_keeps_rarity_and_elite_at_zero_mood() {
        let registry = registry();
        let result = evaluate_support_room(
            &input(
                SupportFacility::Meeting,
                vec![SupportOperator {
                    name: "基础测试干员".into(),
                    elite: 1,
                    rarity: 5,
                    level: 0,
                    mood: 0.0,
                }],
            ),
            &registry,
        )
        .unwrap();
        assert!((result.skill_speed_bonus_pct - 12.0).abs() < 1e-9);
    }

    #[test]
    fn registry_covers_exactly_the_bounded_110_rows() {
        let rows = registry()
            .entries
            .into_iter()
            .map(|entry| entry.row_id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(rows.len(), 110);
        assert!((172..=214).all(|row| rows.contains(&row)));
        assert!((324..=390).all(|row| rows.contains(&row)));
    }

    #[test]
    fn office_upgrade_replaces_the_same_slot() {
        let e0 = evaluate_support_room(
            &input(SupportFacility::Office, vec![op("地灵", 0)]),
            &registry(),
        )
        .unwrap();
        let e1 = evaluate_support_room(
            &input(SupportFacility::Office, vec![op("地灵", 1)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(e0.skill_speed_bonus_pct, 30.0);
        assert_eq!(e1.skill_speed_bonus_pct, 45.0);
        assert_eq!(e1.mood[0].delta_per_hour, 2.0);
    }

    #[test]
    fn office_injects_meeting_speed_from_recruit_slots() {
        let result = evaluate_support_room(
            &input(SupportFacility::Office, vec![op("乌有", 0)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(result.skill_speed_bonus_pct, 35.0);
        assert_eq!(result.meeting_speed_inject_pct, 10.0);
    }

    #[test]
    fn office_skill_reads_operator_in_control_center_condition() {
        let mut with_yin_hui = input(SupportFacility::Office, vec![op("圣聆初雪", 2)]);
        with_yin_hui.layout.control_workforce = vec!["凛御银灰".into()];
        let active = evaluate_support_room(&with_yin_hui, &registry()).unwrap();
        assert_eq!(active.skill_speed_bonus_pct, 45.0);
        assert!(active.unsupported.is_empty());

        let inactive = evaluate_support_room(
            &input(SupportFacility::Office, vec![op("圣聆初雪", 2)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(inactive.skill_speed_bonus_pct, 35.0);
        assert!(inactive.contributions.iter().any(|contribution| {
            contribution.buff_id == "hire_spd_cost&char[001]" && !contribution.active
        }));
    }

    #[test]
    fn meeting_solo_and_partner_conditions_use_actual_room() {
        let solo = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("和弦", 2)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(solo.skill_speed_bonus_pct, 66.0);

        let duo = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("凛视", 2), op("提丰", 2)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(duo.skill_speed_bonus_pct, 67.0);
    }

    #[test]
    fn wish_angelina_only_gains_speed_when_working_alone() {
        let solo = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("予愿安洁莉娜", 2)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(solo.skill_speed_bonus_pct, 66.0);
        assert_eq!(solo.mood[0].delta_per_hour, 2.0);
        assert_eq!(solo.ignored.len(), 1);

        let duo = evaluate_support_room(
            &input(
                SupportFacility::Meeting,
                vec![op("予愿安洁莉娜", 2), op("阿米娅", 2)],
            ),
            &registry(),
        )
        .unwrap();
        assert_eq!(duo.skill_speed_bonus_pct, 32.0);
        assert!(duo.contributions.iter().any(|contribution| {
            contribution.buff_id == "meet_spd&cost_condChar[002]"
                && contribution.kind == "speed_if_solo"
                && !contribution.active
        }));
    }

    #[test]
    fn meeting_reads_global_state_and_averages_ramp() {
        let mut state_input = input(SupportFacility::Meeting, vec![op("双月", 2)]);
        state_input
            .layout
            .global
            .set(GlobalResourceKey::IntelligenceReserve, 3.0);
        let state = evaluate_support_room(&state_input, &registry()).unwrap();
        assert_eq!(state.skill_speed_bonus_pct, 36.0);

        let ramp = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("伊内丝", 2)]),
            &registry(),
        )
        .unwrap();
        assert!((ramp.skill_speed_bonus_pct - 43.9166666667).abs() < 1e-9);
    }

    #[test]
    fn unsupported_fragment_is_visible_without_dropping_safe_part() {
        let result = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("提丰", 2)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(result.skill_speed_bonus_pct, 26.0);
        assert_eq!(result.unsupported.len(), 1);
        assert_eq!(result.unsupported[0].row_id, 344);
    }

    #[test]
    fn duplicate_operator_is_rejected() {
        let err = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("凛视", 2), op("凛视", 2)]),
            &registry(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate operator 凛视"));
    }

    #[test]
    fn operator_without_a_support_skill_still_occupies_the_room() {
        let result = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("阿米娅", 2)]),
            &registry(),
        )
        .unwrap();
        assert_eq!(result.total_speed_bonus_pct, 16.0);
        assert_eq!(result.mood[0].operator, "阿米娅");
    }

    #[test]
    fn unmet_condition_is_exposed_as_inactive() {
        let result = evaluate_support_room(
            &input(SupportFacility::Meeting, vec![op("凛视", 2)]),
            &registry(),
        )
        .unwrap();
        let partner = result
            .contributions
            .iter()
            .find(|contribution| contribution.kind == "speed_if_partner")
            .unwrap();
        assert!(!partner.active);
        assert_eq!(partner.value, 0.0);
    }
}
