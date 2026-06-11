use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Result;
use crate::tier::PromotionTier;

#[derive(Debug, Clone, Deserialize)]
pub struct FacilityBinding {
    pub buff_ids: Vec<String>,
    #[serde(default)]
    pub stepwise: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperatorInstance {
    pub name: String,
    pub tier: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub facilities: HashMap<String, FacilityBinding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperatorInstancesFile {
    pub version: u32,
    pub instances: HashMap<String, OperatorInstance>,
}

#[derive(Debug, Clone)]
pub struct OperatorInstances {
    instances: HashMap<String, OperatorInstance>,
}

/// Stem of a buff id before the `[` index suffix, e.g. `trade_ord_spd[010]` → `trade_ord_spd`.
pub fn buff_stem(id: &str) -> &str {
    id.rsplit_once('[').map(|(stem, _)| stem).unwrap_or(id)
}

/// Expand tier bindings into the buff ids used at runtime.
///
/// - `tier_0`: binding ids as-is
/// - `tier_up` + `stepwise == false`: binding ids as-is (override / replacement skills)
/// - `tier_up` + `stepwise == true`: merge tier_0 ids with tier_up ids; when tier_up is already
///   a superset of tier_0, return tier_up as-is; otherwise replace same-stem tier_0 ids
pub fn resolve_buff_ids(
    tier: PromotionTier,
    binding: &FacilityBinding,
    tier0_binding: Option<&FacilityBinding>,
) -> Vec<String> {
    if tier == PromotionTier::Tier0 {
        return binding.buff_ids.clone();
    }
    if !binding.stepwise {
        return binding.buff_ids.clone();
    }
    let Some(t0) = tier0_binding else {
        return binding.buff_ids.clone();
    };
    merge_stepwise(&t0.buff_ids, &binding.buff_ids)
}

fn merge_stepwise(t0: &[String], up: &[String]) -> Vec<String> {
    if t0.iter().all(|id| up.contains(id)) {
        return up.to_vec();
    }
    let mut out = t0.to_vec();
    for id in up {
        if out.iter().any(|x| x == id) {
            continue;
        }
        let stem = buff_stem(id);
        out.retain(|x| buff_stem(x) != stem);
        out.push(id.clone());
    }
    out
}

impl OperatorInstances {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let file: OperatorInstancesFile = serde_json::from_str(&raw)?;
        Ok(Self {
            instances: file.instances,
        })
    }

    pub fn get(&self, name: &str, tier: PromotionTier) -> Option<&OperatorInstance> {
        let key = format!("{}@{}", name, tier.as_str());
        self.instances.get(&key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &OperatorInstance)> {
        self.instances.iter()
    }

    pub fn resolve_trade_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self
            .get(name, tier)
            .and_then(|i| i.facilities.get("trade"));
        let Some(binding) = tier_binding else {
            return Vec::new();
        };
        let tier0 = self
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("trade"));
        resolve_buff_ids(tier, binding, tier0)
    }

    pub fn trade_buff_ids_for(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        self.resolve_trade_buff_ids(name, tier)
    }
}

impl OperatorInstance {
    pub fn trade_buff_ids(&self) -> Vec<&str> {
        self.facilities
            .get("trade")
            .map(|f| f.buff_ids.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

pub fn default_instances_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("operator_instances.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_stepwise_superset_returns_tier_up() {
        let t0 = vec!["trade_ord_spd[010]".into()];
        let up = vec!["trade_ord_spd[010]".into(), "trade_ord_spd[020]".into()];
        assert_eq!(merge_stepwise(&t0, &up), up);
    }

    #[test]
    fn merge_stepwise_replaces_same_stem() {
        let t0 = vec![
            "trade_ord_against[000]".into(),
            "trade_ord_law[000]".into(),
        ];
        let up = vec![
            "trade_ord_against[010]".into(),
            "trade_ord_law[000]".into(),
        ];
        let expected: Vec<String> = vec![
            "trade_ord_law[000]".to_string(),
            "trade_ord_against[010]".to_string(),
        ];
        assert_eq!(merge_stepwise(&t0, &up), expected);
    }

    #[test]
    fn resolve_buff_ids_non_stepwise_override() {
        let binding = FacilityBinding {
            buff_ids: vec!["trade_ord_limit&cost_P[001]".into()],
            stepwise: false,
        };
        let t0 = FacilityBinding {
            buff_ids: vec!["trade_ord_limit&cost_P[000]".into()],
            stepwise: false,
        };
        assert_eq!(
            resolve_buff_ids(PromotionTier::TierUp, &binding, Some(&t0)),
            vec!["trade_ord_limit&cost_P[001]".to_string()]
        );
    }
}
