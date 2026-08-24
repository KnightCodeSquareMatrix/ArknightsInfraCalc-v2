use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlEffectDomain {
    Trade,
    Manufacture,
    Office,
    Meeting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlEffectUnit {
    OrderEfficiencyPct,
    ProductivityPct,
    HireSpeedPct,
    ClueSpeedPct,
}

/// Structured control-center effect region. The first vertical slice covers
/// support-facility injections; trade and manufacture migrate next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub struct ControlEffectKey {
    pub domain: ControlEffectDomain,
    pub unit: ControlEffectUnit,
}

impl ControlEffectKey {
    pub const TRADE_GLOBAL_EFFICIENCY: Self = Self {
        domain: ControlEffectDomain::Trade,
        unit: ControlEffectUnit::OrderEfficiencyPct,
    };
    pub const MANUFACTURE_GLOBAL_PRODUCTIVITY: Self = Self {
        domain: ControlEffectDomain::Manufacture,
        unit: ControlEffectUnit::ProductivityPct,
    };
    pub const OFFICE_HIRE_SPEED: Self = Self {
        domain: ControlEffectDomain::Office,
        unit: ControlEffectUnit::HireSpeedPct,
    };
    pub const MEETING_CLUE_SPEED: Self = Self {
        domain: ControlEffectDomain::Meeting,
        unit: ControlEffectUnit::ClueSpeedPct,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlEffectStatus {
    Winner,
    TiedWinner,
    Suppressed,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ControlEffectRecord {
    pub source_operator: String,
    pub source_buff_id: String,
    pub atom_id: String,
    pub key: ControlEffectKey,
    pub candidate_value: f64,
    pub effective_value: f64,
    pub status: ControlEffectStatus,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ControlEffectLedger {
    candidates: Vec<ControlEffectRecord>,
    values: HashMap<ControlEffectKey, f64>,
}

impl ControlEffectLedger {
    pub fn record_max(
        &mut self,
        source_operator: &str,
        source_buff_id: &str,
        atom_id: &str,
        key: ControlEffectKey,
        value: f64,
    ) {
        assert!(
            !atom_id.trim().is_empty(),
            "control ledger requires a non-empty stable atom id"
        );
        self.candidates.push(ControlEffectRecord {
            source_operator: source_operator.to_string(),
            source_buff_id: source_buff_id.to_string(),
            atom_id: atom_id.to_string(),
            key,
            candidate_value: value,
            effective_value: 0.0,
            status: ControlEffectStatus::Suppressed,
        });
    }

    pub fn freeze(&mut self) {
        self.values.clear();
        for record in &self.candidates {
            self.values
                .entry(record.key)
                .and_modify(|value| *value = value.max(record.candidate_value))
                .or_insert(record.candidate_value);
        }
        let equal_counts: Vec<_> = self
            .candidates
            .iter()
            .map(|record| {
                self.candidates
                    .iter()
                    .filter(|other| {
                        other.key == record.key
                            && (other.candidate_value - record.candidate_value).abs() < f64::EPSILON
                    })
                    .count()
            })
            .collect();
        for (record, equal_count) in self.candidates.iter_mut().zip(equal_counts) {
            record.effective_value = self.values[&record.key];
            record.status =
                if (record.candidate_value - record.effective_value).abs() >= f64::EPSILON {
                    ControlEffectStatus::Suppressed
                } else if equal_count > 1 {
                    ControlEffectStatus::TiedWinner
                } else {
                    ControlEffectStatus::Winner
                };
        }
    }

    pub fn value(&self, key: ControlEffectKey) -> f64 {
        self.values.get(&key).copied().unwrap_or(0.0)
    }

    pub fn records(&self) -> &[ControlEffectRecord] {
        &self.candidates
    }

    /// Number of effective sources beyond the first source in the four ledger
    /// regions. Equal max sources are redundant for candidate admission even
    /// when they are tied winners.
    pub fn redundant_effect_source_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|record| {
                matches!(
                    record.status,
                    ControlEffectStatus::Winner | ControlEffectStatus::TiedWinner
                )
            })
            .count()
            .saturating_sub(self.values.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "control ledger requires a non-empty stable atom id")]
    fn empty_atom_id_is_rejected() {
        let mut ledger = ControlEffectLedger::default();
        ledger.record_max(
            "测试",
            "control_test",
            " ",
            ControlEffectKey::TRADE_GLOBAL_EFFICIENCY,
            1.0,
        );
    }

    #[test]
    fn meeting_max_marks_winners_and_suppressed_sources() {
        let mut ledger = ControlEffectLedger::default();
        ledger.record_max(
            "老鲤",
            "old_li",
            "meeting_old_li",
            ControlEffectKey::MEETING_CLUE_SPEED,
            25.0,
        );
        ledger.record_max(
            "魔王",
            "civilight",
            "meeting_civilight",
            ControlEffectKey::MEETING_CLUE_SPEED,
            15.0,
        );
        ledger.record_max(
            "老鲤替身",
            "old_li_tie",
            "meeting_old_li_tie",
            ControlEffectKey::MEETING_CLUE_SPEED,
            25.0,
        );
        ledger.freeze();

        assert_eq!(ledger.value(ControlEffectKey::MEETING_CLUE_SPEED), 25.0);
        assert_eq!(ledger.records()[0].status, ControlEffectStatus::TiedWinner);
        assert_eq!(ledger.records()[1].status, ControlEffectStatus::Suppressed);
        assert_eq!(ledger.records()[2].status, ControlEffectStatus::TiedWinner);
    }

    #[test]
    fn global_trade_and_manufacture_are_independent_max_regions() {
        let mut ledger = ControlEffectLedger::default();
        ledger.record_max(
            "阿米娅",
            "control_tra_spd[000]",
            "trade_amiya",
            ControlEffectKey::TRADE_GLOBAL_EFFICIENCY,
            7.0,
        );
        ledger.record_max(
            "斩业星熊",
            "control_token_prod_spd3[000]",
            "manu_star_bear",
            ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY,
            3.0,
        );
        ledger.record_max(
            "凯尔希",
            "control_prod_spd[000]",
            "manu_kaltsit",
            ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY,
            2.0,
        );
        ledger.freeze();

        assert_eq!(ledger.value(ControlEffectKey::TRADE_GLOBAL_EFFICIENCY), 7.0);
        assert_eq!(
            ledger.value(ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY),
            3.0
        );
        assert_eq!(ledger.records()[2].status, ControlEffectStatus::Suppressed);
    }

    #[test]
    fn tied_max_sources_count_as_redundant_candidate_owners() {
        let mut ledger = ControlEffectLedger::default();
        ledger.record_max(
            "阿米娅",
            "control_tra_spd[000]",
            "amiya",
            ControlEffectKey::TRADE_GLOBAL_EFFICIENCY,
            7.0,
        );
        ledger.record_max(
            "阿斯卡纶",
            "control_tra_spd[030]",
            "ascalon",
            ControlEffectKey::TRADE_GLOBAL_EFFICIENCY,
            7.0,
        );
        ledger.freeze();

        assert_eq!(ledger.redundant_effect_source_count(), 1);
    }
}
