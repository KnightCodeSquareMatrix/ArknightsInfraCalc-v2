use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::types::SkillDef;

#[derive(Debug, Clone, Deserialize)]
pub struct SkillTableFile {
    pub version: u32,
    pub skills: Vec<SkillDef>,
}

#[derive(Debug, Clone)]
pub struct SkillTable {
    by_id: HashMap<String, SkillDef>,
    skills: Vec<SkillDef>,
}

impl SkillTable {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let file: SkillTableFile = serde_json::from_str(&raw)?;
        let mut by_id = HashMap::new();
        for skill in &file.skills {
            if skill.id.starts_with("skill_") {
                return Err(Error::msg(format!(
                    "skill_table id {} uses legacy skill_* namespace; use unpack buff_id",
                    skill.id
                )));
            }
            if by_id.insert(skill.id.clone(), skill.clone()).is_some() {
                return Err(Error::msg(format!("duplicate skill id {}", skill.id)));
            }
        }
        Ok(Self {
            by_id,
            skills: file.skills,
        })
    }

    pub fn get(&self, id: &str) -> Option<&SkillDef> {
        self.by_id.get(id)
    }

    pub fn skills(&self) -> &[SkillDef] {
        &self.skills
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.skills.iter().position(|s| s.id == id)
    }

    pub fn resolve_indices(&self, ids: &[String]) -> Result<Vec<usize>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let idx = self
                .index_of(id)
                .ok_or_else(|| Error::msg(format!("unknown skill_table id {id}")))?;
            out.push(idx);
        }
        Ok(out)
    }

    pub fn validate_operator_refs(&self, instances: &OperatorInstances) -> Vec<String> {
        let mut warnings = Vec::new();
        for (_key, inst) in instances.iter() {
            for bid in inst.trade_buff_ids() {
                if !self.by_id.contains_key(bid) {
                    warnings.push(format!(
                        "{} references unknown skill_table id {}",
                        inst.name, bid
                    ));
                }
            }
        }
        warnings
    }

    /// Hard validation: every resolved trade buff for listed operators must exist in skill_table.
    pub fn validate_pilot_operators(
        &self,
        instances: &OperatorInstances,
        operators: &[&str],
    ) -> Result<()> {
        let mut missing = Vec::new();
        for name in operators {
            for tier in [crate::tier::PromotionTier::Tier0, crate::tier::PromotionTier::TierUp] {
                let key = format!("{}@{}", name, tier.as_str());
                if instances.get(name, tier).is_none() {
                    continue;
                }
                for bid in instances.resolve_trade_buff_ids(name, tier) {
                    if self.get(&bid).is_none() {
                        missing.push(format!("{key}: {bid}"));
                    }
                }
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::msg(format!(
                "pilot operator buff_ids missing from skill_table:\n{}",
                missing.join("\n")
            )))
        }
    }
}

pub fn default_skill_table_path() -> Result<std::path::PathBuf> {
    data_path("skill_table.json")
}

pub fn data_path(name: &str) -> Result<std::path::PathBuf> {
    if let Ok(path) = data_path_from_cwd(name) {
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(workspace_root()?.join("data").join(name))
}

fn data_path_from_cwd(name: &str) -> Result<std::path::PathBuf> {
    let mut path = std::env::current_dir().map_err(Error::from)?;
    path.push("data");
    path.push(name);
    Ok(path)
}

pub fn workspace_root() -> Result<std::path::PathBuf> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| Error::msg("workspace root not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;

    const PILOT_OPS: &[&str] = &[
        "但书", "可露希尔", "孑", "德克萨斯", "拉普兰德", "能天使",
    ];

    fn load_pair() -> (SkillTable, OperatorInstances) {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        (table, instances)
    }

    #[test]
    fn pilot_trade_buff_ids_resolve_in_skill_table() {
        let (table, instances) = load_pair();
        table.validate_pilot_operators(&instances, PILOT_OPS).unwrap();
    }

    #[test]
    fn exusiai_tier_up_stepwise_includes_both_spd_buffs() {
        let (_table, instances) = load_pair();
        let ids = instances.resolve_trade_buff_ids("能天使", crate::tier::PromotionTier::TierUp);
        assert!(ids.contains(&"trade_ord_spd[010]".to_string()));
        assert!(ids.contains(&"trade_ord_spd[020]".to_string()));
    }
}
