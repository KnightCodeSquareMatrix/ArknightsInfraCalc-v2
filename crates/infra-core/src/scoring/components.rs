use serde::Serialize;

use crate::efficiency::Efficiency;

use super::PolicyEvaluation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScoringPolicyId {
    /// Current central-control fill policy: sort by the raw sum of trade and
    /// manufacture inject percentages.
    ///
    /// This is a local sorting heuristic for central-control candidates, not a
    /// trade/manufacture balance formula.
    ControlInjectRawSumV0,
    /// Dynamic producer prefix policy: compare fully settled trade and
    /// manufacture efficiencies with explicit weights.
    TradeManufactureWeightedEfficiencyV1,
}

impl Default for ScoringPolicyId {
    fn default() -> Self {
        Self::ControlInjectRawSumV0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Default)]
pub struct TradeManuEfficiencyComponents {
    pub trade_eff_pct: f64,
    pub gold_manu_eff_pct: f64,
    pub battle_record_manu_eff_pct: f64,
    pub trade_station_count: u8,
    pub gold_line_count: u8,
    pub battle_record_line_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TradeManufactureEfficiencyWeights {
    pub trade: u16,
    pub manufacture: u16,
}

impl Default for TradeManufactureEfficiencyWeights {
    fn default() -> Self {
        Self {
            trade: 1,
            manufacture: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TradeManufactureWeightedEfficiencyEvaluation {
    pub policy: ScoringPolicyId,
    pub trade_total: Efficiency,
    pub manufacture_total: Efficiency,
    pub weights: TradeManufactureEfficiencyWeights,
    /// Weighted direct-efficiency key in thousandths.
    pub sort_key_millis: i64,
}

pub fn evaluate_trade_manufacture_weighted_efficiency(
    trade_total: Efficiency,
    manufacture_total: Efficiency,
    weights: TradeManufactureEfficiencyWeights,
) -> TradeManufactureWeightedEfficiencyEvaluation {
    let sort_key_millis = i64::from(trade_total.millis()) * i64::from(weights.trade)
        + i64::from(manufacture_total.millis()) * i64::from(weights.manufacture);
    TradeManufactureWeightedEfficiencyEvaluation {
        policy: ScoringPolicyId::TradeManufactureWeightedEfficiencyV1,
        trade_total,
        manufacture_total,
        weights,
        sort_key_millis,
    }
}

/// Current central-control inject sorting policy.
///
/// The three efficiency components remain separately meaningful. The returned
/// sort key preserves historical behavior only for ranking control-room fill
/// candidates.
pub fn evaluate_control_inject_policy(input: TradeManuEfficiencyComponents) -> PolicyEvaluation {
    PolicyEvaluation::new(
        ScoringPolicyId::ControlInjectRawSumV0,
        input.trade_eff_pct + input.gold_manu_eff_pct + input.battle_record_manu_eff_pct,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_inject_raw_sum_reports_policy_and_current_sort_key() {
        let score = evaluate_control_inject_policy(TradeManuEfficiencyComponents {
            trade_eff_pct: 7.0,
            gold_manu_eff_pct: 2.0,
            battle_record_manu_eff_pct: 3.0,
            trade_station_count: 2,
            gold_line_count: 2,
            battle_record_line_count: 2,
        });

        assert_eq!(score.policy, ScoringPolicyId::ControlInjectRawSumV0);
        assert_eq!(score.sort_key_pct, 12.0);
    }

    #[test]
    fn weighted_trade_manufacture_policy_uses_direct_efficiency_totals() {
        let score = evaluate_trade_manufacture_weighted_efficiency(
            Efficiency::from_decimal(5.498),
            Efficiency::from_decimal(9.270),
            TradeManufactureEfficiencyWeights::default(),
        );

        assert_eq!(
            score.policy,
            ScoringPolicyId::TradeManufactureWeightedEfficiencyV1
        );
        assert_eq!(score.sort_key_millis, 14_768);
        assert_eq!(score.weights.trade, 1);
        assert_eq!(score.weights.manufacture, 1);
    }
}
