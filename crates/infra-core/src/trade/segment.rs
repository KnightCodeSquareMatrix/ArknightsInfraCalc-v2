//! 贸易链段（EffSegment）：producer manifest + consumer 组合 → L3 shortcut 锚点。
//!
//! 数据：`data/trade_segments.json`；求解命中见 `match_registered_trade_segment`；
//! meta 落位见 `search/role_pick.rs`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::Result;
use crate::global_resource::GlobalInjectManifest;
use crate::skill_table::{data_path, SkillTable};
use crate::trade::input::TradeOperator;
use crate::trade::shortcut::{
    is_docus_syracusa_station, match_ling_jie_shortcut, trade_shortcut_cache, TradeShortcutMatch,
};

#[derive(Debug, Clone, Deserialize)]
struct TradeSegmentFile {
    segments: Vec<TradeSegmentDef>,
    roles: Vec<TradeRoleDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradeSegmentDef {
    pub id: String,
    pub shortcut_id: String,
    #[serde(default)]
    pub priority: i32,
    pub producer: SegmentProducerDef,
    pub consumer: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SegmentProducerDef {
    #[serde(default)]
    pub haru_e2_in_control: bool,
    #[serde(default)]
    pub karlan_precision: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradeRoleDef {
    pub id: String,
    pub pick_steps: Vec<RolePickStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RolePickStep {
    pub kind: String,
    #[serde(default)]
    pub segment_id: Option<String>,
    #[serde(default)]
    pub shortcut_id: Option<String>,
    #[serde(default)]
    pub only_if_producer: bool,
}

pub(crate) struct TradeSegmentCache {
    segments: Vec<TradeSegmentDef>,
    by_id: HashMap<String, usize>,
    roles: HashMap<String, TradeRoleDef>,
}

impl TradeSegmentCache {
    fn build(file: TradeSegmentFile) -> Self {
        let mut by_id = HashMap::new();
        for (i, seg) in file.segments.iter().enumerate() {
            by_id.insert(seg.id.clone(), i);
        }
        let roles = file.roles.into_iter().map(|r| (r.id.clone(), r)).collect();
        Self {
            segments: file.segments,
            by_id,
            roles,
        }
    }

    pub(crate) fn segment(&self, id: &str) -> Option<&TradeSegmentDef> {
        self.by_id.get(id).map(|&i| &self.segments[i])
    }

    pub(crate) fn role(&self, id: &str) -> Option<&TradeRoleDef> {
        self.roles.get(id)
    }

    fn segments_by_priority(&self) -> Vec<&TradeSegmentDef> {
        let mut list: Vec<_> = self.segments.iter().collect();
        list.sort_by(|a, b| b.priority.cmp(&a.priority));
        list
    }
}

static TRADE_SEGMENT_CACHE: OnceLock<Option<TradeSegmentCache>> = OnceLock::new();

pub fn load_trade_segments(path: &Path) -> Result<TradeSegmentFile> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn default_segments_path() -> Result<std::path::PathBuf> {
    data_path("trade_segments.json")
}

pub(crate) fn trade_segment_cache() -> Option<&'static TradeSegmentCache> {
    TRADE_SEGMENT_CACHE
        .get_or_init(|| {
            let path = default_segments_path().ok()?;
            let file = load_trade_segments(&path).ok()?;
            Some(TradeSegmentCache::build(file))
        })
        .as_ref()
}

pub fn segment_producer_active(producer: &SegmentProducerDef, inject: &GlobalInjectManifest) -> bool {
    if producer.haru_e2_in_control && !inject.haru_e2_in_control() {
        return false;
    }
    if producer.karlan_precision && inject.karlan_precision().is_none() {
        return false;
    }
    producer.haru_e2_in_control || producer.karlan_precision
}

fn segment_consumer_matches(
    kind: &str,
    ops: &[TradeOperator],
    table: &SkillTable,
    inject: &GlobalInjectManifest,
) -> bool {
    match kind {
        "docus_syracusa" => is_docus_syracusa_station(ops, table),
        "ling_jie" => match_ling_jie_shortcut(ops, inject).is_some(),
        _ => false,
    }
}

/// 按 `priority` 命中第一个链段；用于 `resolve_trade_shortcut`。
pub fn match_registered_trade_segment(
    ops: &[TradeOperator],
    table: &SkillTable,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    let cache = trade_segment_cache()?;
    for seg in cache.segments_by_priority() {
        if !segment_producer_active(&seg.producer, inject) {
            continue;
        }
        if !segment_consumer_matches(&seg.consumer, ops, table, inject) {
            continue;
        }
        let entry = trade_shortcut_cache()?.get_by_id(&seg.shortcut_id)?.clone();
        return Some(TradeShortcutMatch { entry });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::pool::build_trade_pool;
    use crate::roster::Roster;
    use crate::trade::shortcut::resolve_trade_shortcut;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    #[test]
    fn segment_registry_loads_docus_and_ling_jie() {
        let cache = trade_segment_cache().expect("segments loaded");
        assert!(cache.segment("docus_syracusa").is_some());
        assert!(cache.segment("ling_jie").is_some());
        assert!(cache.role("docus").is_some());
    }

    #[test]
    fn registered_segment_docus_syracusa_matches_via_resolve() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("但书", 2), ("伺夜", 2), ("贝洛内", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let ops: Vec<TradeOperator> = ["但书", "伺夜", "贝洛内"]
            .iter()
            .map(|n| pool.entry(n).unwrap().to_trade_operator())
            .collect();
        let mut inject = GlobalInjectManifest::default();
        inject.record_haru_e2_in_control();
        let m = resolve_trade_shortcut(&ops, &table, 80.0, 3, &inject).expect("match");
        assert_eq!(m.entry.id, "gsl_docus_syracusa");
    }
}
