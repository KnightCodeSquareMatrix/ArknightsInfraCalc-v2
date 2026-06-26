use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Result;
use crate::tier::PromotionTier;

/// Whether an elite-2 / tier_up binding **upgrades** the tier_0 buffs (stem-matched merge),
/// or **replaces** them entirely.
///
/// - `true`  → tier_up replaces same-stem tier_0 buffs while preserving others (α→β upgrade).
/// - `false` → tier_up completely replaces the tier_0 binding; only tier_up buffs are used
///             (entirely new skill at E2, e.g. 鸿雪 销路宣发→际崖居民).
///
/// The common case for `true` is "single-buff α→β" where the result is identical to `false`,
/// but `true` is semantically correct for future multi-buff scenarios (e.g. 贝洛内 E2 keeps
/// `trade_ord_limit&cost_P[020]` from E0 while upgrading `trade_ord_spd_ext[020]→[021]`).
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
/// - `tier_up` + `stepwise == true`: merge tier_0 ids with tier_up ids; same-stem tier_0 ids
///   are replaced by the tier_up id (e.g. 温蒂 自动化·β → 仿生海龙)
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

    pub fn tags_for(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        self.get(name, tier)
            .or_else(|| self.get(name, PromotionTier::TierUp))
            .or_else(|| self.get(name, PromotionTier::Tier0))
            .map(|i| i.tags.clone())
            .unwrap_or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &OperatorInstance)> {
        self.instances.iter()
    }

    pub fn resolve_trade_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self.get(name, tier).and_then(|i| i.facilities.get("trade"));
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

    pub fn resolve_control_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self
            .get(name, tier)
            .and_then(|i| i.facilities.get("control"));
        let Some(binding) = tier_binding else {
            return Vec::new();
        };
        let tier0 = self
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("control"));
        resolve_buff_ids(tier, binding, tier0)
    }

    pub fn resolve_manufacture_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self
            .get(name, tier)
            .and_then(|i| i.facilities.get("manufacture"));
        let Some(binding) = tier_binding else {
            return Vec::new();
        };
        let tier0 = self
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("manufacture"));
        resolve_buff_ids(tier, binding, tier0)
    }

    pub fn resolve_power_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self.get(name, tier).and_then(|i| i.facilities.get("power"));
        let Some(binding) = tier_binding else {
            return Vec::new();
        };
        let tier0 = self
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("power"));
        resolve_buff_ids(tier, binding, tier0)
    }

    pub fn resolve_dorm_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self.get(name, tier).and_then(|i| i.facilities.get("dorm"));
        let Some(binding) = tier_binding else {
            return Vec::new();
        };
        let tier0 = self
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("dorm"));
        resolve_buff_ids(tier, binding, tier0)
    }

    pub fn resolve_office_buff_ids(&self, name: &str, tier: PromotionTier) -> Vec<String> {
        let tier_binding = self
            .get(name, tier)
            .and_then(|i| i.facilities.get("office"));
        let Some(binding) = tier_binding else {
            return Vec::new();
        };
        let tier0 = self
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("office"));
        resolve_buff_ids(tier, binding, tier0)
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
        assert_eq!(
            merge_stepwise(&t0, &up),
            vec!["trade_ord_spd[020]".to_string()]
        );
    }

    #[test]
    fn merge_stepwise_replaces_same_stem() {
        let t0 = vec!["trade_ord_against[000]".into(), "trade_ord_law[000]".into()];
        let up = vec!["trade_ord_against[010]".into(), "trade_ord_law[000]".into()];
        let expected: Vec<String> = vec![
            "trade_ord_law[000]".to_string(),
            "trade_ord_against[010]".to_string(),
        ];
        assert_eq!(merge_stepwise(&t0, &up), expected);
    }

    #[test]
    fn merge_stepwise_weedy_manu_power_keeps_highest_only() {
        let t0 = vec!["manu_prod_spd&power[010]".into()];
        let up = vec![
            "manu_prod_spd&power[010]".into(),
            "manu_prod_spd&power[020]".into(),
        ];
        assert_eq!(
            merge_stepwise(&t0, &up),
            vec!["manu_prod_spd&power[020]".to_string()]
        );
    }

    #[test]
    fn resolve_saria_tier_up_manu_beta_only() {
        let binding = FacilityBinding {
            buff_ids: vec!["manu_prod_spd&power[010]".into()],
            stepwise: false,
        };
        assert_eq!(
            resolve_buff_ids(PromotionTier::TierUp, &binding, None),
            vec!["manu_prod_spd&power[010]".to_string()]
        );
    }

    #[test]
    fn resolve_stellar_thorn_tier_up_keeps_metal_and_trade_bonus() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let ids = instances.resolve_manufacture_buff_ids("引星棘刺", PromotionTier::TierUp);
        assert_eq!(
            ids,
            vec![
                "manu_formula_spd[101]".to_string(),
                "manu_prod_spd&trade[1000]".to_string(),
            ],
            "精2 应保留金属工艺并叠加原质塑金（勿用 manu_formula_spd[102] 替换金属）"
        );
    }

    #[test]
    fn resolve_cai_du_manu_tier0_and_tier_up_do_not_stack() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        assert_eq!(
            instances.resolve_manufacture_buff_ids("裁度", PromotionTier::Tier0),
            vec!["manu_prod_spd&limit&cost[200]".to_string()],
            "裁度精0应为量体裁衣 20%，不是得心应手"
        );
        assert_eq!(
            instances.resolve_manufacture_buff_ids("裁度", PromotionTier::TierUp),
            vec!["manu_prod_spd&limit&cost[100]".to_string()],
            "裁度精2应替换为得心应手 25%+仓库，不应与量体裁衣叠加"
        );
    }
}
