use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
pub struct CandidateScore {
    pub raw_score: f64,
    pub decision_score: f64,
}

impl CandidateScore {
    pub fn raw_only(raw_score: f64) -> Self {
        Self {
            raw_score,
            decision_score: raw_score,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamColumn {
    pub key: String,
    pub score: f64,
}

impl TeamColumn {
    fn new(key: impl Into<String>, score: f64) -> Self {
        Self {
            key: key.into(),
            score,
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

    pub score: CandidateScore,
    pub columns: Vec<TeamColumn>,

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
        let mut columns = vec![TeamColumn::new(
            "manufacture.composite_score",
            hit.composite_score,
        )];
        push_non_zero_column(&mut columns, "manufacture.gold.prod_total", hit.per_station.gold);
        push_non_zero_column(
            &mut columns,
            "manufacture.battle_record.prod_total",
            hit.per_station.battle_record,
        );
        push_non_zero_column(
            &mut columns,
            "manufacture.originium.prod_total",
            hit.per_station.originium,
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("prod_base".to_string(), json!(hit.breakdown.prod_base));
        metadata.insert("prod_skill".to_string(), json!(hit.breakdown.prod_skill));
        metadata.insert("prod_global".to_string(), json!(hit.breakdown.prod_global));
        metadata.insert("prod_total".to_string(), json!(hit.breakdown.prod_total));
        metadata.insert("storage_limit".to_string(), json!(hit.breakdown.storage_limit));
        metadata.insert("storage".to_string(), json!(hit.storage));

        Self {
            station_kind: CandidateStationKind::Manufacture,
            room_id,
            recipe,
            order_kind: None,
            operators: first_non_empty(&[
                &hit.names,
                &hit.gold_names,
                &hit.battle_record_names,
            ]),
            source,
            source_id: None,
            system_tags: Vec::new(),
            score: CandidateScore::raw_only(hit.composite_score),
            columns,
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
            if hit.shortcut.is_some() {
                CandidateSource::Shortcut
            } else {
                CandidateSource::DynamicSearch
            }
        });
        let mut columns = vec![
            TeamColumn::new("trade.score", hit.score),
            TeamColumn::new("trade.trade_pct", hit.trade_pct),
            TeamColumn::new("trade.gold_pct", hit.gold_pct),
            TeamColumn::new("trade.unit_trade_per_day", hit.unit_trade_per_day),
            TeamColumn::new("trade.unit_gold_per_day", hit.unit_gold_per_day),
            TeamColumn::new("trade.output_multiplier", hit.output_multiplier),
        ];
        push_non_zero_column(
            &mut columns,
            "trade.unit_originium_per_day",
            hit.unit_originium_per_day,
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("shortcut".to_string(), json!(hit.shortcut));
        metadata.insert("breakdown".to_string(), json!(hit.breakdown));

        Self {
            station_kind: CandidateStationKind::Trade,
            room_id,
            recipe: None,
            order_kind: order_kind.map(|order| format!("{order:?}")),
            operators: first_non_empty(&[&hit.names, &hit.gold_names, &hit.originium_names]),
            source,
            source_id: hit.shortcut.clone(),
            system_tags: Vec::new(),
            score: CandidateScore::raw_only(hit.score),
            columns,
            selected: None,
            rejected: None,
            rejection_reason: None,
            metadata,
        }
    }

    pub fn from_manufacture_system_trace(
        trace: &crate::layout::ManufactureSystemCandidateTrace,
    ) -> Self {
        let raw_score = trace.raw_score.unwrap_or(0.0);
        let mut columns = Vec::new();
        if trace.raw_score.is_some() {
            columns.push(TeamColumn::new("manufacture.raw_score", raw_score));
        }

        let mut metadata = BTreeMap::new();
        metadata.insert("trace_source".to_string(), json!(trace.source));
        metadata.insert("evaluation_failed".to_string(), json!(trace.evaluation_failed));
        metadata.insert("linked_producers".to_string(), json!(trace.linked_producers));
        metadata.insert("evidence".to_string(), json!(trace.evidence));

        Self {
            station_kind: CandidateStationKind::Manufacture,
            room_id: Some(trace.room.clone()),
            recipe: Some(trace.recipe.clone()),
            order_kind: None,
            operators: trace.operators.clone(),
            source: candidate_source_from_trace(&trace.source),
            source_id: Some(trace.source_system.clone()),
            system_tags: vec![trace.source_system.clone()],
            score: CandidateScore::raw_only(raw_score),
            columns,
            selected: Some(trace.selected),
            rejected: Some(trace.rejected),
            rejection_reason: trace.rejection_reason.clone(),
            metadata,
        }
    }
}

fn candidate_source_from_trace(source: &str) -> CandidateSource {
    match source {
        "system-baked" => CandidateSource::SystemBaked,
        "manual-system-candidate" => CandidateSource::ManualSystemCandidate,
        "manual-rule" => CandidateSource::ManualRule,
        _ => CandidateSource::Unknown,
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

fn push_non_zero_column(columns: &mut Vec<TeamColumn>, key: &'static str, score: f64) {
    if score != 0.0 {
        columns.push(TeamColumn::new(key, score));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ManufactureLinkedProducer, ManufactureSystemCandidateTrace};
    use crate::manufacture::{ManuProdBreakdown, ManuStorageBreakdown};
    use crate::search::{ManuScoreBreakdown, ManuSearchHit, TradeScoreBreakdown, TradeSearchHit};

    fn test_manu_hit() -> ManuSearchHit {
        ManuSearchHit {
            names: vec!["清流".to_string(), "温蒂".to_string(), "冬时".to_string()],
            gold_names: vec![],
            battle_record_names: vec![],
            composite_score: 130.0,
            per_station: ManuProdBreakdown {
                gold: 130.0,
                ..Default::default()
            },
            storage: ManuStorageBreakdown {
                gold: 20,
                ..Default::default()
            },
            breakdown: ManuScoreBreakdown {
                prod_base: 3.0,
                prod_skill: 127.0,
                prod_global: 0.0,
                prod_total: 130.0,
                storage_limit: 20,
                recipe: "gold".to_string(),
            },
        }
    }

    #[test]
    fn manu_search_hit_candidate_preserves_raw_score() {
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
        assert_eq!(candidate.score.raw_score, 130.0);
        assert_eq!(candidate.score.decision_score, 130.0);
        assert!(candidate
            .columns
            .iter()
            .any(|column| column.key == "manufacture.composite_score" && column.score == 130.0));
        assert!(candidate.selected.is_none());
        assert!(candidate.rejected.is_none());
    }

    #[test]
    fn manu_split_hit_uses_first_non_empty_column_names() {
        let mut hit = test_manu_hit();
        hit.names.clear();
        hit.gold_names = vec!["夜烟".to_string(), "清流".to_string(), "砾".to_string()];

        let candidate = TeamCandidate::from_manu_search_hit(
            &hit,
            None,
            None,
            CandidateSource::DynamicSearch,
        );

        assert_eq!(candidate.operators, hit.gold_names);
    }

    #[test]
    fn trade_shortcut_hit_candidate_uses_shortcut_source() {
        let hit = TradeSearchHit {
            names: vec!["巫恋".to_string(), "龙舌兰".to_string(), "柏喙".to_string()],
            gold_names: vec![],
            originium_names: vec![],
            score: 120.0,
            trade_pct: 120.0,
            gold_pct: 35.0,
            shortcut: Some("witch_tequila_bibeak".to_string()),
            unit_trade_per_day: 60000.0,
            unit_gold_per_day: 40.0,
            unit_originium_per_day: 0.0,
            output_multiplier: 2.2,
            breakdown: TradeScoreBreakdown {
                order_eff_total_pct: 120.0,
                mechanic_equiv_eff_pct: 35.0,
                shortcut_id: Some("witch_tequila_bibeak".to_string()),
                ..Default::default()
            },
        };

        let candidate = TeamCandidate::from_trade_search_hit(&hit, None, None, None);

        assert_eq!(candidate.station_kind, CandidateStationKind::Trade);
        assert_eq!(candidate.source, CandidateSource::Shortcut);
        assert_eq!(candidate.source_id.as_deref(), Some("witch_tequila_bibeak"));
        assert_eq!(candidate.operators, hit.names);
        assert_eq!(candidate.score.raw_score, hit.score);
        assert_eq!(candidate.score.decision_score, hit.score);
        assert!(candidate
            .columns
            .iter()
            .any(|column| column.key == "trade.trade_pct" && column.score == 120.0));
        assert!(candidate
            .columns
            .iter()
            .any(|column| column.key == "trade.gold_pct" && column.score == 35.0));
    }

    #[test]
    fn system_baked_manufacture_trace_candidate_preserves_rejection_metadata() {
        let trace = ManufactureSystemCandidateTrace {
            room: "manu_1".to_string(),
            recipe: "gold".to_string(),
            operators: vec!["清流".to_string(), "温蒂".to_string(), "冬时".to_string()],
            source: "system-baked".to_string(),
            selected: false,
            rejected: true,
            rejection_reason: Some("tier_gate_not_met".to_string()),
            raw_score: Some(128.0),
            evaluation_failed: None,
            linked_producers: vec![ManufactureLinkedProducer {
                station: "power".to_string(),
                operator: "承曦格雷伊".to_string(),
                required_elite: Some(2),
                current_elite: Some(0),
                satisfied: false,
                role: "linked_virtual_power".to_string(),
            }],
            source_system: "automation_group".to_string(),
            evidence: vec!["feedback seed".to_string()],
        };

        let candidate = TeamCandidate::from_manufacture_system_trace(&trace);

        assert_eq!(candidate.source, CandidateSource::SystemBaked);
        assert_eq!(candidate.source_id.as_deref(), Some("automation_group"));
        assert_eq!(candidate.system_tags, vec!["automation_group"]);
        assert_eq!(candidate.selected, Some(false));
        assert_eq!(candidate.rejected, Some(true));
        assert_eq!(candidate.rejection_reason.as_deref(), Some("tier_gate_not_met"));
        assert_eq!(candidate.score.raw_score, 128.0);
        assert_eq!(candidate.score.decision_score, 128.0);
        let value = serde_json::to_value(&candidate).unwrap();
        assert_eq!(value["source"], "system_baked");
        assert_eq!(value["score"]["raw_score"], value["score"]["decision_score"]);
        assert!(candidate.metadata["linked_producers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|producer| producer["operator"] == "承曦格雷伊"));
        assert_eq!(candidate.metadata["evidence"][0], "feedback seed");
    }

    #[test]
    fn manual_trace_without_raw_score_stays_inert() {
        let trace = ManufactureSystemCandidateTrace {
            room: "manu_1".to_string(),
            recipe: "gold".to_string(),
            operators: vec!["清流".to_string(), "温蒂".to_string(), "冬时".to_string()],
            source: "manual-system-candidate".to_string(),
            selected: false,
            rejected: true,
            rejection_reason: Some("missing_operator".to_string()),
            raw_score: None,
            evaluation_failed: Some("missing_operator:温蒂".to_string()),
            linked_producers: vec![],
            source_system: "automation_group".to_string(),
            evidence: vec![],
        };

        let candidate = TeamCandidate::from_manufacture_system_trace(&trace);

        assert_eq!(candidate.score.raw_score, 0.0);
        assert_eq!(candidate.score.decision_score, 0.0);
        assert!(candidate.columns.is_empty());
        assert_eq!(
            candidate.metadata["evaluation_failed"].as_str(),
            Some("missing_operator:温蒂")
        );
    }
}
