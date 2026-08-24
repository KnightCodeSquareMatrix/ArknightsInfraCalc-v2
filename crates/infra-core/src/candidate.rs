use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::Efficiency;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    SystemBaked,
    GenericBaked,
    Shortcut,
    DynamicSearch,
    Fallback,
    ManualRule,
    ManualSystemCandidate,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStationKind {
    Manufacture,
    Trade,
    Power,
    Control,
    Dormitory,
    Office,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamMetric {
    pub key: String,
    pub value: f64,
}

impl TeamMetric {
    fn new(key: impl Into<String>, value: f64) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamCandidate {
    pub station_kind: CandidateStationKind,
    pub room_id: Option<String>,
    pub recipe: Option<String>,
    pub order_kind: Option<String>,
    pub operators: Vec<String>,

    pub source: CandidateSource,
    pub source_id: Option<String>,
    pub system_tags: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_efficiency: Option<Efficiency>,
    pub metrics: Vec<TeamMetric>,

    pub selected: Option<bool>,
    pub rejected: Option<bool>,
    pub rejection_reason: Option<String>,

    pub metadata: BTreeMap<String, Value>,
}

impl TeamCandidate {
    pub fn from_manu_search_hit(
        hit: &crate::search::ManuSearchHit,
        recipe: Option<crate::types::RecipeKind>,
        room_id: Option<String>,
        source: CandidateSource,
    ) -> Self {
        let recipe = recipe
            .map(|recipe| format!("{recipe:?}"))
            .or_else(|| non_empty_string(&hit.breakdown.recipe));
        let mut metrics = vec![TeamMetric::new(
            "manufacture.final_efficiency",
            hit.final_efficiency.as_f64(),
        )];
        push_non_zero_metric(
            &mut metrics,
            "manufacture.gold.final_efficiency",
            hit.per_station.gold.as_f64(),
        );
        push_non_zero_metric(
            &mut metrics,
            "manufacture.battle_record.final_efficiency",
            hit.per_station.battle_record.as_f64(),
        );
        push_non_zero_metric(
            &mut metrics,
            "manufacture.originium.final_efficiency",
            hit.per_station.originium.as_f64(),
        );

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "base_efficiency".to_string(),
            json!(hit.breakdown.base_efficiency),
        );
        metadata.insert(
            "occupancy_efficiency".to_string(),
            json!(hit.breakdown.occupancy_efficiency),
        );
        metadata.insert(
            "skill_efficiency".to_string(),
            json!(hit.breakdown.skill_efficiency),
        );
        metadata.insert(
            "global_efficiency".to_string(),
            json!(hit.breakdown.global_efficiency),
        );
        metadata.insert(
            "final_efficiency".to_string(),
            json!(hit.breakdown.final_efficiency),
        );
        metadata.insert(
            "storage_limit".to_string(),
            json!(hit.breakdown.storage_limit),
        );
        metadata.insert("storage".to_string(), json!(hit.storage));

        Self {
            station_kind: CandidateStationKind::Manufacture,
            room_id,
            recipe,
            order_kind: None,
            operators: first_non_empty(&[&hit.names, &hit.gold_names, &hit.battle_record_names]),
            source,
            source_id: None,
            system_tags: Vec::new(),
            final_efficiency: Some(hit.final_efficiency),
            metrics,
            selected: None,
            rejected: None,
            rejection_reason: None,
            metadata,
        }
    }

    pub fn from_trade_search_hit(
        hit: &crate::search::TradeSearchHit,
        order_kind: Option<crate::trade::input::TradeOrderKind>,
        room_id: Option<String>,
        source: Option<CandidateSource>,
    ) -> Self {
        let source = source.unwrap_or_else(|| {
            if hit.rule_id.is_some() {
                CandidateSource::Shortcut
            } else {
                CandidateSource::DynamicSearch
            }
        });
        let mut metrics = vec![
            TeamMetric::new("trade.final_efficiency", hit.final_efficiency.as_f64()),
            TeamMetric::new(
                "trade.mechanic_equivalent_efficiency",
                hit.mechanic_equivalent_efficiency.as_f64(),
            ),
            TeamMetric::new("trade.unit_trade_per_day", hit.unit_trade_per_day),
            TeamMetric::new("trade.unit_gold_per_day", hit.unit_gold_per_day),
        ];
        push_non_zero_metric(
            &mut metrics,
            "trade.unit_originium_per_day",
            hit.unit_originium_per_day,
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("rule_id".to_string(), json!(hit.rule_id));
        metadata.insert("breakdown".to_string(), json!(hit.breakdown));

        Self {
            station_kind: CandidateStationKind::Trade,
            room_id,
            recipe: None,
            order_kind: order_kind.map(|order| format!("{order:?}")),
            operators: first_non_empty(&[&hit.names, &hit.gold_names, &hit.originium_names]),
            source,
            source_id: hit.rule_id.clone(),
            system_tags: Vec::new(),
            final_efficiency: Some(hit.final_efficiency),
            metrics,
            selected: None,
            rejected: None,
            rejection_reason: None,
            metadata,
        }
    }
}

fn first_non_empty(lists: &[&Vec<String>]) -> Vec<String> {
    lists
        .iter()
        .find(|names| !names.is_empty())
        .map(|names| (*names).clone())
        .unwrap_or_default()
}

fn non_empty_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn push_non_zero_metric(metrics: &mut Vec<TeamMetric>, key: &'static str, value: f64) {
    if value != 0.0 {
        metrics.push(TeamMetric::new(key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manufacture::{ManuProdBreakdown, ManuStorageBreakdown};
    use crate::search::{
        ManuEfficiencyBreakdown, ManuSearchHit, TradeEfficiencyBreakdown, TradeSearchHit,
    };
    use crate::Efficiency;

    fn test_manu_hit() -> ManuSearchHit {
        ManuSearchHit {
            names: vec!["清流".to_string(), "温蒂".to_string(), "冬时".to_string()],
            gold_names: vec![],
            battle_record_names: vec![],
            final_efficiency: Efficiency::from_decimal(2.300),
            per_station: ManuProdBreakdown {
                gold: Efficiency::from_decimal(2.300),
                ..Default::default()
            },
            storage: ManuStorageBreakdown {
                gold: 20,
                ..Default::default()
            },
            breakdown: ManuEfficiencyBreakdown {
                base_efficiency: Efficiency::from_decimal(1.030),
                occupancy_efficiency: Efficiency::from_decimal(0.030),
                skill_efficiency: Efficiency::from_decimal(1.270),
                global_efficiency: Efficiency::ZERO,
                final_efficiency: Efficiency::from_decimal(2.300),
                storage_limit: 20,
                recipe: "gold".to_string(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn manu_search_hit_candidate_preserves_final_efficiency() {
        let hit = test_manu_hit();
        let candidate = TeamCandidate::from_manu_search_hit(
            &hit,
            None,
            Some("manu_1".to_string()),
            CandidateSource::DynamicSearch,
        );

        assert_eq!(candidate.station_kind, CandidateStationKind::Manufacture);
        assert_eq!(candidate.source, CandidateSource::DynamicSearch);
        assert_eq!(candidate.room_id.as_deref(), Some("manu_1"));
        assert_eq!(candidate.recipe.as_deref(), Some("gold"));
        assert_eq!(candidate.operators, hit.names);
        assert_eq!(candidate.final_efficiency, Some(hit.final_efficiency));
        assert!(candidate
            .metrics
            .iter()
            .any(|metric| metric.key == "manufacture.final_efficiency" && metric.value == 2.300));
        assert!(candidate.selected.is_none());
        assert!(candidate.rejected.is_none());

        let json = serde_json::to_value(&candidate).unwrap();
        assert_eq!(json["final_efficiency"], serde_json::json!(2.300));
        assert!(json.get("metrics").is_some());
        assert!(json.get("score").is_none());
        assert!(json.get("columns").is_none());
        assert!(json.get("raw_score").is_none());
        assert!(json.get("decision_score").is_none());
    }

    #[test]
    fn manu_split_hit_uses_first_non_empty_column_names() {
        let mut hit = test_manu_hit();
        hit.names.clear();
        hit.gold_names = vec!["夜烟".to_string(), "清流".to_string(), "砾".to_string()];

        let candidate =
            TeamCandidate::from_manu_search_hit(&hit, None, None, CandidateSource::DynamicSearch);

        assert_eq!(candidate.operators, hit.gold_names);
    }

    #[test]
    fn trade_shortcut_hit_candidate_uses_shortcut_source() {
        let hit = TradeSearchHit {
            names: vec!["巫恋".to_string(), "龙舌兰".to_string(), "柏喙".to_string()],
            gold_names: vec![],
            originium_names: vec![],
            final_efficiency: Efficiency::from_decimal(1.200),
            mechanic_equivalent_efficiency: Efficiency::from_decimal(0.350),
            rule_id: Some("witch_tequila_bibeak".to_string()),
            unit_trade_per_day: 60000.0,
            unit_gold_per_day: 40.0,
            unit_originium_per_day: 0.0,
            breakdown: Some(TradeEfficiencyBreakdown {
                final_efficiency: Efficiency::from_decimal(1.200),
                mechanic_equivalent_efficiency: Efficiency::from_decimal(0.350),
                rule_id: Some("witch_tequila_bibeak".to_string()),
                ..Default::default()
            }),
        };

        let candidate = TeamCandidate::from_trade_search_hit(&hit, None, None, None);

        assert_eq!(candidate.station_kind, CandidateStationKind::Trade);
        assert_eq!(candidate.source, CandidateSource::Shortcut);
        assert_eq!(candidate.source_id.as_deref(), Some("witch_tequila_bibeak"));
        assert_eq!(candidate.operators, hit.names);
        assert_eq!(candidate.final_efficiency, Some(hit.final_efficiency));
        assert!(candidate
            .metrics
            .iter()
            .any(|metric| metric.key == "trade.final_efficiency" && metric.value == 1.200));
        assert!(candidate.metrics.iter().any(|metric| metric.key
            == "trade.mechanic_equivalent_efficiency"
            && metric.value == 0.350));
    }
}
