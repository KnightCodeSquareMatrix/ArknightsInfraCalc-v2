use std::collections::{HashMap, HashSet};

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
    /// regions. A physical operator counts once per key even when it owns
    /// multiple effective atoms. Equal max sources from different operators
    /// are redundant for candidate admission even when they are tied winners.
    pub fn redundant_effect_source_count(&self) -> usize {
        let mut sources_by_key: HashMap<ControlEffectKey, HashSet<&str>> = HashMap::new();
        for record in &self.candidates {
            if matches!(
                record.status,
                ControlEffectStatus::Winner | ControlEffectStatus::TiedWinner
            ) {
                sources_by_key
                    .entry(record.key)
                    .or_default()
                    .insert(record.source_operator.as_str());
            }
        }
        sources_by_key
            .values()
            .map(|sources| sources.len().saturating_sub(1))
            .sum()
    }

    pub fn has_redundant_effect_sources(&self) -> bool {
        self.redundant_effect_source_count() > 0
    }

    /// A control state may retain only one physical source for each EffectKey.
    /// Lower values are still illegal occupants even when their atom is
    /// suppressed by a higher source.
    pub fn has_conflicting_effect_sources(&self) -> bool {
        self.conflicting_effect_source_count() > 0
    }

    pub fn conflicting_effect_source_count(&self) -> usize {
        let mut sources_by_key: HashMap<ControlEffectKey, HashSet<&str>> = HashMap::new();
        for record in &self.candidates {
            sources_by_key
                .entry(record.key)
                .or_default()
                .insert(record.source_operator.as_str());
        }
        sources_by_key
            .values()
            .map(|sources| sources.len().saturating_sub(1))
            .sum()
    }
}
