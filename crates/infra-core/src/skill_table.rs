use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

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
            let mut atom_ids = HashSet::new();
            for atom in &skill.atoms {
                if let Some(atom_id) = atom.atom_id.as_deref().filter(|id| !id.trim().is_empty()) {
                    if !atom_ids.insert(atom_id) {
                        return Err(Error::msg(format!(
                            "skill {} has duplicate atom id {}",
                            skill.id, atom_id
                        )));
                    }
                } else if skill.facility == "control" {
                    return Err(Error::msg(format!(
                        "control skill {} has non-empty atom without stable id",
                        skill.id
                    )));
                }
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
            for bid in inst
                .trade_buff_ids()
                .into_iter()
                .chain(inst.recycle_buff_ids())
            {
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
            for tier in [
                crate::tier::PromotionTier::Tier0,
                crate::tier::PromotionTier::TierUp,
            ] {
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
    let mut searched = Vec::new();
    if let Some(path) = data_path_from_env(name)? {
        return Ok(path);
    }
    for root in runtime_data_roots()? {
        let path = root.join(name);
        searched.push(path.clone());
        if path.exists() {
            return Ok(path);
        }
    }
    if let Some((embedded_name, bytes)) = embedded_data(name) {
        return materialize_embedded_data(name, embedded_name, bytes);
    }
    let fallback = workspace_root()?.join("data").join(name);
    searched.push(fallback.clone());
    if fallback.exists() {
        Ok(fallback)
    } else {
        Err(Error::msg(format!(
            "data file {name} not found; searched {}",
            searched
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

fn data_path_from_env(name: &str) -> Result<Option<PathBuf>> {
    let Some(root) = std::env::var_os("ARKNIGHTS_INFRA_DATA_DIR") else {
        return Ok(None);
    };
    let path = PathBuf::from(root).join(name);
    if path.exists() {
        Ok(Some(path))
    } else {
        Err(Error::msg(format!(
            "ARKNIGHTS_INFRA_DATA_DIR is set, but {} was not found",
            path.display()
        )))
    }
}

fn runtime_data_roots() -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            push_unique(&mut roots, exe_dir.join("data"));
            if let Some(bundle_parent) = exe_dir.parent() {
                push_unique(&mut roots, bundle_parent.join("data"));
            }
        }
    }
    push_unique(
        &mut roots,
        std::env::current_dir().map_err(Error::from)?.join("data"),
    );
    Ok(roots)
}

fn push_unique(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if !roots.iter().any(|root| root == &path) {
        roots.push(path);
    }
}

fn materialize_embedded_data(
    requested_name: &str,
    embedded_name: &'static str,
    bytes: &'static [u8],
) -> Result<PathBuf> {
    static NEXT_TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    embedded_name.hash(&mut hasher);
    bytes.hash(&mut hasher);
    let root = std::env::temp_dir()
        .join("arknights-infra-calc-v2")
        .join(format!("embedded-{:016x}", hasher.finish()));
    let path = root.join(requested_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let needs_write = match std::fs::metadata(&path) {
        Ok(meta) => meta.len() != bytes.len() as u64,
        Err(_) => true,
    };
    if needs_write {
        // 多线程首次加载同一份 embedded 数据时，不能直接写最终路径：其他线程可能在
        // `write` 完成前看到已创建的空文件。先写线程唯一临时文件，再原子替换。
        let temp_id = NEXT_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("embedded-data");
        let temp_path = root.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            temp_id
        ));
        std::fs::write(&temp_path, bytes)?;
        if let Err(rename_error) = std::fs::rename(&temp_path, &path) {
            let winner_is_valid =
                std::fs::metadata(&path).is_ok_and(|meta| meta.len() == bytes.len() as u64);
            let _ = std::fs::remove_file(&temp_path);
            if !winner_is_valid {
                return Err(rename_error.into());
            }
        }
    }
    Ok(path)
}

fn embedded_data(name: &str) -> Option<(&'static str, &'static [u8])> {
    exact_embedded_data(name).or_else(|| {
        if is_layout_json(name) {
            exact_embedded_data("fixtures/243/layout.json")
        } else if is_operbox_json(name) {
            exact_embedded_data("fixtures/243/operbox_full_e2.json")
        } else {
            None
        }
    })
}

fn is_layout_json(name: &str) -> bool {
    name.starts_with("layout/") && name.ends_with(".json")
}

fn is_operbox_json(name: &str) -> bool {
    let Some(file_name) = Path::new(name).file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    file_name.ends_with(".json")
        && (file_name == "operbox.json"
            || file_name.starts_with("operbox_")
            || file_name.ends_with("_operbox.json"))
}

fn exact_embedded_data(name: &str) -> Option<(&'static str, &'static [u8])> {
    let bytes = match name {
        "operator_instances.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/operator_instances.json"
        )) as &[u8],
        "skill_table.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/skill_table.json"
        )) as &[u8],
        "producer_rules.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/producer_rules.json"
        )) as &[u8],
        "mood_model.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/mood_model.json"
        )) as &[u8],
        "standalone_roster.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/standalone_roster.json"
        )) as &[u8],
        "base_systems.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/base_systems.json"
        )) as &[u8],
        "orchestration_rules.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/orchestration_rules.json"
        )) as &[u8],
        "trade_segments.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/trade_segments.json"
        )) as &[u8],
        "trade_shortcuts.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/trade_shortcuts.json"
        )) as &[u8],
        "training_recommendations.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/training_recommendations.json"
        )) as &[u8],
        "training_advice_knowledge.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/training_advice_knowledge.json"
        )) as &[u8],
        "fixtures/243/layout.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/fixtures/243/layout.json"
        )) as &[u8],
        "fixtures/243/operbox_full_e2.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/fixtures/243/operbox_full_e2.json"
        )) as &[u8],
        "fixtures/243/schedule_export.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/fixtures/243/schedule_export.json"
        )) as &[u8],
        "roster.csv" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/roster.csv"
        )) as &[u8],
        "roster_gongsun.csv" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/roster_gongsun.csv"
        )) as &[u8],
        "operbox_gongsun.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/operbox_gongsun.json"
        )) as &[u8],
        "layout/153.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/153.json"
        )) as &[u8],
        "layout/243.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/243.json"
        )) as &[u8],
        "layout/243_use_this_.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/243_use_this_.json"
        )) as &[u8],
        "layout/252.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/252.json"
        )) as &[u8],
        "layout/333.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/333.json"
        )) as &[u8],
        "layout/342.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/342.json"
        )) as &[u8],
        "layout/snhunt.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/layout/snhunt.json"
        )) as &[u8],
        "REGRESSION_CASES.csv" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/REGRESSION_CASES.csv"
        )) as &[u8],
        "UNIT_OUTPUT_ANCHORS.csv" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/UNIT_OUTPUT_ANCHORS.csv"
        )) as &[u8],
        "schedule_243/operbox_ideal_e2.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/schedule_243/operbox_ideal_e2.json"
        )) as &[u8],
        "schedule_243/assignment_automation_trio_e2.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/schedule_243/assignment_automation_trio_e2.json"
        )) as &[u8],
        "schedule_243/assignment_gongsun_closure_docus.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/schedule_243/assignment_gongsun_closure_docus.json"
        )) as &[u8],
        "schedule_243/assignment_greedy_witch_closure.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/schedule_243/assignment_greedy_witch_closure.json"
        )) as &[u8],
        "schedule_243/assignment_ideal_witch_docus.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/schedule_243/assignment_ideal_witch_docus.json"
        )) as &[u8],
        _ => return None,
    };
    Some((
        match name {
            "operator_instances.json" => "operator_instances.json",
            "skill_table.json" => "skill_table.json",
            "producer_rules.json" => "producer_rules.json",
            "mood_model.json" => "mood_model.json",
            "standalone_roster.json" => "standalone_roster.json",
            "base_systems.json" => "base_systems.json",
            "orchestration_rules.json" => "orchestration_rules.json",
            "trade_segments.json" => "trade_segments.json",
            "trade_shortcuts.json" => "trade_shortcuts.json",
            "training_recommendations.json" => "training_recommendations.json",
            "training_advice_knowledge.json" => "training_advice_knowledge.json",
            "fixtures/243/layout.json" => "fixtures/243/layout.json",
            "fixtures/243/operbox_full_e2.json" => "fixtures/243/operbox_full_e2.json",
            "fixtures/243/schedule_export.json" => "fixtures/243/schedule_export.json",
            "roster.csv" => "roster.csv",
            "roster_gongsun.csv" => "roster_gongsun.csv",
            "operbox_gongsun.json" => "operbox_gongsun.json",
            "layout/153.json" => "layout/153.json",
            "layout/243.json" => "layout/243.json",
            "layout/243_use_this_.json" => "layout/243_use_this_.json",
            "layout/252.json" => "layout/252.json",
            "layout/333.json" => "layout/333.json",
            "layout/342.json" => "layout/342.json",
            "layout/snhunt.json" => "layout/snhunt.json",
            "REGRESSION_CASES.csv" => "REGRESSION_CASES.csv",
            "UNIT_OUTPUT_ANCHORS.csv" => "UNIT_OUTPUT_ANCHORS.csv",
            "schedule_243/operbox_ideal_e2.json" => "schedule_243/operbox_ideal_e2.json",
            "schedule_243/assignment_automation_trio_e2.json" => {
                "schedule_243/assignment_automation_trio_e2.json"
            }
            "schedule_243/assignment_gongsun_closure_docus.json" => {
                "schedule_243/assignment_gongsun_closure_docus.json"
            }
            "schedule_243/assignment_greedy_witch_closure.json" => {
                "schedule_243/assignment_greedy_witch_closure.json"
            }
            "schedule_243/assignment_ideal_witch_docus.json" => {
                "schedule_243/assignment_ideal_witch_docus.json"
            }
            _ => unreachable!(),
        },
        bytes,
    ))
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

    const PILOT_OPS: &[&str] = &["但书", "可露希尔", "孑", "德克萨斯", "拉普兰德", "能天使"];

    fn load_pair() -> (SkillTable, OperatorInstances) {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        (table, instances)
    }

    #[test]
    fn solver_runtime_data_is_embedded() {
        for name in [
            "operator_instances.json",
            "skill_table.json",
            "producer_rules.json",
            "mood_model.json",
            "standalone_roster.json",
            "base_systems.json",
            "orchestration_rules.json",
            "trade_segments.json",
            "trade_shortcuts.json",
            "training_recommendations.json",
            "training_advice_knowledge.json",
        ] {
            assert!(
                exact_embedded_data(name).is_some(),
                "missing embedded {name}"
            );
        }
    }

    #[test]
    fn embedded_producer_catalog_loads_through_runtime_loader() {
        let (skill_name, skill_bytes) = exact_embedded_data("skill_table.json").unwrap();
        let skill_path = materialize_embedded_data(
            "tests/embedded-runtime/skill_table.json",
            skill_name,
            skill_bytes,
        )
        .unwrap();
        let table = SkillTable::load(&skill_path).unwrap();

        let (producer_name, producer_bytes) = exact_embedded_data("producer_rules.json").unwrap();
        let producer_path = materialize_embedded_data(
            "tests/embedded-runtime/producer_rules.json",
            producer_name,
            producer_bytes,
        )
        .unwrap();
        let catalog =
            crate::response_dependency::load_producer_rule_catalog(&producer_path, &table).unwrap();

        assert_eq!(catalog.rules.len(), 4);
        assert!(catalog
            .rules
            .iter()
            .any(|rule| rule.id == "trade_glasgow_scaling"));
        assert!(catalog
            .rules
            .iter()
            .any(|rule| rule.id == "manu_blacksteel_scaling"));
    }

    #[test]
    fn embedded_data_materialization_is_atomic_under_concurrency() {
        const BYTES: &[u8] = b"atomic embedded data regression\n";
        let path = materialize_embedded_data(
            "tests/atomic-materialization.txt",
            "atomic-materialization-v1",
            BYTES,
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
        let workers: Vec<_> = (0..12)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let path = materialize_embedded_data(
                        "tests/atomic-materialization.txt",
                        "atomic-materialization-v1",
                        BYTES,
                    )
                    .unwrap();
                    std::fs::read(path).unwrap()
                })
            })
            .collect();

        for worker in workers {
            assert_eq!(worker.join().unwrap(), BYTES);
        }
    }

    #[test]
    fn pilot_trade_buff_ids_resolve_in_skill_table() {
        let (table, instances) = load_pair();
        table
            .validate_pilot_operators(&instances, PILOT_OPS)
            .unwrap();
    }

    #[test]
    fn exusiai_tier_up_stepwise_replaces_alpha_with_expert() {
        let (_table, instances) = load_pair();
        let ids = instances.resolve_trade_buff_ids("能天使", crate::tier::PromotionTier::TierUp);
        assert_eq!(ids, vec!["trade_ord_spd[020]".to_string()]);
    }

    #[test]
    fn all_manufacture_buff_ids_resolve_in_skill_table() {
        let (table, instances) = load_pair();
        let mut missing = Vec::new();
        for (_key, inst) in instances.iter() {
            let name = inst.name.clone();
            for tier in [
                crate::tier::PromotionTier::Tier0,
                crate::tier::PromotionTier::TierUp,
            ] {
                for bid in instances.resolve_manufacture_buff_ids(&name, tier) {
                    if table.get(&bid).is_none() {
                        missing.push(format!("{}@{}: {bid}", name, tier.as_str()));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "manufacture buff_ids missing from skill_table:\n{}",
            missing.join("\n")
        );
    }

    #[test]
    fn manufacture_constant_skills_have_atoms() {
        let (table, _instances) = load_pair();
        let mut empty_constants = Vec::new();
        for skill in table.skills() {
            if skill.facility != "manufacture" {
                continue;
            }
            let id = skill.id.as_str();
            let delegated = id.contains("variable")
                || id.contains("skill_change")
                || id.contains("skill_spd1")
                || id.contains("skill_limit")
                || id.contains("bd[")
                || id.contains("bd_to")
                || id.contains("constrLv")
                || id.contains("cost_all")
                || id.contains("double[")
                || id.contains("fraction")
                || id.contains("addition")
                || id.contains("reduce[")
                || id.contains("train&lv")
                || id.contains("token_prod")
                || id.contains("formula_spd&cost")
                || id.contains("formula_spd&bd")
                || id.contains("formula_spd&dorm")
                || id.contains("formula_spd_P")
                || id.contains("bd_n1")
                || id.contains("prod_bd[")
                || id == "manu_prod_spd[1000]";
            if !delegated && skill.atoms.is_empty() {
                empty_constants.push(id.to_string());
            }
        }
        assert!(
            empty_constants.is_empty(),
            "expected constant manufacture skills to have atoms: {:?}",
            empty_constants
        );
    }

    #[test]
    fn control_atom_without_id_is_rejected() {
        let path = write_skill_table_fixture(
            r#"{"version":1,"skills":[{"id":"control_test","skill_name":"test","facility":"control","tier":"tier_0","atoms":[{"action":{"kind":"state_produce","params":{"key":"Passion","amount":1.0}},"phase":"state_write","phase_order":10}]}]}"#,
        );
        let error = SkillTable::load(&path).unwrap_err().to_string();
        remove_skill_table_fixture(&path);
        assert!(error.contains("without stable id"), "{error}");
    }

    #[test]
    fn duplicate_control_atom_id_is_rejected() {
        let path = write_skill_table_fixture(
            r#"{"version":1,"skills":[{"id":"control_test","skill_name":"test","facility":"control","tier":"tier_0","atoms":[{"id":"same","action":{"kind":"state_produce","params":{"key":"Passion","amount":1.0}},"phase":"state_write","phase_order":10},{"id":"same","action":{"kind":"state_produce","params":{"key":"Passion","amount":1.0}},"phase":"state_write","phase_order":11}]}]}"#,
        );
        let error = SkillTable::load(&path).unwrap_err().to_string();
        remove_skill_table_fixture(&path);
        assert!(error.contains("duplicate atom id"), "{error}");
    }

    #[test]
    fn duplicate_non_control_atom_id_is_rejected() {
        let path = write_skill_table_fixture(
            r#"{"version":1,"skills":[{"id":"trade_test","skill_name":"test","facility":"trade","tier":"tier_0","atoms":[{"id":"same","action":{"kind":"add_flat_eff","params":{"value":1.0}},"phase":"constant","phase_order":10},{"id":"same","action":{"kind":"add_flat_eff","params":{"value":2.0}},"phase":"constant","phase_order":11}]}]}"#,
        );
        let error = SkillTable::load(&path).unwrap_err().to_string();
        remove_skill_table_fixture(&path);
        assert!(error.contains("duplicate atom id"), "{error}");
    }

    #[test]
    fn existing_multi_atom_control_ids_are_loaded() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let skill = table.get("control_mp_psk[000]").unwrap();
        let ids: Vec<_> = skill
            .atoms
            .iter()
            .map(|atom| atom.atom_id.as_deref())
            .collect();
        assert_eq!(
            ids,
            vec![
                Some("control_mp_psk[000]:pinus_battle_record"),
                Some("control_mp_psk[000]:pinus_gold")
            ]
        );
    }

    fn write_skill_table_fixture(raw: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "arknights-skill-table-test-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(&path, raw).unwrap();
        path
    }

    fn remove_skill_table_fixture(path: &std::path::Path) {
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn all_power_buff_ids_resolve_in_skill_table() {
        let (table, instances) = load_pair();
        let mut missing = Vec::new();
        for (_key, inst) in instances.iter() {
            let name = inst.name.clone();
            for tier in [
                crate::tier::PromotionTier::Tier0,
                crate::tier::PromotionTier::TierUp,
            ] {
                for bid in instances.resolve_power_buff_ids(&name, tier) {
                    if table.get(&bid).is_none() {
                        missing.push(format!("{}@{}: {bid}", name, tier.as_str()));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "power buff_ids missing from skill_table:\n{}",
            missing.join("\n")
        );
    }
}
