mod rank;
mod state;

use state::{build_variant_pools, distinct_names, OperatorState};

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant as StdInstant;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::layout::LayoutContext;
use crate::pool::{
    build_trade_combo_operators_vec, combinations_indices, filter_general_manufacture_search_pool,
    n_choose_k_u64,
};
use crate::pool::{filter_standalone_exact_with, StandaloneFilter};
use crate::pool::{HasName, ManuPool, PoolCore, TradePool};
use crate::search::{
    ManuEfficiencyBreakdown, ManuSearchHit, ManuSearchOptions, ManuSearchReport,
    SearchTripleFilter, TradeEfficiencyBreakdown, TradeSearchHit, TradeSearchOptions,
    TradeSearchReport,
};
use crate::skill_table::{data_path, default_skill_table_path, SkillTable};
use crate::trade::input::{TradeOperator, TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use crate::trade::shortcut::trade_station_exclusive_violation;
use crate::trade::solver::solve_trade_with_shift_prevalidated;
use crate::types::{CompiledAtom, Condition, RecipeKind, Selector};
use crate::FacilityKind;

pub const BAKE_SCHEMA_VERSION: u32 = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BakeMode {
    #[default]
    Auto,
    Disabled,
    Required,
}

fn baked_miss<T>(mode: BakeMode, reason: impl Into<String>) -> Result<Option<T>> {
    if mode == BakeMode::Required {
        Err(Error::msg(format!("required Bake miss: {}", reason.into())))
    } else {
        Ok(None)
    }
}

fn baked_miss_lazy<T>(mode: BakeMode, reason: impl FnOnce() -> String) -> Result<Option<T>> {
    if mode == BakeMode::Required {
        baked_miss(mode, reason())
    } else {
        Ok(None)
    }
}

pub type BakeProgressCallback = Arc<dyn Fn(BakeProgressEvent) + Send + Sync>;

#[derive(Clone)]
pub struct BakeOptions {
    pub out_dir: PathBuf,
    pub include_trade: bool,
    pub include_manufacture: bool,
    pub limit_per_signature: Option<usize>,
    pub generator: Option<BakeGeneratorFingerprint>,
    pub progress: Option<BakeProgressCallback>,
}

impl Default for BakeOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("data/baked"),
            include_trade: true,
            include_manufacture: true,
            limit_per_signature: None,
            generator: None,
            progress: None,
        }
    }
}

impl std::fmt::Debug for BakeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BakeOptions")
            .field("out_dir", &self.out_dir)
            .field("include_trade", &self.include_trade)
            .field("include_manufacture", &self.include_manufacture)
            .field("limit_per_signature", &self.limit_per_signature)
            .field("generator", &self.generator)
            .field("progress", &self.progress.as_ref().map(|_| "<callback>"))
            .finish()
    }
}

pub fn default_baked_out_dir() -> Result<PathBuf> {
    if let Some(out_dir) = std::env::var_os("ARKNIGHTS_INFRA_BAKED_DIR") {
        return Ok(PathBuf::from(out_dir));
    }
    let roots = default_baked_data_roots()?;
    for root in &roots {
        let out_dir = root.join("baked");
        if out_dir.join("CURRENT").exists() || out_dir.join("manifest.json").exists() {
            return Ok(out_dir);
        }
    }
    Ok(roots
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("data"))
        .join("baked"))
}

/// Resolve an atomically published generation, or a legacy flat catalog.
/// CURRENT accepts exactly one directory name; symlinks may not escape the root.
pub fn resolved_baked_catalog_dir(out_dir: &Path) -> Result<PathBuf> {
    let pointer = out_dir.join("CURRENT");
    let name = match fs::read_to_string(&pointer) {
        Ok(name) => name,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(out_dir.to_path_buf());
        }
        Err(error) => return Err(error.into()),
    };
    let name = name.trim();
    let mut components = Path::new(name).components();
    if name.is_empty()
        || name.contains(['/', '\\'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(Error::msg("invalid baked CURRENT generation name"));
    }
    let root = fs::canonicalize(out_dir)?;
    let generation = fs::canonicalize(out_dir.join(name))?;
    if generation.parent() != Some(root.as_path()) || !generation.is_dir() {
        return Err(Error::msg(
            "baked CURRENT generation escapes catalog directory",
        ));
    }
    Ok(generation)
}

fn default_baked_data_roots() -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("ARKNIGHTS_INFRA_DATA_DIR") {
        push_unique_path(&mut roots, PathBuf::from(root));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            push_unique_path(&mut roots, exe_dir.join("data"));
            if let Some(bundle_parent) = exe_dir.parent() {
                push_unique_path(&mut roots, bundle_parent.join("data"));
            }
        }
    }
    push_unique_path(
        &mut roots,
        std::env::current_dir().map_err(Error::from)?.join("data"),
    );
    Ok(roots)
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|candidate| candidate == &path) {
        paths.push(path);
    }
}

#[derive(Debug, Clone)]
pub enum BakeProgressEvent {
    Started {
        out_dir: PathBuf,
        operator_count: usize,
        signature_count: usize,
        worker_count: usize,
    },
    SignatureStarted {
        facility: &'static str,
        signature_key: String,
        combo_count: u64,
    },
    SignatureFinished {
        facility: &'static str,
        signature_key: String,
        rows: usize,
        elapsed_ms: u128,
    },
    Writing {
        path: PathBuf,
        rows: Option<usize>,
    },
    Finished {
        combo_table_rows: usize,
        elapsed_ms: u128,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakeGeneratorFingerprint {
    pub kind: String,
    pub path: String,
    pub bytes: u64,
    pub hash64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BakeReport {
    pub out_dir: PathBuf,
    pub schema_version: u32,
    pub operator_count: usize,
    pub trade_signatures: usize,
    pub trade_hits: usize,
    pub trade_mechanism_rows: usize,
    pub trade_mechanism_signatures: usize,
    pub manufacture_signatures: usize,
    pub manufacture_hits: usize,
    pub combo_table_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<BakeGeneratorFingerprint>,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct CompleteBakedTradeRow {
    pub row_id: BakedTradeRowId,
    pub names: Vec<String>,
    pub logical_mask: Vec<u64>,
    pub siracusa_count: u8,
    pub glasgow_count: u8,
    pub karlan_count: u8,
    pub baseline_response: TradeSearchHit,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BakedTradeRowId {
    pub signature_key: String,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct CompleteBakedTradeRows {
    pub signature_key: String,
    pub rows: Vec<CompleteBakedTradeRow>,
}

pub struct CompleteBakedTradeCatalog {
    disk: BakedComboTableDisk,
    trade_pool: TradePool,
    trade_state_index: HashMap<OperatorState, usize>,
    skill_table: SkillTable,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakeManifest {
    schema_version: u32,
    generated_by: String,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator: Option<BakeGeneratorFingerprint>,
    inputs: Vec<BakeInputFingerprint>,
    options: BakeManifestOptions,
    combo_table: BakeComboTableFingerprint,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakeComboTableFingerprint {
    bytes: u64,
    hash64: String,
    rows_by_signature: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakeManifestOptions {
    include_trade: bool,
    include_manufacture: bool,
    limit_per_signature: Option<usize>,
    roster_model: String,
    layout_model: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakeInputFingerprint {
    path: String,
    bytes: u64,
    hash64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakedComboTable {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    #[serde(default)]
    operator_names: Vec<String>,
    operator_states: Vec<OperatorState>,
    mask_words: usize,
    indexes: Vec<BakedComboIndex>,
    rows: Vec<BakedComboRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BakedComboTableDisk {
    schema_version: u32,
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    operator_names: Vec<String>,
    operator_states: Vec<OperatorState>,
    mask_words: usize,
    indexes: Vec<BakedComboIndex>,
    rows: Vec<BakedComboRowDisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct BakedComboRowDisk {
    room_level: u8,
    operator_capacity: usize,
    operator_indices: Vec<usize>,
    sort_efficiency_millis: i32,
    order_kind: Option<TradeOrderKind>,
    recipe: Option<RecipeKind>,
    trade_mechanism_signature: Option<TradeRoomMechanismSignature>,
    trade_base_efficiency_millis: Option<i32>,
    trade_occupancy_efficiency_millis: Option<i32>,
    trade_skill_efficiency_millis: Option<i32>,
    trade_control_efficiency_millis: Option<i32>,
    trade_paper_efficiency_millis: Option<i32>,
    trade_mechanic_equivalent_efficiency_millis: Option<i32>,
    trade_unit_output_multiplier_millis: Option<i32>,
    trade_final_efficiency_millis: Option<i32>,
    trade_equivalent_skill_efficiency_millis: Option<i32>,
    trade_gold_equivalent_efficiency_millis: Option<i32>,
    trade_combined_output_value_millis: Option<i32>,
    trade_total_efficiency_millis: Option<i32>,
    rule_id: Option<String>,
    unit_trade_per_day: Option<f64>,
    unit_gold_per_day: Option<f64>,
    unit_gold_consumption_per_day: Option<f64>,
    unit_originium_per_day: Option<f64>,
    manufacture_base_efficiency_millis: Option<i32>,
    manufacture_occupancy_efficiency_millis: Option<i32>,
    manufacture_skill_efficiency_millis: Option<i32>,
    manufacture_global_efficiency_millis: Option<i32>,
    manufacture_final_efficiency_millis: Option<i32>,
    manufacture_storage_limit: Option<i32>,
    manufacture_unit_output_per_day: Option<f64>,
    manufacture_output_per_day: Option<f64>,
}

impl From<&BakedComboTable> for BakedComboTableDisk {
    fn from(value: &BakedComboTable) -> Self {
        Self {
            schema_version: value.schema_version,
            generator: value.generator.clone(),
            operator_count: value.operator_count,
            operator_names: value.operator_names.clone(),
            operator_states: value.operator_states.clone(),
            mask_words: value.mask_words,
            indexes: value.indexes.clone(),
            rows: value.rows.iter().map(BakedComboRowDisk::from).collect(),
        }
    }
}

impl From<&BakedComboRow> for BakedComboRowDisk {
    fn from(value: &BakedComboRow) -> Self {
        Self {
            room_level: value.room_level,
            operator_capacity: value.operator_capacity,
            operator_indices: value.operator_indices.clone(),
            sort_efficiency_millis: value.sort_efficiency.millis(),
            order_kind: value.order_kind,
            recipe: value.recipe,
            trade_mechanism_signature: value.trade_mechanism_signature.clone(),
            trade_base_efficiency_millis: value.trade_base_efficiency.map(Efficiency::millis),
            trade_occupancy_efficiency_millis: value
                .trade_occupancy_efficiency
                .map(Efficiency::millis),
            trade_skill_efficiency_millis: value.trade_skill_efficiency.map(Efficiency::millis),
            trade_control_efficiency_millis: value.trade_control_efficiency.map(Efficiency::millis),
            trade_paper_efficiency_millis: value.trade_paper_efficiency.map(Efficiency::millis),
            trade_mechanic_equivalent_efficiency_millis: value
                .trade_mechanic_equivalent_efficiency
                .map(Efficiency::millis),
            trade_unit_output_multiplier_millis: value
                .trade_unit_output_multiplier
                .map(Efficiency::millis),
            trade_final_efficiency_millis: value.trade_final_efficiency.map(Efficiency::millis),
            trade_equivalent_skill_efficiency_millis: value
                .trade_equivalent_skill_efficiency
                .map(Efficiency::millis),
            trade_gold_equivalent_efficiency_millis: value
                .trade_gold_equivalent_efficiency
                .map(Efficiency::millis),
            trade_combined_output_value_millis: value
                .trade_combined_output_value
                .map(Efficiency::millis),
            trade_total_efficiency_millis: value.trade_total_efficiency.map(Efficiency::millis),
            rule_id: value.rule_id.clone(),
            unit_trade_per_day: value.unit_trade_per_day,
            unit_gold_per_day: value.unit_gold_per_day,
            unit_gold_consumption_per_day: value.unit_gold_consumption_per_day,
            unit_originium_per_day: value.unit_originium_per_day,
            manufacture_base_efficiency_millis: value
                .manufacture_base_efficiency
                .map(Efficiency::millis),
            manufacture_occupancy_efficiency_millis: value
                .manufacture_occupancy_efficiency
                .map(Efficiency::millis),
            manufacture_skill_efficiency_millis: value
                .manufacture_skill_efficiency
                .map(Efficiency::millis),
            manufacture_global_efficiency_millis: value
                .manufacture_global_efficiency
                .map(Efficiency::millis),
            manufacture_final_efficiency_millis: value
                .manufacture_final_efficiency
                .map(Efficiency::millis),
            manufacture_storage_limit: value.manufacture_storage_limit,
            manufacture_unit_output_per_day: value.manufacture_unit_output_per_day,
            manufacture_output_per_day: value.manufacture_output_per_day,
        }
    }
}

impl From<BakedComboTableDisk> for BakedComboTable {
    fn from(value: BakedComboTableDisk) -> Self {
        Self {
            schema_version: value.schema_version,
            generator: value.generator,
            operator_count: value.operator_count,
            operator_names: value.operator_names,
            operator_states: value.operator_states,
            mask_words: value.mask_words,
            indexes: value.indexes,
            rows: value.rows.into_iter().map(BakedComboRow::from).collect(),
        }
    }
}

impl From<BakedComboRowDisk> for BakedComboRow {
    fn from(value: BakedComboRowDisk) -> Self {
        Self {
            row_id: 0,
            facility: String::new(),
            signature_key: String::new(),
            room_level: value.room_level,
            operator_capacity: value.operator_capacity,
            names: Vec::new(),
            operator_indices: value.operator_indices,
            operator_mask: Vec::new(),
            sort_efficiency: Efficiency::from_millis(value.sort_efficiency_millis),
            order_kind: value.order_kind,
            recipe: value.recipe,
            trade_mechanism_signature: value.trade_mechanism_signature,
            trade_base_efficiency: value
                .trade_base_efficiency_millis
                .map(Efficiency::from_millis),
            trade_occupancy_efficiency: value
                .trade_occupancy_efficiency_millis
                .map(Efficiency::from_millis),
            trade_skill_efficiency: value
                .trade_skill_efficiency_millis
                .map(Efficiency::from_millis),
            trade_control_efficiency: value
                .trade_control_efficiency_millis
                .map(Efficiency::from_millis),
            trade_paper_efficiency: value
                .trade_paper_efficiency_millis
                .map(Efficiency::from_millis),
            trade_mechanic_equivalent_efficiency: value
                .trade_mechanic_equivalent_efficiency_millis
                .map(Efficiency::from_millis),
            trade_unit_output_multiplier: value
                .trade_unit_output_multiplier_millis
                .map(Efficiency::from_millis),
            trade_final_efficiency: value
                .trade_final_efficiency_millis
                .map(Efficiency::from_millis),
            trade_equivalent_skill_efficiency: value
                .trade_equivalent_skill_efficiency_millis
                .map(Efficiency::from_millis),
            trade_gold_equivalent_efficiency: value
                .trade_gold_equivalent_efficiency_millis
                .map(Efficiency::from_millis),
            trade_combined_output_value: value
                .trade_combined_output_value_millis
                .map(Efficiency::from_millis),
            trade_total_efficiency: value
                .trade_total_efficiency_millis
                .map(Efficiency::from_millis),
            rule_id: value.rule_id,
            unit_trade_per_day: value.unit_trade_per_day,
            unit_gold_per_day: value.unit_gold_per_day,
            unit_gold_consumption_per_day: value.unit_gold_consumption_per_day,
            unit_originium_per_day: value.unit_originium_per_day,
            manufacture_base_efficiency: value
                .manufacture_base_efficiency_millis
                .map(Efficiency::from_millis),
            manufacture_occupancy_efficiency: value
                .manufacture_occupancy_efficiency_millis
                .map(Efficiency::from_millis),
            manufacture_skill_efficiency: value
                .manufacture_skill_efficiency_millis
                .map(Efficiency::from_millis),
            manufacture_global_efficiency: value
                .manufacture_global_efficiency_millis
                .map(Efficiency::from_millis),
            manufacture_final_efficiency: value
                .manufacture_final_efficiency_millis
                .map(Efficiency::from_millis),
            manufacture_storage_limit: value.manufacture_storage_limit,
            manufacture_unit_output_per_day: value.manufacture_unit_output_per_day,
            manufacture_output_per_day: value.manufacture_output_per_day,
        }
    }
}

#[derive(Debug)]
struct RuntimeBakedComboTable {
    table: BakedComboTable,
    operator_index_by_state: HashMap<OperatorState, usize>,
    /// 每个 signature 实际出现过的干员位图（行 mask 的并集）。
    /// 查询池必须是该集合的子集，否则 Bake 无法覆盖该池的全部组合，必须 miss 回退。
    universe_mask_by_key: HashMap<String, Vec<u64>>,
    /// 贸易四种与制造九种合法 signature；磁盘仍保留原 String key。
    rank_rows_by_signature: [Option<Vec<usize>>; BAKED_SIGNATURE_SLOTS],
    trade_gold_lines: u32,
    trade_canonical: HashMap<usize, usize>,
    manu_canonical: HashMap<usize, usize>,
    trade_atoms: HashMap<usize, Arc<[CompiledAtom]>>,
    manu_atoms: HashMap<usize, Arc<[CompiledAtom]>>,
    /// 持有 Arc 防止地址复用；比较结果仅属于当前加载表的完整状态。
    atom_match_cache: Mutex<HashMap<(bool, usize, usize), (Arc<[CompiledAtom]>, bool)>>,
    skill_source_json: Arc<str>,
    skill_match_by_identity: Mutex<HashMap<u64, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BakedComboIndex {
    signature_key: String,
    start: usize,
    len: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakedComboRow {
    #[serde(default, skip_serializing)]
    row_id: usize,
    #[serde(rename = "f", default, skip_serializing)]
    facility: String,
    #[serde(default, skip_serializing)]
    signature_key: String,
    #[serde(rename = "l")]
    room_level: u8,
    #[serde(rename = "c")]
    operator_capacity: usize,
    #[serde(default, skip_serializing)]
    names: Vec<String>,
    #[serde(rename = "oi")]
    operator_indices: Vec<usize>,
    #[serde(default, skip_serializing)]
    operator_mask: Vec<u64>,
    #[serde(rename = "s")]
    sort_efficiency: Efficiency,
    #[serde(rename = "ok", skip_serializing_if = "Option::is_none")]
    order_kind: Option<TradeOrderKind>,
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    recipe: Option<RecipeKind>,
    #[serde(rename = "tms", skip_serializing_if = "Option::is_none")]
    trade_mechanism_signature: Option<TradeRoomMechanismSignature>,
    #[serde(rename = "tb", skip_serializing_if = "Option::is_none")]
    trade_base_efficiency: Option<Efficiency>,
    #[serde(rename = "to", skip_serializing_if = "Option::is_none")]
    trade_occupancy_efficiency: Option<Efficiency>,
    #[serde(rename = "ts", skip_serializing_if = "Option::is_none")]
    trade_skill_efficiency: Option<Efficiency>,
    #[serde(rename = "tc", skip_serializing_if = "Option::is_none")]
    trade_control_efficiency: Option<Efficiency>,
    #[serde(rename = "tp", skip_serializing_if = "Option::is_none")]
    trade_paper_efficiency: Option<Efficiency>,
    #[serde(rename = "tm", skip_serializing_if = "Option::is_none")]
    trade_mechanic_equivalent_efficiency: Option<Efficiency>,
    #[serde(rename = "tu", skip_serializing_if = "Option::is_none")]
    trade_unit_output_multiplier: Option<Efficiency>,
    #[serde(rename = "tf", skip_serializing_if = "Option::is_none")]
    trade_final_efficiency: Option<Efficiency>,
    #[serde(rename = "te", skip_serializing_if = "Option::is_none")]
    trade_equivalent_skill_efficiency: Option<Efficiency>,
    #[serde(rename = "tt", skip_serializing_if = "Option::is_none")]
    trade_total_efficiency: Option<Efficiency>,
    trade_gold_equivalent_efficiency: Option<Efficiency>,
    trade_combined_output_value: Option<Efficiency>,
    #[serde(rename = "ri", skip_serializing_if = "Option::is_none")]
    rule_id: Option<String>,
    #[serde(rename = "utd", skip_serializing_if = "Option::is_none")]
    unit_trade_per_day: Option<f64>,
    #[serde(rename = "ugd", skip_serializing_if = "Option::is_none")]
    unit_gold_per_day: Option<f64>,
    #[serde(rename = "ugcd", skip_serializing_if = "Option::is_none")]
    unit_gold_consumption_per_day: Option<f64>,
    #[serde(rename = "uod", default, skip_serializing_if = "Option::is_none")]
    unit_originium_per_day: Option<f64>,
    #[serde(rename = "mb", skip_serializing_if = "Option::is_none")]
    manufacture_base_efficiency: Option<Efficiency>,
    #[serde(rename = "mo", skip_serializing_if = "Option::is_none")]
    manufacture_occupancy_efficiency: Option<Efficiency>,
    #[serde(rename = "ms", skip_serializing_if = "Option::is_none")]
    manufacture_skill_efficiency: Option<Efficiency>,
    #[serde(rename = "mg", skip_serializing_if = "Option::is_none")]
    manufacture_global_efficiency: Option<Efficiency>,
    #[serde(rename = "mf", skip_serializing_if = "Option::is_none")]
    manufacture_final_efficiency: Option<Efficiency>,
    #[serde(rename = "msl", default, skip_serializing_if = "Option::is_none")]
    manufacture_storage_limit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manufacture_unit_output_per_day: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manufacture_output_per_day: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
struct TradeRoomMechanismSignature {
    peer_count: Option<u8>,
    glasgow_peer_present: Option<bool>,
    glasgow_count: Option<u8>,
    king_of_victoria_present: Option<bool>,
    snhunt_count: Option<u8>,
    laterano_count: Option<u8>,
}

impl TradeRoomMechanismSignature {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

fn trade_room_mechanism_signature(
    operators: &[TradeOperator],
) -> Option<TradeRoomMechanismSignature> {
    const GLASGOW: &str = "cc.g.glasgow";
    const SNHUNT: &str = "cc.g.snhunt";
    const LATERANO: &str = "cc.g.laterano";

    let observes_tag_count = |target: &str| {
        operators.iter().any(|operator| {
            operator.compiled_atoms.iter().any(|compiled| {
                matches!(
                    compiled.atom.selector.as_ref(),
                    Some(Selector::TaggedCountInRoom { tag }) if tag == target
                )
            })
        })
    };
    let glasgow_peer_present = operators
        .iter()
        .enumerate()
        .filter(|(_, operator)| {
            operator.compiled_atoms.iter().any(|compiled| {
                matches!(
                    compiled.atom.condition.as_ref(),
                    Some(Condition::PeerTagInRoom { tag }) if tag == GLASGOW
                )
            })
        })
        .map(|(observer_index, _)| {
            operators
                .iter()
                .enumerate()
                .any(|(index, operator)| index != observer_index && operator.has_tag(GLASGOW))
        })
        .reduce(|left, right| left || right);
    let observes_peer_count = operators.iter().any(|operator| {
        operator
            .compiled_atoms
            .iter()
            .any(|compiled| matches!(compiled.atom.selector, Some(Selector::RoomPeerCount)))
    });
    let observes_king = operators.iter().any(|operator| {
        operator.compiled_atoms.iter().any(|compiled| {
            matches!(
                compiled.atom.condition.as_ref(),
                Some(Condition::PartnerInRoom { name }) if name == "推进之王"
            )
        })
    });
    let count_tag = |tag: &str| {
        operators
            .iter()
            .filter(|operator| operator.has_tag(tag))
            .count()
            .min(u8::MAX as usize) as u8
    };

    let signature = TradeRoomMechanismSignature {
        peer_count: observes_peer_count
            .then_some(operators.len().saturating_sub(1).min(u8::MAX as usize) as u8),
        glasgow_peer_present,
        glasgow_count: observes_tag_count(GLASGOW).then(|| count_tag(GLASGOW)),
        king_of_victoria_present: observes_king
            .then(|| operators.iter().any(|operator| operator.name == "推进之王")),
        snhunt_count: observes_tag_count(SNHUNT).then(|| count_tag(SNHUNT)),
        laterano_count: observes_tag_count(LATERANO).then(|| count_tag(LATERANO)),
    };
    (!signature.is_empty()).then_some(signature)
}

pub fn bake_catalogs(options: &BakeOptions) -> Result<BakeReport> {
    let start = Instant::now();
    if options.out_dir.join("CURRENT").exists() {
        return Err(Error::msg(
            "use bake_and_publish_catalogs to replace a published catalog",
        ));
    }
    fs::create_dir_all(&options.out_dir)?;

    let instances_path = default_instances_path()?;
    let skill_table_path = default_skill_table_path()?;
    let instances = OperatorInstances::load(&instances_path)?;
    let table = SkillTable::load(&skill_table_path)?;
    let pools = build_variant_pools(&instances, &table)?;
    let mut operators = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for state in pools
        .trade
        .entries
        .iter()
        .map(OperatorState::from_trade_entry)
        .chain(
            pools
                .manufacture
                .entries
                .iter()
                .map(OperatorState::from_manu_entry),
        )
    {
        if seen.insert(state.clone()) {
            operators.push(state);
        }
    }
    let signature_count = expected_signature_count(options);

    emit_progress(
        options,
        BakeProgressEvent::Started {
            out_dir: options.out_dir.clone(),
            operator_count: operators.len(),
            signature_count,
            worker_count: rayon::current_num_threads(),
        },
    );

    emit_progress(
        options,
        BakeProgressEvent::Writing {
            path: options.out_dir.join("operators.json"),
            rows: Some(operators.len()),
        },
    );
    write_json(options.out_dir.join("operators.json"), &operators)?;
    remove_stale_catalogs(&options.out_dir)?;

    let operator_index: HashMap<OperatorState, usize> = operators
        .iter()
        .enumerate()
        .map(|(idx, op)| (op.clone(), idx))
        .collect();
    let mask_words = operators.len().div_ceil(64).max(1);

    let mut trade_signatures = 0usize;
    let mut trade_hits = 0usize;
    let mut trade_mechanism_rows = 0usize;
    let mut trade_mechanism_signatures = 0usize;
    let mut rows = Vec::new();
    if options.include_trade {
        let baked = bake_trade_rows(&pools.trade, &table, options, &operator_index, mask_words)?;
        trade_signatures = baked.signatures;
        trade_hits = baked.rows.len();
        trade_mechanism_rows = baked
            .rows
            .iter()
            .filter(|row| row.trade_mechanism_signature.is_some())
            .count();
        trade_mechanism_signatures = baked
            .rows
            .iter()
            .filter_map(|row| row.trade_mechanism_signature.clone())
            .collect::<BTreeSet<_>>()
            .len();
        rows.extend(baked.rows);
    }

    let mut manufacture_signatures = 0usize;
    let mut manufacture_hits = 0usize;
    if options.include_manufacture {
        let baked = bake_manufacture_rows(
            &pools.manufacture,
            &table,
            options,
            &operator_index,
            mask_words,
        )?;
        manufacture_signatures = baked.signatures;
        manufacture_hits = baked.rows.len();
        rows.extend(baked.rows);
    }

    let combo_table = build_combo_table_from_rows(
        operators.clone(),
        mask_words,
        rows,
        options.generator.clone(),
    );
    let combo_table_rows = combo_table.rows.len();
    emit_progress(
        options,
        BakeProgressEvent::Writing {
            path: options.out_dir.join("combo_table.bin"),
            rows: Some(combo_table_rows),
        },
    );
    let combo_disk = BakedComboTableDisk::from(&combo_table);
    let combo_bytes = bincode::serialize(&combo_disk)
        .map_err(|e| Error::msg(format!("encode combo_table.bin: {e}")))?;
    fs::write(options.out_dir.join("combo_table.bin"), &combo_bytes)?;

    let manifest = BakeManifest {
        schema_version: BAKE_SCHEMA_VERSION,
        generated_by: "infra-core::bake".to_string(),
        model: "binary_single_room_combo_table".to_string(),
        generator: options.generator.clone(),
        inputs: bake_input_fingerprints()?,
        options: BakeManifestOptions {
            include_trade: options.include_trade,
            include_manufacture: options.include_manufacture,
            limit_per_signature: options.limit_per_signature,
            roster_model:
                "all modeled instance facility variants; five representative progress states"
                    .to_string(),
            layout_model: "single room signatures; gold trade keeps gold line count in key"
                .to_string(),
        },
        combo_table: BakeComboTableFingerprint {
            bytes: combo_bytes.len() as u64,
            hash64: hash_bytes(&combo_bytes),
            rows_by_signature: combo_table
                .indexes
                .iter()
                .map(|index| (index.signature_key.clone(), index.len))
                .collect(),
        },
    };
    emit_progress(
        options,
        BakeProgressEvent::Writing {
            path: options.out_dir.join("manifest.json"),
            rows: None,
        },
    );
    write_json(options.out_dir.join("manifest.json"), &manifest)?;

    let report = BakeReport {
        out_dir: options.out_dir.clone(),
        schema_version: BAKE_SCHEMA_VERSION,
        operator_count: operators.len(),
        trade_signatures,
        trade_hits,
        trade_mechanism_rows,
        trade_mechanism_signatures,
        manufacture_signatures,
        manufacture_hits,
        combo_table_rows,
        generator: options.generator.clone(),
        elapsed_ms: start.elapsed().as_millis(),
    };
    emit_progress(
        options,
        BakeProgressEvent::Writing {
            path: options.out_dir.join("summary.json"),
            rows: None,
        },
    );
    write_json(options.out_dir.join("summary.json"), &report)?;
    emit_progress(
        options,
        BakeProgressEvent::Finished {
            combo_table_rows,
            elapsed_ms: start.elapsed().as_millis(),
        },
    );
    Ok(report)
}

/// Generate and fully verify a fresh immutable generation, then atomically
/// publish CURRENT. Never replaces or edits the generation readers are using.
pub fn bake_and_publish_catalogs(options: &BakeOptions) -> Result<BakeReport> {
    publish_baked_catalogs(options, false)
}

struct BakeGeneration {
    path: PathBuf,
    published: bool,
}

impl Drop for BakeGeneration {
    fn drop(&mut self) {
        if !self.published {
            // A failed publication can leave the private generation read-only.
            let _ = set_generation_readonly(&self.path, false);
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct BakePublishLock(PathBuf);

impl Drop for BakePublishLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn set_generation_readonly(path: &Path, readonly: bool) -> Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = permissions.mode();
        permissions.set_mode(if readonly {
            mode & !0o222
        } else {
            mode | 0o200
        });
    }
    #[cfg(not(unix))]
    permissions.set_readonly(readonly);
    // Restore a directory before descending, freeze it after its children.
    if !readonly {
        fs::set_permissions(path, permissions.clone())?;
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            set_generation_readonly(&entry?.path(), readonly)?;
        }
    }
    if readonly {
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn publish_baked_catalogs(options: &BakeOptions, prefer_existing: bool) -> Result<BakeReport> {
    if options.limit_per_signature.is_some() {
        return Err(Error::msg("limited catalogs cannot be published"));
    }
    let generator = options
        .generator
        .as_ref()
        .ok_or_else(|| Error::msg("catalog publication requires generator fingerprint"))?;
    fs::create_dir_all(&options.out_dir)?;
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
    let generation_name = format!(
        "gen-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::msg(error.to_string()))?
            .as_nanos(),
        NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
    );
    let generation_path = options.out_dir.join(&generation_name);
    fs::create_dir(&generation_path)?;
    let mut generation = BakeGeneration {
        path: generation_path.clone(),
        published: false,
    };
    let mut generation_options = options.clone();
    generation_options.out_dir = generation_path;
    let mut report = bake_catalogs(&generation_options)?;
    verify_baked_catalog_responses(&generation_options.out_dir, generator)?;

    let lock_path = options.out_dir.join(".publish-lock");
    if let Err(error) = fs::create_dir(&lock_path) {
        // Never wait behind another publisher on a runtime request.
        if validate_baked_catalog(&options.out_dir, generator).is_ok() {
            report.out_dir = options.out_dir.clone();
            return Ok(report);
        }
        return Err(Error::msg(format!("Bake publication unavailable: {error}")));
    }
    let _lock = BakePublishLock(lock_path);
    if prefer_existing && validate_baked_catalog(&options.out_dir, generator).is_ok() {
        report.out_dir = options.out_dir.clone();
        return Ok(report);
    }
    set_generation_readonly(&generation.path, true)?;
    let pointer_path = options.out_dir.join(format!(".CURRENT-{generation_name}"));
    let publish = || -> Result<()> {
        let mut pointer = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pointer_path)?;
        writeln!(pointer, "{generation_name}")?;
        pointer.sync_all()?;
        drop(pointer);
        fs::rename(&pointer_path, options.out_dir.join("CURRENT"))?;
        Ok(())
    };
    if let Err(error) = publish() {
        let _ = fs::remove_file(&pointer_path);
        return Err(error);
    }
    generation.published = true;
    report.out_dir = options.out_dir.clone();
    Ok(report)
}

fn emit_progress(options: &BakeOptions, event: BakeProgressEvent) {
    if let Some(callback) = &options.progress {
        callback(event);
    }
}

fn expected_signature_count(options: &BakeOptions) -> usize {
    let trade = if options.include_trade { 3 * 2 } else { 0 };
    let manufacture = if options.include_manufacture {
        3 * 3
    } else {
        0
    };
    trade + manufacture
}

fn remove_stale_catalogs(out_dir: &Path) -> Result<()> {
    for name in [
        "trade_combos.json",
        "manufacture_combos.json",
        "combo_table.json",
    ] {
        let path = out_dir.join(name);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

struct BakedRows {
    signatures: usize,
    rows: Vec<BakedComboRow>,
}

fn bake_trade_rows(
    pool: &TradePool,
    table: &SkillTable,
    options: &BakeOptions,
    operator_index: &HashMap<OperatorState, usize>,
    mask_words: usize,
) -> Result<BakedRows> {
    let layout = Arc::new(LayoutContext::search_baseline());
    let gold_lines = if layout.gold_manu_line_count > 0 {
        layout.gold_manu_line_count
    } else {
        TradeSearchOptions::default().gold_production_lines
    };
    let mut all_rows = Vec::new();
    let mut signatures = 0usize;

    for room_level in 1..=3 {
        for order_kind in [TradeOrderKind::Gold, TradeOrderKind::Originium] {
            let operator_capacity = station_operator_capacity(room_level);
            let signature_key =
                trade_lookup_key(room_level, operator_capacity, order_kind, gold_lines);
            let combo_count = n_choose_k_u64(pool.entries.len(), operator_capacity);
            let sig_start = Instant::now();
            emit_progress(
                options,
                BakeProgressEvent::SignatureStarted {
                    facility: "trade",
                    signature_key: signature_key.clone(),
                    combo_count,
                },
            );
            let row_options = TradeSearchOptions {
                trade_level: room_level,
                operator_capacity,
                mood: 24.0,
                top_k: 1,
                gold_production_lines: gold_lines,
                layout: Arc::clone(&layout),
                shift_hours: 24.0,
                order_mode: TradeSearchOrderMode::Single(order_kind),
                bake_mode: crate::bake::BakeMode::Disabled,
                full_pool: false,
            };

            let mut rows: Vec<_> = combinations_indices(pool.entries.len(), operator_capacity)
                .collect::<Vec<_>>()
                .par_iter()
                .filter_map(|combo| {
                    trade_combo_row(
                        &pool,
                        table,
                        &row_options,
                        order_kind,
                        combo,
                        &signature_key,
                        operator_index,
                        mask_words,
                    )
                })
                .collect();
            sort_signature_rows(&mut rows);
            if let Some(limit) = options.limit_per_signature {
                rows.truncate(limit);
            }
            emit_progress(
                options,
                BakeProgressEvent::SignatureFinished {
                    facility: "trade",
                    signature_key,
                    rows: rows.len(),
                    elapsed_ms: sig_start.elapsed().as_millis(),
                },
            );
            signatures += 1;
            all_rows.extend(rows);
        }
    }

    Ok(BakedRows {
        signatures,
        rows: all_rows,
    })
}

#[allow(clippy::too_many_arguments)]
fn trade_combo_row(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    combo: &[usize],
    signature_key: &str,
    operator_index: &HashMap<OperatorState, usize>,
    mask_words: usize,
) -> Option<BakedComboRow> {
    if !distinct_names(combo.iter().map(|idx| pool.entries[*idx].name.as_str())) {
        return None;
    }
    let (hit, result) = options.solve_trade_combo(pool, table, order_kind, combo, None, None)?;
    let operators = build_trade_combo_operators_vec(pool, combo, None, None);
    let trade_mechanism_signature = trade_room_mechanism_signature(&operators);
    let indices = state_indices(operators.iter().map(trade_operator_state), operator_index).ok()?;
    Some(trade_row_from_hit(
        &hit,
        result.production.unit.gold_consumption_per_day,
        "trade",
        signature_key,
        options.trade_level,
        options.operator_capacity,
        Some(order_kind),
        None,
        trade_mechanism_signature,
        &indices,
        mask_words,
    ))
}

/// 把 live 命中结构完整落进行里；Bake 行级对账用同一函数重建期望行。
#[allow(clippy::too_many_arguments)]
fn trade_row_from_hit(
    hit: &TradeSearchHit,
    unit_gold_consumption_per_day: f64,
    facility: &str,
    signature_key: &str,
    room_level: u8,
    operator_capacity: usize,
    order_kind: Option<TradeOrderKind>,
    recipe: Option<RecipeKind>,
    trade_mechanism_signature: Option<TradeRoomMechanismSignature>,
    operator_indices: &[usize],
    mask_words: usize,
) -> BakedComboRow {
    let breakdown = hit
        .breakdown
        .as_ref()
        .expect("live trade hit always carries a breakdown");
    let operator_mask = operator_mask_from_indices(operator_indices, mask_words);
    BakedComboRow {
        row_id: 0,
        facility: facility.to_string(),
        signature_key: signature_key.to_string(),
        room_level,
        operator_capacity,
        names: hit.names.clone(),
        operator_indices: operator_indices.to_vec(),
        operator_mask,
        sort_efficiency: hit.final_efficiency,
        order_kind,
        recipe,
        trade_mechanism_signature,
        trade_base_efficiency: Some(breakdown.base_efficiency),
        trade_occupancy_efficiency: Some(breakdown.occupancy_efficiency),
        trade_skill_efficiency: Some(breakdown.skill_efficiency),
        trade_control_efficiency: Some(breakdown.control_efficiency),
        trade_paper_efficiency: Some(breakdown.paper_efficiency),
        trade_mechanic_equivalent_efficiency: Some(breakdown.mechanic_equivalent_efficiency),
        trade_unit_output_multiplier: Some(breakdown.unit_output_multiplier),
        trade_final_efficiency: Some(breakdown.final_efficiency),
        trade_equivalent_skill_efficiency: Some(breakdown.equivalent_skill_efficiency),
        trade_gold_equivalent_efficiency: Some(breakdown.gold_equivalent_efficiency),
        trade_combined_output_value: Some(breakdown.combined_output_value),
        trade_total_efficiency: Some(breakdown.total_efficiency),
        rule_id: hit.rule_id.clone(),
        unit_trade_per_day: Some(hit.unit_trade_per_day),
        unit_gold_per_day: Some(hit.unit_gold_per_day),
        unit_gold_consumption_per_day: Some(unit_gold_consumption_per_day),
        unit_originium_per_day: Some(hit.unit_originium_per_day),
        manufacture_base_efficiency: None,
        manufacture_occupancy_efficiency: None,
        manufacture_skill_efficiency: None,
        manufacture_global_efficiency: None,
        manufacture_final_efficiency: None,
        manufacture_storage_limit: None,
        manufacture_unit_output_per_day: None,
        manufacture_output_per_day: None,
    }
}

fn bake_manufacture_rows(
    pool: &ManuPool,
    table: &SkillTable,
    options: &BakeOptions,
    operator_index: &HashMap<OperatorState, usize>,
    mask_words: usize,
) -> Result<BakedRows> {
    let layout = Arc::new(LayoutContext::search_baseline());
    let mut all_rows = Vec::new();
    let mut signatures = 0usize;

    for room_level in 1..=3 {
        for recipe in [
            RecipeKind::Gold,
            RecipeKind::BattleRecord,
            RecipeKind::Originium,
        ] {
            let operator_capacity = station_operator_capacity(room_level);
            let signature_key = manufacture_lookup_key(room_level, operator_capacity, recipe);
            let combo_count = n_choose_k_u64(pool.entries.len(), operator_capacity);
            let sig_start = Instant::now();
            emit_progress(
                options,
                BakeProgressEvent::SignatureStarted {
                    facility: "manufacture",
                    signature_key: signature_key.clone(),
                    combo_count,
                },
            );
            let row_options = ManuSearchOptions {
                level: room_level,
                operator_capacity,
                recipe_mode: crate::manufacture::input::ManuSearchRecipeMode::Single(recipe),
                mood: 24.0,
                top_k: 1,
                layout: Arc::clone(&layout),
                shift_hours: None,
                must_include_name: None,
                bake_mode: crate::bake::BakeMode::Disabled,
                full_pool: false,
            };

            let mut rows: Vec<_> = combinations_indices(pool.entries.len(), operator_capacity)
                .collect::<Vec<_>>()
                .par_iter()
                .filter_map(|combo| {
                    manufacture_combo_row(
                        &pool,
                        table,
                        &row_options,
                        recipe,
                        combo,
                        &signature_key,
                        operator_index,
                        mask_words,
                    )
                })
                .collect();
            sort_signature_rows(&mut rows);
            if let Some(limit) = options.limit_per_signature {
                rows.truncate(limit);
            }
            emit_progress(
                options,
                BakeProgressEvent::SignatureFinished {
                    facility: "manufacture",
                    signature_key,
                    rows: rows.len(),
                    elapsed_ms: sig_start.elapsed().as_millis(),
                },
            );
            signatures += 1;
            all_rows.extend(rows);
        }
    }

    Ok(BakedRows {
        signatures,
        rows: all_rows,
    })
}

#[allow(clippy::too_many_arguments)]
fn manufacture_combo_row(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
    combo: &[usize],
    signature_key: &str,
    operator_index: &HashMap<OperatorState, usize>,
    mask_words: usize,
) -> Option<BakedComboRow> {
    if !distinct_names(combo.iter().map(|idx| pool.entries[*idx].name.as_str())) {
        return None;
    }
    let hit = options.solve_manufacture_combo(pool, table, recipe, combo)?;
    let indices = state_indices(
        combo
            .iter()
            .map(|idx| OperatorState::from_manu_entry(&pool.entries[*idx])),
        operator_index,
    )
    .ok()?;
    Some(manufacture_row_from_hit(
        &hit,
        "manufacture",
        signature_key,
        options.level,
        options.operator_capacity,
        recipe,
        &indices,
        mask_words,
    ))
}

/// 把 live 命中结构完整落进行里；Bake 行级对账用同一函数重建期望行。
#[allow(clippy::too_many_arguments)]
fn manufacture_row_from_hit(
    hit: &ManuSearchHit,
    facility: &str,
    signature_key: &str,
    room_level: u8,
    operator_capacity: usize,
    recipe: RecipeKind,
    operator_indices: &[usize],
    mask_words: usize,
) -> BakedComboRow {
    let operator_mask = operator_mask_from_indices(operator_indices, mask_words);
    BakedComboRow {
        row_id: 0,
        facility: facility.to_string(),
        signature_key: signature_key.to_string(),
        room_level,
        operator_capacity,
        names: hit.names.clone(),
        operator_indices: operator_indices.to_vec(),
        operator_mask,
        sort_efficiency: hit.final_efficiency,
        order_kind: None,
        recipe: Some(recipe),
        trade_mechanism_signature: None,
        trade_base_efficiency: None,
        trade_occupancy_efficiency: None,
        trade_skill_efficiency: None,
        trade_control_efficiency: None,
        trade_paper_efficiency: None,
        trade_mechanic_equivalent_efficiency: None,
        trade_unit_output_multiplier: None,
        trade_final_efficiency: None,
        trade_equivalent_skill_efficiency: None,
        trade_gold_equivalent_efficiency: None,
        trade_combined_output_value: None,
        trade_total_efficiency: None,
        rule_id: None,
        unit_trade_per_day: None,
        unit_gold_per_day: None,
        unit_gold_consumption_per_day: None,
        unit_originium_per_day: None,
        manufacture_base_efficiency: Some(hit.breakdown.base_efficiency),
        manufacture_occupancy_efficiency: Some(hit.breakdown.occupancy_efficiency),
        manufacture_skill_efficiency: Some(hit.breakdown.skill_efficiency),
        manufacture_global_efficiency: Some(hit.breakdown.global_efficiency),
        manufacture_final_efficiency: Some(hit.breakdown.final_efficiency),
        manufacture_storage_limit: Some(hit.breakdown.storage_limit),
        manufacture_unit_output_per_day: Some(hit.breakdown.unit_output_per_day),
        manufacture_output_per_day: Some(hit.breakdown.output_per_day),
    }
}

fn sort_signature_rows(rows: &mut [BakedComboRow]) {
    rows.sort_by(compare_baked_rows);
}

fn compare_baked_rows(a: &BakedComboRow, b: &BakedComboRow) -> std::cmp::Ordering {
    b.sort_efficiency
        .cmp(&a.sort_efficiency)
        .then_with(|| {
            b.manufacture_storage_limit
                .unwrap_or_default()
                .cmp(&a.manufacture_storage_limit.unwrap_or_default())
        })
        .then_with(|| a.operator_indices.cmp(&b.operator_indices))
}

pub fn validate_baked_catalog(out_dir: &Path, generator: &BakeGeneratorFingerprint) -> Result<()> {
    load_validated_baked_catalog(out_dir, generator).map(|_| ())
}

struct ValidatedBakedCatalog {
    disk: BakedComboTableDisk,
    trade_pool: Option<TradePool>,
    trade_skill_table: Option<SkillTable>,
}

fn load_validated_baked_catalog(
    out_dir: &Path,
    generator: &BakeGeneratorFingerprint,
) -> Result<ValidatedBakedCatalog> {
    let resolved_dir = resolved_baked_catalog_dir(out_dir)?;
    let out_dir = resolved_dir.as_path();
    let manifest_path = out_dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)?;
    let manifest: BakeManifest = serde_json::from_str(&raw)?;
    if manifest.schema_version != BAKE_SCHEMA_VERSION {
        return Err(Error::msg(format!(
            "baked schema mismatch: found {}, expected {}; rerun `infra-cli bake --out {}`",
            manifest.schema_version,
            BAKE_SCHEMA_VERSION,
            out_dir.display()
        )));
    }
    if manifest.options.limit_per_signature.is_some() {
        return Err(Error::msg(
            "limited baked catalog is not complete enough for runtime publication",
        ));
    }
    if !manifest.options.include_trade && !manifest.options.include_manufacture {
        return Err(Error::msg("baked catalog contains no facility"));
    }
    let Some(baked_generator) = manifest.generator.as_ref() else {
        return Err(Error::msg(format!(
            "baked manifest has no generator fingerprint; rerun `infra-cli bake --out {}`",
            out_dir.display()
        )));
    };
    if baked_generator.hash64 != generator.hash64
        || baked_generator.bytes != generator.bytes
        || baked_generator.kind != generator.kind
    {
        return Err(Error::msg(format!(
            "baked generator mismatch: baked hash={} bytes={}, current hash={} bytes={}; rerun `infra-cli bake --out {}`",
            baked_generator.hash64,
            baked_generator.bytes,
            generator.hash64,
            generator.bytes,
            out_dir.display()
        )));
    }
    validate_baked_input_fingerprints(&manifest, out_dir)?;
    let combo_path = out_dir.join("combo_table.bin");
    let raw = fs::read(&combo_path)
        .map_err(|e| Error::msg(format!("read {}: {e}", combo_path.display())))?;
    if manifest.combo_table.bytes != raw.len() as u64
        || manifest.combo_table.hash64 != hash_bytes(&raw)
    {
        return Err(Error::msg("baked combo table fingerprint mismatch"));
    }
    let disk: BakedComboTableDisk = bincode::deserialize(&raw)
        .map_err(|e| Error::msg(format!("read {}: {e}", combo_path.display())))?;
    validate_baked_combo_table(&disk, generator, &manifest.options)?;
    let rows_by_signature = disk
        .indexes
        .iter()
        .map(|index| (index.signature_key.clone(), index.len))
        .collect::<BTreeMap<_, _>>();
    if rows_by_signature != manifest.combo_table.rows_by_signature {
        return Err(Error::msg("baked combo table row counts mismatch"));
    }
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let pools = build_variant_pools(&instances, &table)?;
    let mut expected_states = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for state in pools
        .trade
        .entries
        .iter()
        .map(OperatorState::from_trade_entry)
        .chain(
            pools
                .manufacture
                .entries
                .iter()
                .map(OperatorState::from_manu_entry),
        )
    {
        if seen.insert(state.clone()) {
            expected_states.push(state);
        }
    }
    if disk.operator_states != expected_states {
        return Err(Error::msg(
            "baked operator states differ from canonical live variants",
        ));
    }
    if manifest.options.include_trade {
        let state_index = pool_state_index(&pools.trade, OperatorState::from_trade_entry);
        validate_trade_mechanism_signatures(&disk, &pools.trade, &state_index)?;
        validate_complete_trade_row_universe(&disk, &pools.trade, &table)?;
    }
    if manifest.options.include_manufacture {
        validate_complete_manufacture_row_universe(&disk, &pools.manufacture, &table)?;
    }
    let (trade_pool, trade_skill_table) = if manifest.options.include_trade {
        (Some(pools.trade), Some(table))
    } else {
        (None, None)
    };
    Ok(ValidatedBakedCatalog {
        disk,
        trade_pool,
        trade_skill_table,
    })
}

/// `bake verify` 的统计结果：行级全量对账与查询级差分的检查/不一致计数。
#[derive(Debug, Clone, Copy, Default)]
pub struct BakeVerifyReport {
    pub rows_checked: usize,
    pub row_mismatches: usize,
    pub queries_checked: usize,
    pub query_mismatches: usize,
    pub skipped_signatures: usize,
    pub skipped_queries: usize,
}

/// 每个 signature 的确定性查询样本数。
pub const BAKE_VERIFY_QUERIES_PER_SIGNATURE: usize = 320;
const BAKE_VERIFY_MAX_REPORTED_MISMATCHES: usize = 20;
const BAKE_VERIFY_TOP_K: [usize; 5] = [1, 2, 3, 5, 8];

/// 固定种子的确定性伪随机数（SplitMix64），避免依赖平台随机源。
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn seeded_rng(key: &str) -> SplitMix64 {
    let mut seed = 0x243F_6A88_85A3_08D3u64;
    for byte in key.bytes() {
        seed = seed.rotate_left(5) ^ u64::from(byte);
        seed = seed.wrapping_mul(0x0000_0100_0000_01B3);
    }
    SplitMix64::new(seed)
}

/// 按真人抽样，再选一个状态；返回原池升序下标，每个姓名最多一次。
fn sample_variant_subset<T: HasName>(
    rng: &mut SplitMix64,
    pool: &PoolCore<T>,
    size: usize,
) -> Vec<usize> {
    let mut groups = BTreeMap::<&str, Vec<usize>>::new();
    for (idx, entry) in pool.entries.iter().enumerate() {
        groups.entry(entry.pool_name()).or_default().push(idx);
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    let count = size.min(groups.len());
    let mut chosen = Vec::with_capacity(count);
    for slot in 0..count {
        let pick = slot + (rng.next_u64() as usize) % (groups.len() - slot);
        groups.swap(slot, pick);
        let group = &groups[slot];
        chosen.push(group[(rng.next_u64() as usize) % group.len()]);
    }
    chosen.sort_unstable();
    chosen
}

fn distinct_pool_name_count<T: HasName>(pool: &PoolCore<T>) -> usize {
    pool.entries
        .iter()
        .map(HasName::pool_name)
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn sample_query_size(
    rng: &mut SplitMix64,
    universe_len: usize,
    capacity: usize,
    query_index: usize,
) -> usize {
    let range = universe_len - capacity + 1;
    match query_index {
        0 => universe_len,
        1 => capacity,
        _ => capacity + (rng.next_u64() as usize) % range,
    }
}

/// 把两个 JSON 值的差异路径写进 `problems`（最多保留 8 条）。
fn json_diff_paths(
    left: &serde_json::Value,
    right: &serde_json::Value,
    prefix: &str,
    problems: &mut Vec<String>,
) {
    if left == right || problems.len() >= 8 {
        return;
    }
    match (left, right) {
        (serde_json::Value::Object(left_map), serde_json::Value::Object(right_map)) => {
            let mut keys: Vec<&str> = left_map.keys().map(String::as_str).collect();
            keys.extend(
                right_map
                    .keys()
                    .map(String::as_str)
                    .filter(|key| !left_map.contains_key(*key)),
            );
            keys.sort_unstable();
            for key in keys {
                let path = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                match (left_map.get(key), right_map.get(key)) {
                    (Some(left_value), Some(right_value)) => {
                        json_diff_paths(left_value, right_value, &path, problems);
                    }
                    _ => problems.push(path),
                }
            }
        }
        (serde_json::Value::Array(left_items), serde_json::Value::Array(right_items)) => {
            for idx in 0..left_items.len().max(right_items.len()) {
                let path = format!("{prefix}[{idx}]");
                match (left_items.get(idx), right_items.get(idx)) {
                    (Some(left_value), Some(right_value)) => {
                        json_diff_paths(left_value, right_value, &path, problems);
                    }
                    _ => problems.push(path),
                }
            }
        }
        _ => problems.push(if prefix.is_empty() {
            "<value>".to_string()
        } else {
            prefix.to_string()
        }),
    }
}

fn push_value_diff<T: Serialize>(live: &T, bake: &T, prefix: &str, problems: &mut Vec<String>) {
    match (serde_json::to_value(live), serde_json::to_value(bake)) {
        (Ok(live_value), Ok(bake_value)) => {
            json_diff_paths(&live_value, &bake_value, prefix, problems);
        }
        _ => problems.push(format!("{prefix}: serialize failure")),
    }
}

fn baked_signature_state_covered(
    table: &RuntimeBakedComboTable,
    key: &str,
    state: &OperatorState,
) -> bool {
    let Some(universe) = table.universe_mask_by_key.get(key) else {
        return false;
    };
    let Some(idx) = table.operator_index_by_state.get(state).copied() else {
        return false;
    };
    universe
        .get(idx / 64)
        .is_some_and(|word| word & (1u64 << (idx % 64)) != 0)
}

fn filter_pool_to_signature_universe<T: Clone>(
    pool: PoolCore<T>,
    table: &RuntimeBakedComboTable,
    key: &str,
    identity: fn(&T) -> OperatorState,
) -> PoolCore<T> {
    PoolCore {
        entries: pool
            .entries
            .into_iter()
            .filter(|entry| baked_signature_state_covered(table, key, &identity(entry)))
            .collect(),
        skipped: pool.skipped,
    }
}

enum RowVerifyContext {
    Trade {
        pool: TradePool,
        state_index: HashMap<OperatorState, usize>,
        options: TradeSearchOptions,
    },
    Manufacture {
        pool: ManuPool,
        state_index: HashMap<OperatorState, usize>,
        options: ManuSearchOptions,
    },
}

fn runtime_row_for_verify(
    actual: &BakedComboRowDisk,
    row_id: usize,
    signature_key: &str,
    names: &[&str],
    mask_words: usize,
) -> BakedComboRow {
    let mut row = BakedComboRow::from(actual.clone());
    row.row_id = row_id;
    row.facility = row_facility_from_signature(signature_key).to_string();
    row.signature_key = signature_key.to_string();
    row.names = names.iter().map(|name| (*name).to_string()).collect();
    row.operator_mask = operator_mask_from_indices(&row.operator_indices, mask_words);
    row
}

fn verify_baked_row(
    actual: &BakedComboRowDisk,
    row_id: usize,
    signature_key: &str,
    disk: &BakedComboTableDisk,
    context: &RowVerifyContext,
    table: &SkillTable,
    operator_index: &HashMap<OperatorState, usize>,
) -> Option<String> {
    let names: Vec<&str> = actual
        .operator_indices
        .iter()
        .map(|operator| disk.operator_names[*operator].as_str())
        .collect();
    match context {
        RowVerifyContext::Trade {
            pool,
            state_index,
            options,
        } => {
            let Some(order) = actual.order_kind else {
                return Some(format!("row {row_id} {signature_key}: missing order kind"));
            };
            let combo = match row_pool_indices(actual, disk, state_index) {
                Ok(combo) => combo,
                Err(error) => return Some(format!("row {row_id} {signature_key}: {error}")),
            };
            let Some((hit, result)) =
                options.solve_trade_combo(pool, table, order, &combo, None, None)
            else {
                return Some(format!(
                    "row {row_id} {signature_key}: live solver rejected the baked combo"
                ));
            };
            let operators = build_trade_combo_operators_vec(pool, &combo, None, None);
            let mechanism_signature = trade_room_mechanism_signature(&operators);
            let indices =
                match state_indices(operators.iter().map(trade_operator_state), operator_index) {
                    Ok(indices) => indices,
                    Err(error) => return Some(format!("row {row_id} {signature_key}: {error}")),
                };
            let expected = trade_row_from_hit(
                &hit,
                result.production.unit.gold_consumption_per_day,
                "trade",
                signature_key,
                actual.room_level,
                actual.operator_capacity,
                Some(order),
                None,
                mechanism_signature,
                &indices,
                disk.mask_words,
            );
            if let Some(failure) = verify_stored_row(&expected, actual, row_id, signature_key) {
                return Some(failure);
            }
            let runtime_row =
                runtime_row_for_verify(actual, row_id, signature_key, &names, disk.mask_words);
            let Some(baked_hit) = row_to_trade_hit(&runtime_row) else {
                return Some(format!(
                    "row {row_id} {signature_key}: baked row has incomplete response"
                ));
            };
            verify_hit_values(&hit, &baked_hit, row_id, signature_key)
        }
        RowVerifyContext::Manufacture {
            pool,
            state_index,
            options,
        } => {
            let Some(recipe) = actual.recipe else {
                return Some(format!("row {row_id} {signature_key}: missing recipe"));
            };
            let combo = match row_pool_indices(actual, disk, state_index) {
                Ok(combo) => combo,
                Err(error) => return Some(format!("row {row_id} {signature_key}: {error}")),
            };
            let Some(hit) = options.solve_manufacture_combo(pool, table, recipe, &combo) else {
                return Some(format!(
                    "row {row_id} {signature_key}: live solver rejected the baked combo"
                ));
            };
            let indices = match state_indices(
                combo
                    .iter()
                    .map(|idx| OperatorState::from_manu_entry(&pool.entries[*idx])),
                operator_index,
            ) {
                Ok(indices) => indices,
                Err(error) => return Some(format!("row {row_id} {signature_key}: {error}")),
            };
            let expected = manufacture_row_from_hit(
                &hit,
                "manufacture",
                signature_key,
                actual.room_level,
                actual.operator_capacity,
                recipe,
                &indices,
                disk.mask_words,
            );
            if let Some(failure) = verify_stored_row(&expected, actual, row_id, signature_key) {
                return Some(failure);
            }
            let runtime_row =
                runtime_row_for_verify(actual, row_id, signature_key, &names, disk.mask_words);
            let Some(baked_hit) = row_to_manu_hit(&runtime_row) else {
                return Some(format!(
                    "row {row_id} {signature_key}: baked row has incomplete response"
                ));
            };
            verify_hit_values(&hit, &baked_hit, row_id, signature_key)
        }
    }
}

fn verify_stored_row(
    expected: &BakedComboRow,
    actual: &BakedComboRowDisk,
    row_id: usize,
    signature_key: &str,
) -> Option<String> {
    let expected_disk = BakedComboRowDisk::from(expected);
    if expected_disk == *actual {
        return None;
    }
    let mut problems = Vec::new();
    match (
        serde_json::to_value(&expected_disk),
        serde_json::to_value(actual),
    ) {
        (Ok(left), Ok(right)) => json_diff_paths(&left, &right, "", &mut problems),
        _ => problems.push("<stored row serialization>".to_string()),
    }
    Some(format!(
        "row {row_id} {signature_key}: stored row differs at {}",
        problems.join(", ")
    ))
}

fn verify_hit_values<T: Serialize>(
    live: &T,
    bake: &T,
    row_id: usize,
    signature_key: &str,
) -> Option<String> {
    let live_value = match serde_json::to_value(live) {
        Ok(value) => value,
        Err(error) => {
            return Some(format!(
                "row {row_id} {signature_key}: serialize live hit: {error}"
            ))
        }
    };
    let bake_value = match serde_json::to_value(bake) {
        Ok(value) => value,
        Err(error) => {
            return Some(format!(
                "row {row_id} {signature_key}: serialize baked hit: {error}"
            ))
        }
    };
    if live_value == bake_value {
        return None;
    }
    let mut problems = Vec::new();
    json_diff_paths(&live_value, &bake_value, "", &mut problems);
    Some(format!(
        "row {row_id} {signature_key}: hit differs at {}",
        problems.join(", ")
    ))
}

pub fn verify_baked_catalog_responses(
    out_dir: &Path,
    generator: &BakeGeneratorFingerprint,
) -> Result<BakeVerifyReport> {
    let validated = load_validated_baked_catalog(out_dir, generator)?;
    let disk = validated.disk;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let pools = build_variant_pools(&instances, &table)?;
    let trade_pool = pools.trade;
    let manufacture_pool = pools.manufacture;
    let operator_index: HashMap<OperatorState, usize> = disk
        .operator_states
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, state)| (state, index))
        .collect();
    let trade_state_index = pool_state_index(&trade_pool, OperatorState::from_trade_entry);
    let manu_state_index = pool_state_index(&manufacture_pool, OperatorState::from_manu_entry);
    let layout = Arc::new(LayoutContext::search_baseline());
    let runtime_table = runtime_baked_table_from_disk(disk.clone())?;
    let gold_lines = if layout.gold_manu_line_count > 0 {
        layout.gold_manu_line_count
    } else {
        TradeSearchOptions::default().gold_production_lines
    };
    let mut report = BakeVerifyReport::default();
    let mut failures: Vec<String> = Vec::new();

    // ---- 行级全量对账 ----
    for index in &disk.indexes {
        let rows = &disk.rows[index.start..index.start + index.len];
        report.rows_checked += rows.len();
        let Some(first) = rows.first() else {
            report.skipped_signatures += 1;
            continue;
        };
        let context = if let Some(order) = first.order_kind {
            RowVerifyContext::Trade {
                pool: trade_pool.clone(),
                state_index: trade_state_index.clone(),
                options: TradeSearchOptions {
                    trade_level: first.room_level,
                    operator_capacity: first.operator_capacity,
                    mood: 24.0,
                    top_k: 1,
                    gold_production_lines: gold_lines,
                    layout: Arc::clone(&layout),
                    shift_hours: 24.0,
                    order_mode: TradeSearchOrderMode::Single(order),
                    bake_mode: BakeMode::Disabled,
                    full_pool: false,
                },
            }
        } else if let Some(recipe) = first.recipe {
            RowVerifyContext::Manufacture {
                pool: manufacture_pool.clone(),
                state_index: manu_state_index.clone(),
                options: ManuSearchOptions {
                    level: first.room_level,
                    operator_capacity: first.operator_capacity,
                    recipe_mode: crate::manufacture::input::ManuSearchRecipeMode::Single(recipe),
                    mood: 24.0,
                    top_k: 1,
                    layout: Arc::clone(&layout),
                    shift_hours: None,
                    must_include_name: None,
                    bake_mode: BakeMode::Disabled,
                    full_pool: false,
                },
            }
        } else {
            report.skipped_signatures += 1;
            continue;
        };
        let outcomes: Vec<Option<String>> = rows
            .par_iter()
            .enumerate()
            .map(|(offset, actual)| {
                verify_baked_row(
                    actual,
                    index.start + offset,
                    &index.signature_key,
                    &disk,
                    &context,
                    &table,
                    &operator_index,
                )
            })
            .collect();
        for failure in outcomes.into_iter().flatten() {
            report.row_mismatches += 1;
            if failures.len() < BAKE_VERIFY_MAX_REPORTED_MISMATCHES {
                failures.push(failure);
            }
        }
    }

    // ---- 查询级差分 ----
    for index in &disk.indexes {
        if index.len == 0 {
            report.skipped_signatures += 1;
            continue;
        }
        let first = &disk.rows[index.start];
        if let Some(order) = first.order_kind {
            let capacity = first.operator_capacity;
            let level = first.room_level;
            let standalone_universe = filter_standalone_exact_with(
                &trade_pool,
                FacilityKind::TradePost,
                StandaloneFilter::for_order(order),
            )
            .unwrap_or_else(|| trade_pool.clone());
            let standalone_universe = filter_pool_to_signature_universe(
                standalone_universe,
                &runtime_table,
                &index.signature_key,
                OperatorState::from_trade_entry,
            );
            if distinct_pool_name_count(&standalone_universe) < capacity {
                report.skipped_signatures += 1;
                continue;
            }
            // 默认路径：生产查表实际收到的是 standalone 过滤后的池。
            run_trade_query_samples(
                &index.signature_key,
                level,
                capacity,
                order,
                &standalone_universe,
                false,
                &table,
                &layout,
                &runtime_table,
                gold_lines,
                &mut report,
                &mut failures,
            );
            // full_pool 路径：池就是调用方给的完整池，覆盖烘焙生成池的大小。
            let full_universe = filter_pool_to_signature_universe(
                trade_pool.clone(),
                &runtime_table,
                &index.signature_key,
                OperatorState::from_trade_entry,
            );
            if distinct_pool_name_count(&full_universe) >= capacity {
                run_trade_query_samples(
                    &index.signature_key,
                    level,
                    capacity,
                    order,
                    &full_universe,
                    true,
                    &table,
                    &layout,
                    &runtime_table,
                    gold_lines,
                    &mut report,
                    &mut failures,
                );
            }
        } else if let Some(recipe) = first.recipe {
            let universe = filter_general_manufacture_search_pool(&manufacture_pool);
            let universe = filter_pool_to_signature_universe(
                universe,
                &runtime_table,
                &index.signature_key,
                OperatorState::from_manu_entry,
            );
            let capacity = first.operator_capacity;
            if distinct_pool_name_count(&universe) < capacity {
                report.skipped_signatures += 1;
                continue;
            }
            let level = first.room_level;
            let mut rng = seeded_rng(&index.signature_key);
            let standalone_universe = filter_standalone_exact_with(
                &universe,
                FacilityKind::Factory,
                StandaloneFilter::for_recipe(recipe),
            )
            .unwrap_or_else(|| universe.clone());
            for query_index in 0..BAKE_VERIFY_QUERIES_PER_SIGNATURE {
                let full_pool = query_index % 2 == 1;
                let sampled_universe = if full_pool {
                    &universe
                } else {
                    &standalone_universe
                };
                if distinct_pool_name_count(sampled_universe) < capacity {
                    report.skipped_queries += 1;
                    continue;
                }
                let size = sample_query_size(
                    &mut rng,
                    distinct_pool_name_count(sampled_universe),
                    capacity,
                    query_index,
                );
                let chosen = sample_variant_subset(&mut rng, sampled_universe, size);
                let pool = ManuPool {
                    entries: chosen
                        .iter()
                        .map(|&position| sampled_universe.entries[position].clone())
                        .collect(),
                    skipped: Vec::new(),
                };
                if pool.entries.len() < capacity {
                    report.skipped_queries += 1;
                    continue;
                }
                let top_k = BAKE_VERIFY_TOP_K[query_index % BAKE_VERIFY_TOP_K.len()];
                let must_name = if query_index % 7 == 6 {
                    let position = (rng.next_u64() as usize) % pool.entries.len();
                    Some(pool.entries[position].name.clone())
                } else {
                    None
                };
                let live_options = ManuSearchOptions {
                    level,
                    operator_capacity: capacity,
                    recipe_mode: crate::manufacture::input::ManuSearchRecipeMode::Single(recipe),
                    mood: [24.0, 12.0, 3.0][query_index % 3],
                    top_k,
                    layout: Arc::clone(&layout),
                    shift_hours: [None, Some(12.0), Some(36.0), Some(0.0)][query_index % 4],
                    must_include_name: must_name,
                    bake_mode: BakeMode::Disabled,
                    full_pool,
                };
                let mut bake_options = live_options;
                bake_options.bake_mode = BakeMode::Required;
                if !baked_manufacture_compatible(&pool, &bake_options, &table)
                    || canonical_manu_query_ids(&runtime_table, &pool).is_err()
                {
                    report.skipped_queries += 1;
                    continue;
                }
                let mut live_options = bake_options.clone();
                live_options.bake_mode = BakeMode::Disabled;
                let live = crate::search::search_manufacture_triples(&pool, &table, &live_options);
                let bake = baked_manufacture_report(
                    &runtime_table,
                    &pool,
                    &bake_options,
                    recipe,
                    if bake_options.must_include_name.is_some() {
                        n_choose_k_u64(pool.entries.len() - 1, capacity - 1)
                    } else {
                        n_choose_k_u64(pool.entries.len(), capacity)
                    },
                    StdInstant::now(),
                );
                report.queries_checked += 1;
                let failure = compare_manufacture_query(
                    live,
                    bake,
                    query_index,
                    &index.signature_key,
                    size,
                    top_k,
                );
                if let Some(failure) = failure {
                    report.query_mismatches += 1;
                    if failures.len() < BAKE_VERIFY_MAX_REPORTED_MISMATCHES {
                        failures.push(failure);
                    }
                }
            }
        } else {
            report.skipped_signatures += 1;
        }
    }

    if report.row_mismatches > 0 || report.query_mismatches > 0 {
        let mut message = format!(
            "bake verify failed: rows_checked={} row_mismatches={} queries_checked={} query_mismatches={} skipped_signatures={} skipped_queries={}",
            report.rows_checked,
            report.row_mismatches,
            report.queries_checked,
            report.query_mismatches,
            report.skipped_signatures,
            report.skipped_queries
        );
        for failure in &failures {
            message.push_str("\n  - ");
            message.push_str(failure);
        }
        return Err(Error::msg(message));
    }
    Ok(report)
}

fn push_verify_failure(failures: &mut Vec<String>, failure: String) {
    if failures.len() < BAKE_VERIFY_MAX_REPORTED_MISMATCHES {
        failures.push(failure);
    }
}

/// 对给定查询池跑固定种子的确定性查询差分，比较 live 与 Bake 的 best/top。
#[allow(clippy::too_many_arguments)]
fn run_trade_query_samples(
    signature_key: &str,
    level: u8,
    capacity: usize,
    order: TradeOrderKind,
    universe: &TradePool,
    full_pool: bool,
    table: &SkillTable,
    layout: &Arc<LayoutContext>,
    runtime_table: &RuntimeBakedComboTable,
    gold_lines: u32,
    report: &mut BakeVerifyReport,
    failures: &mut Vec<String>,
) {
    let mut rng = seeded_rng(&format!("{signature_key}|full_pool={full_pool}"));
    for query_index in 0..BAKE_VERIFY_QUERIES_PER_SIGNATURE {
        let size = sample_query_size(
            &mut rng,
            distinct_pool_name_count(universe),
            capacity,
            query_index,
        );
        let chosen = sample_variant_subset(&mut rng, universe, size);
        let pool = TradePool {
            entries: chosen
                .iter()
                .map(|&position| universe.entries[position].clone())
                .collect(),
            skipped: Vec::new(),
        };
        let top_k = BAKE_VERIFY_TOP_K[query_index % BAKE_VERIFY_TOP_K.len()];
        let must_name = if query_index % 7 == 6 {
            let position = (rng.next_u64() as usize) % pool.entries.len();
            Some(pool.entries[position].name.clone())
        } else {
            None
        };
        let filter = SearchTripleFilter {
            must_include_name: must_name,
            ..SearchTripleFilter::default()
        };
        let live_options = TradeSearchOptions {
            trade_level: level,
            operator_capacity: capacity,
            mood: [24.0, 12.0, 3.0][query_index % 3],
            top_k,
            gold_production_lines: gold_lines,
            layout: Arc::clone(layout),
            shift_hours: [24.0, 8.0, 0.0, -4.0][query_index % 4],
            order_mode: TradeSearchOrderMode::Single(order),
            bake_mode: BakeMode::Disabled,
            full_pool,
        };
        if pool.entries.len() < capacity {
            report.skipped_queries += 1;
            continue;
        }
        let mut bake_options = live_options;
        bake_options.bake_mode = BakeMode::Required;
        let compatible = baked_trade_compatible(&pool, &bake_options, &filter, table);
        let mut bake_filter = filter;
        bake_filter.forbidden_pairs = crate::trade::legality::GLOBAL_FORBIDDEN_TRADE_PAIRS
            .iter()
            .map(|&(left, right)| (left.to_string(), right.to_string()))
            .collect();
        if !compatible || canonical_trade_query_ids(runtime_table, &pool).is_err() {
            report.skipped_queries += 1;
            continue;
        }
        let mut live_options = bake_options.clone();
        live_options.bake_mode = BakeMode::Disabled;
        let live = crate::search::search_trade_triples_filtered(
            &pool,
            table,
            &live_options,
            bake_filter.clone(),
        );
        let bake = baked_trade_report(
            runtime_table,
            &pool,
            &bake_options,
            order,
            &bake_filter,
            if bake_filter.has_must_include() {
                n_choose_k_u64(pool.entries.len() - 1, capacity - 1)
            } else {
                n_choose_k_u64(pool.entries.len(), capacity)
            },
            StdInstant::now(),
        );
        report.queries_checked += 1;
        if let Some(failure) =
            compare_trade_query(live, bake, query_index, signature_key, size, top_k)
        {
            report.query_mismatches += 1;
            push_verify_failure(failures, failure);
        }
    }
}

fn compare_trade_query(
    live: Result<TradeSearchReport>,
    bake: Result<Option<TradeSearchReport>>,
    query_index: usize,
    signature_key: &str,
    size: usize,
    top_k: usize,
) -> Option<String> {
    let mut problems = Vec::new();
    match (live, bake) {
        (Ok(live_report), Ok(Some(bake_report))) => {
            push_value_diff(&live_report.best, &bake_report.best, "best", &mut problems);
            push_value_diff(&live_report.top, &bake_report.top, "top", &mut problems);
            if live_report.evaluated != bake_report.evaluated {
                problems.push("evaluated differs".to_string());
            }
            if live_report.combinations != bake_report.combinations {
                problems.push("combinations differs".to_string());
            }
        }
        (Ok(_), Ok(None)) => problems.push("bake miss".to_string()),
        (Ok(_), Err(error)) => problems.push(format!("bake error: {error}")),
        (Err(live_error), Err(bake_error))
            if live_error.to_string() == "no trade combo matched search filter"
                && bake_error.to_string()
                    == "required Bake miss: signature has no eligible trade combo" => {}
        (Err(error), _) => problems.push(format!("live error: {error}")),
    }
    if problems.is_empty() {
        return None;
    }
    Some(format!(
        "query {query_index} {signature_key} size={size} top_k={top_k}: {}",
        problems.join(", ")
    ))
}

fn compare_manufacture_query(
    live: Result<ManuSearchReport>,
    bake: Result<Option<ManuSearchReport>>,
    query_index: usize,
    signature_key: &str,
    size: usize,
    top_k: usize,
) -> Option<String> {
    let mut problems = Vec::new();
    match (live, bake) {
        (Ok(live_report), Ok(Some(bake_report))) => {
            push_value_diff(&live_report.best, &bake_report.best, "best", &mut problems);
            push_value_diff(&live_report.top, &bake_report.top, "top", &mut problems);
            if live_report.evaluated != bake_report.evaluated {
                problems.push("evaluated differs".to_string());
            }
            if live_report.combinations != bake_report.combinations {
                problems.push("combinations differs".to_string());
            }
        }
        (Ok(_), Ok(None)) => problems.push("bake miss".to_string()),
        (Ok(_), Err(error)) => problems.push(format!("bake error: {error}")),
        (Err(error), _) => problems.push(format!("live error: {error}")),
    }
    if problems.is_empty() {
        return None;
    }
    Some(format!(
        "query {query_index} {signature_key} size={size} top_k={top_k}: {}",
        problems.join(", ")
    ))
}

pub fn load_complete_baked_trade_catalog(
    out_dir: &Path,
    generator: &BakeGeneratorFingerprint,
) -> Result<CompleteBakedTradeCatalog> {
    let validated = load_validated_baked_catalog(out_dir, generator)?;
    let trade_pool = validated
        .trade_pool
        .ok_or_else(|| Error::msg("baked catalog does not include trade"))?;
    let skill_table = validated
        .trade_skill_table
        .ok_or_else(|| Error::msg("baked catalog does not include trade"))?;
    let trade_state_index = pool_state_index(&trade_pool, OperatorState::from_trade_entry);
    Ok(CompleteBakedTradeCatalog {
        disk: validated.disk,
        trade_state_index,
        trade_pool,
        skill_table,
    })
}

impl CompleteBakedTradeCatalog {
    pub fn rows_for(
        &self,
        room_level: u8,
        order_kind: TradeOrderKind,
    ) -> Result<CompleteBakedTradeRows> {
        let disk = &self.disk;
        let signature_key = trade_lookup_key(
            room_level,
            station_operator_capacity(room_level),
            order_kind,
            baked_search_baseline().gold_manu_line_count,
        );
        let index = disk
            .indexes
            .iter()
            .find(|index| index.signature_key == signature_key)
            .ok_or_else(|| {
                Error::msg(format!("complete baked trade rows miss {signature_key:?}"))
            })?;
        let mut rows = Vec::with_capacity(index.len);
        for (offset, disk_row) in disk.rows[index.start..index.start + index.len]
            .iter()
            .enumerate()
        {
            let names = disk_row
                .operator_indices
                .iter()
                .map(|operator| disk.operator_names[*operator].clone())
                .collect::<Vec<_>>();
            let operators = row_pool_indices(disk_row, disk, &self.trade_state_index)?
                .into_iter()
                .map(|idx| self.trade_pool.entries[idx].to_trade_operator())
                .collect::<Vec<_>>();
            let count = |tag: &str| {
                operators
                    .iter()
                    .filter(|operator| operator.has_tag(tag))
                    .count()
                    .min(u8::MAX as usize) as u8
            };
            let mut row = BakedComboRow::from(disk_row.clone());
            row.row_id = index.start + offset;
            row.facility = "trade".to_string();
            row.signature_key = signature_key.clone();
            row.names = names.clone();
            row.operator_mask = operator_mask_from_indices(&row.operator_indices, disk.mask_words);
            rows.push(CompleteBakedTradeRow {
                row_id: BakedTradeRowId {
                    signature_key: signature_key.clone(),
                    offset,
                },
                names,
                logical_mask: row.operator_mask.clone(),
                siracusa_count: count("cc.g.siracusa"),
                glasgow_count: count("cc.g.glasgow"),
                karlan_count: count("cc.g.karlan"),
                baseline_response: row_to_trade_hit(&row)
                    .ok_or_else(|| Error::msg("baked trade row has incomplete response"))?,
            });
        }
        Ok(CompleteBakedTradeRows {
            signature_key,
            rows,
        })
    }

    pub fn resolve_row_live(
        &self,
        row_id: &BakedTradeRowId,
        room_level: u8,
        order_kind: TradeOrderKind,
        layout: Arc<LayoutContext>,
        mood: f64,
        shift_hours: f64,
    ) -> Result<TradeSearchHit> {
        let expected_key = trade_lookup_key(
            room_level,
            station_operator_capacity(room_level),
            order_kind,
            baked_search_baseline().gold_manu_line_count,
        );
        if row_id.signature_key != expected_key {
            return Err(Error::msg(format!(
                "baked trade row signature mismatch: row={:?}, query={expected_key:?}",
                row_id.signature_key
            )));
        }
        let index = self
            .disk
            .indexes
            .iter()
            .find(|index| index.signature_key == row_id.signature_key)
            .ok_or_else(|| Error::msg("baked trade row signature is absent"))?;
        if row_id.offset >= index.len {
            return Err(Error::msg("baked trade row offset is out of range"));
        }
        let disk_row = &self.disk.rows[index.start + row_id.offset];
        let operators = row_pool_indices(disk_row, &self.disk, &self.trade_state_index)?
            .into_iter()
            .map(|idx| self.trade_pool.entries[idx].to_trade_operator())
            .collect();
        let input = TradeRoomInput {
            level: room_level,
            operators,
            order_count: None,
            mood,
            gold_production_lines: Some(layout.gold_manu_line_count),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout,
            active_order_kind: order_kind,
        };
        let result = solve_trade_with_shift_prevalidated(&input, &self.skill_table, shift_hours)?;
        let efficiency = &result.efficiency;
        Ok(TradeSearchHit {
            names: input
                .operators
                .iter()
                .map(|operator| operator.name.clone())
                .collect(),
            gold_names: vec![],
            originium_names: vec![],
            final_efficiency: efficiency.final_efficiency,
            mechanic_equivalent_efficiency: result.order_mechanic.mechanic_equivalent_efficiency,
            rule_id: result.rule_id.clone(),
            unit_trade_per_day: result.production.unit.unit_trade_per_day,
            unit_gold_per_day: result.production.unit.unit_gold_per_day,
            unit_originium_per_day: result.production.unit.unit_originium_per_day,
            breakdown: Some(TradeEfficiencyBreakdown {
                base_efficiency: efficiency.paper.base_efficiency,
                occupancy_efficiency: efficiency.paper.occupancy_efficiency,
                skill_efficiency: efficiency.paper.skill_efficiency,
                control_efficiency: efficiency.paper.control_efficiency,
                global_efficiency: efficiency.paper.control_efficiency,
                paper_efficiency: efficiency.paper.paper_efficiency,
                total_efficiency: efficiency.total_efficiency,
                mechanic_equivalent_efficiency: result
                    .order_mechanic
                    .mechanic_equivalent_efficiency,
                unit_output_multiplier: efficiency.production_basis.unit_output_multiplier,
                final_efficiency: efficiency.final_efficiency,
                equivalent_skill_efficiency: efficiency.equivalent_skill_efficiency,
                trade_equivalent_efficiency: efficiency.trade_equivalent_efficiency,
                gold_equivalent_efficiency: efficiency.gold_equivalent_efficiency,
                combined_output_value: efficiency.combined_output_value,
                unit_trade_per_day: result.production.unit.unit_trade_per_day,
                unit_gold_per_day: result.production.unit.unit_gold_per_day,
                trade_output_per_day: result.production.unit.unit_trade_per_day
                    * efficiency.total_efficiency.as_f64(),
                gold_consumption_per_day: result.production.unit.gold_consumption_per_day
                    * crate::trade::unit_output::GSL_GOLD_UNIT_SCALE
                    * efficiency.total_efficiency.as_f64(),
                originium_shards_per_day: result.production.unit.unit_originium_per_day
                    * efficiency.total_efficiency.as_f64(),
                rule_id: result.rule_id,
                ..TradeEfficiencyBreakdown::default()
            }),
        })
    }
}

fn validate_complete_trade_row_universe(
    disk: &BakedComboTableDisk,
    trade_pool: &TradePool,
    table: &SkillTable,
) -> Result<()> {
    let operator_index = disk
        .operator_states
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, state)| (state, idx))
        .collect::<HashMap<_, _>>();
    for index in disk
        .indexes
        .iter()
        .filter(|index| index.signature_key.starts_with("trade:"))
    {
        let first = disk
            .rows
            .get(index.start)
            .ok_or_else(|| Error::msg("trade signature has no rows"))?;
        let mut expected = BTreeSet::new();
        for combo in combinations_indices(trade_pool.entries.len(), first.operator_capacity) {
            if !distinct_names(
                combo
                    .iter()
                    .map(|idx| trade_pool.entries[*idx].name.as_str()),
            ) {
                continue;
            }
            let operators = build_trade_combo_operators_vec(trade_pool, &combo, None, None);
            if trade_station_exclusive_violation(&operators, table) {
                continue;
            }
            expected.insert(state_indices(
                operators.iter().map(trade_operator_state),
                &operator_index,
            )?);
        }
        validate_row_universe(disk, index, expected)?;
    }
    Ok(())
}

fn validate_complete_manufacture_row_universe(
    disk: &BakedComboTableDisk,
    pool: &ManuPool,
    table: &SkillTable,
) -> Result<()> {
    let operator_index = disk
        .operator_states
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, state)| (state, idx))
        .collect::<HashMap<_, _>>();
    for index in disk
        .indexes
        .iter()
        .filter(|index| index.signature_key.starts_with("manufacture:"))
    {
        let first = disk
            .rows
            .get(index.start)
            .ok_or_else(|| Error::msg("manufacture signature has no rows"))?;
        let mut expected = BTreeSet::new();
        for combo in combinations_indices(pool.entries.len(), first.operator_capacity) {
            if !distinct_names(combo.iter().map(|idx| pool.entries[*idx].name.as_str())) {
                continue;
            }
            let operators = combo
                .iter()
                .map(|idx| pool.entries[*idx].to_manu_operator())
                .collect::<Vec<_>>();
            if !crate::search::manufacture_combo_allowed_for_shift(&operators, table, None) {
                continue;
            }
            expected.insert(state_indices(
                combo
                    .iter()
                    .map(|idx| OperatorState::from_manu_entry(&pool.entries[*idx])),
                &operator_index,
            )?);
        }
        validate_row_universe(disk, index, expected)?;
    }
    Ok(())
}

fn validate_row_universe(
    disk: &BakedComboTableDisk,
    index: &BakedComboIndex,
    expected: BTreeSet<Vec<usize>>,
) -> Result<()> {
    let actual = disk.rows[index.start..index.start + index.len]
        .iter()
        .map(|row| row.operator_indices.clone())
        .collect::<BTreeSet<_>>();
    if actual != expected || index.len != expected.len() {
        return Err(Error::msg(format!(
            "baked row universe is incomplete for {:?}",
            index.signature_key
        )));
    }
    Ok(())
}

fn validate_trade_mechanism_signatures(
    disk: &BakedComboTableDisk,
    trade_pool: &TradePool,
    state_index: &HashMap<OperatorState, usize>,
) -> Result<()> {
    for actual in disk.rows.iter().filter(|row| row.order_kind.is_some()) {
        let combo = row_pool_indices(actual, disk, state_index)?;
        // Row indices already describe actual execution order; do not reconstruct by name.
        let operators = combo
            .iter()
            .map(|idx| trade_pool.entries[*idx].to_trade_operator())
            .collect::<Vec<_>>();
        if trade_room_mechanism_signature(&operators) != actual.trade_mechanism_signature {
            return Err(Error::msg(format!(
                "baked trade mechanism signature mismatch for states {:?}",
                actual.operator_indices
            )));
        }
    }
    Ok(())
}

fn validate_baked_combo_table(
    table: &BakedComboTableDisk,
    generator: &BakeGeneratorFingerprint,
    options: &BakeManifestOptions,
) -> Result<()> {
    if table.schema_version != BAKE_SCHEMA_VERSION {
        return Err(Error::msg("baked table schema mismatch"));
    }
    let Some(table_generator) = table.generator.as_ref() else {
        return Err(Error::msg("baked table has no generator fingerprint"));
    };
    if table_generator.kind != generator.kind
        || table_generator.bytes != generator.bytes
        || table_generator.hash64 != generator.hash64
    {
        return Err(Error::msg("baked table generator mismatch"));
    }
    if table.operator_count != table.operator_states.len()
        || table.operator_count != table.operator_names.len()
        || table
            .operator_states
            .iter()
            .zip(&table.operator_names)
            .any(|(state, name)| &state.name != name)
    {
        return Err(Error::msg("baked table operator state/count mismatch"));
    }
    let unique_states = table
        .operator_states
        .iter()
        .collect::<std::collections::HashSet<_>>();
    if unique_states.len() != table.operator_states.len() {
        return Err(Error::msg("baked table contains duplicate operator states"));
    }
    let expected_mask_words = table.operator_count.div_ceil(64).max(1);
    if table.mask_words != expected_mask_words {
        return Err(Error::msg("baked table mask width mismatch"));
    }
    let expected = expected_signature_keys(options);
    let actual: std::collections::BTreeSet<_> = table
        .indexes
        .iter()
        .map(|index| index.signature_key.clone())
        .collect();
    if actual != expected {
        return Err(Error::msg(format!(
            "baked signature set mismatch: expected {}, found {}",
            expected.len(),
            actual.len()
        )));
    }

    let mut previous_end = 0usize;
    let mut keys = std::collections::HashSet::new();
    for index in &table.indexes {
        if !keys.insert(index.signature_key.as_str()) {
            return Err(Error::msg("duplicate baked signature"));
        }
        let Some(end) = index.start.checked_add(index.len) else {
            return Err(Error::msg("baked index range overflow"));
        };
        if index.start != previous_end || end > table.rows.len() {
            return Err(Error::msg(format!(
                "invalid baked index range for {:?}",
                index.signature_key
            )));
        }
        if index.len == 0 {
            return Err(Error::msg(format!(
                "empty baked signature {:?}",
                index.signature_key
            )));
        }
        if row_facility_from_signature(&index.signature_key).is_empty() {
            return Err(Error::msg(format!(
                "unknown baked signature {:?}",
                index.signature_key
            )));
        }
        for row in &table.rows[index.start..end] {
            let mut unique_indices = row.operator_indices.clone();
            unique_indices.sort_unstable();
            unique_indices.dedup();
            if row.operator_capacity != row.operator_indices.len()
                || unique_indices.len() != row.operator_indices.len()
                || row
                    .operator_indices
                    .iter()
                    .any(|operator| *operator >= table.operator_count)
                || !distinct_names(row.operator_indices.iter().filter_map(|idx| {
                    table
                        .operator_states
                        .get(*idx)
                        .map(|state| state.name.as_str())
                }))
            {
                return Err(Error::msg(format!(
                    "invalid baked row in signature {:?}",
                    index.signature_key
                )));
            }
            validate_baked_row_for_signature(row, &index.signature_key)?;
        }
        for pair in table.rows[index.start..end].windows(2) {
            let left_storage = pair[0].manufacture_storage_limit.unwrap_or_default();
            let right_storage = pair[1].manufacture_storage_limit.unwrap_or_default();
            if pair[0].sort_efficiency_millis < pair[1].sort_efficiency_millis
                || (pair[0].sort_efficiency_millis == pair[1].sort_efficiency_millis
                    && left_storage < right_storage)
                || (pair[0].sort_efficiency_millis == pair[1].sort_efficiency_millis
                    && left_storage == right_storage
                    && pair[0].operator_indices > pair[1].operator_indices)
            {
                return Err(Error::msg(format!(
                    "baked rows are not sorted for {:?}",
                    index.signature_key
                )));
            }
        }
        previous_end = end;
    }
    if previous_end != table.rows.len() {
        return Err(Error::msg("baked indexes do not cover every row"));
    }
    Ok(())
}

fn expected_signature_keys(options: &BakeManifestOptions) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let gold_lines = baked_search_baseline().gold_manu_line_count;
    if options.include_trade {
        for level in 1..=3 {
            keys.insert(trade_lookup_key(
                level,
                station_operator_capacity(level),
                TradeOrderKind::Gold,
                gold_lines,
            ));
        }
        keys.insert(trade_lookup_key(
            3,
            station_operator_capacity(3),
            TradeOrderKind::Originium,
            gold_lines,
        ));
    }
    if options.include_manufacture {
        for level in 1..=3 {
            for recipe in [
                RecipeKind::Gold,
                RecipeKind::BattleRecord,
                RecipeKind::Originium,
            ] {
                keys.insert(manufacture_lookup_key(
                    level,
                    station_operator_capacity(level),
                    recipe,
                ));
            }
        }
    }
    keys
}

fn validate_baked_row_for_signature(row: &BakedComboRowDisk, key: &str) -> Result<()> {
    if row.operator_capacity != station_operator_capacity(row.room_level) {
        return Err(Error::msg(format!("invalid room capacity in {key:?}")));
    }
    if let Some(signature) = &row.trade_mechanism_signature {
        let count_is_valid = |count: Option<u8>| {
            count.is_none_or(|value| usize::from(value) <= row.operator_capacity)
        };
        if signature
            .peer_count
            .is_some_and(|value| usize::from(value) >= row.operator_capacity)
            || !count_is_valid(signature.glasgow_count)
            || !count_is_valid(signature.snhunt_count)
            || !count_is_valid(signature.laterano_count)
            || signature.is_empty()
        {
            return Err(Error::msg(format!(
                "invalid trade mechanism signature in {key:?}"
            )));
        }
    }
    let expected_key = if let Some(order) = row.order_kind {
        if row.recipe.is_some()
            || !trade_response_fields_present(row)
            || any_manufacture_response_field_present(row)
        {
            return Err(Error::msg(format!("invalid trade row fields in {key:?}")));
        }
        trade_lookup_key(
            row.room_level,
            row.operator_capacity,
            order,
            baked_search_baseline().gold_manu_line_count,
        )
    } else if let Some(recipe) = row.recipe {
        if row.order_kind.is_some()
            || row.trade_mechanism_signature.is_some()
            || !manufacture_response_fields_present(row)
            || any_trade_response_field_present(row)
        {
            return Err(Error::msg(format!(
                "invalid manufacture row fields in {key:?}"
            )));
        }
        manufacture_lookup_key(row.room_level, row.operator_capacity, recipe)
    } else {
        return Err(Error::msg(format!("baked row has no facility in {key:?}")));
    };
    if expected_key != key {
        return Err(Error::msg(format!("baked row does not match {key:?}")));
    }
    let sort_efficiency = row
        .trade_final_efficiency_millis
        .or(row.manufacture_final_efficiency_millis)
        .ok_or_else(|| Error::msg(format!("baked row has no final efficiency in {key:?}")))?;
    if row.sort_efficiency_millis != sort_efficiency {
        return Err(Error::msg(format!(
            "baked row sort key mismatch in {key:?}"
        )));
    }
    Ok(())
}

fn trade_response_fields_present(row: &BakedComboRowDisk) -> bool {
    row.trade_base_efficiency_millis.is_some()
        && row.trade_occupancy_efficiency_millis.is_some()
        && row.trade_skill_efficiency_millis.is_some()
        && row.trade_control_efficiency_millis.is_some()
        && row.trade_paper_efficiency_millis.is_some()
        && row.trade_mechanic_equivalent_efficiency_millis.is_some()
        && row.trade_unit_output_multiplier_millis.is_some()
        && row.trade_final_efficiency_millis.is_some()
        && row.trade_equivalent_skill_efficiency_millis.is_some()
        && row.trade_gold_equivalent_efficiency_millis.is_some()
        && row.trade_combined_output_value_millis.is_some()
        && row.trade_total_efficiency_millis.is_some()
        && row.unit_trade_per_day.is_some()
        && row.unit_gold_per_day.is_some()
        && row.unit_gold_consumption_per_day.is_some()
        && row.unit_originium_per_day.is_some()
}

fn manufacture_response_fields_present(row: &BakedComboRowDisk) -> bool {
    row.manufacture_base_efficiency_millis.is_some()
        && row.manufacture_occupancy_efficiency_millis.is_some()
        && row.manufacture_skill_efficiency_millis.is_some()
        && row.manufacture_global_efficiency_millis.is_some()
        && row.manufacture_final_efficiency_millis.is_some()
        && row.manufacture_storage_limit.is_some()
        && row.manufacture_unit_output_per_day.is_some()
        && row.manufacture_output_per_day.is_some()
}

fn any_trade_response_field_present(row: &BakedComboRowDisk) -> bool {
    row.trade_base_efficiency_millis.is_some()
        || row.trade_occupancy_efficiency_millis.is_some()
        || row.trade_skill_efficiency_millis.is_some()
        || row.trade_control_efficiency_millis.is_some()
        || row.trade_paper_efficiency_millis.is_some()
        || row.trade_mechanic_equivalent_efficiency_millis.is_some()
        || row.trade_unit_output_multiplier_millis.is_some()
        || row.trade_final_efficiency_millis.is_some()
        || row.trade_equivalent_skill_efficiency_millis.is_some()
        || row.trade_total_efficiency_millis.is_some()
        || row.unit_trade_per_day.is_some()
        || row.unit_gold_per_day.is_some()
        || row.unit_gold_consumption_per_day.is_some()
        || row.unit_originium_per_day.is_some()
}

fn any_manufacture_response_field_present(row: &BakedComboRowDisk) -> bool {
    row.manufacture_base_efficiency_millis.is_some()
        || row.manufacture_occupancy_efficiency_millis.is_some()
        || row.manufacture_skill_efficiency_millis.is_some()
        || row.manufacture_global_efficiency_millis.is_some()
        || row.manufacture_final_efficiency_millis.is_some()
        || row.manufacture_storage_limit.is_some()
        || row.manufacture_unit_output_per_day.is_some()
        || row.manufacture_output_per_day.is_some()
}

fn validate_baked_input_fingerprints(manifest: &BakeManifest, out_dir: &Path) -> Result<()> {
    let expected = bake_input_fingerprints()?;
    for current in expected {
        let Some(name) = Path::new(&current.path).file_name() else {
            continue;
        };
        let Some(baked) = manifest
            .inputs
            .iter()
            .find(|input| Path::new(&input.path).file_name() == Some(name))
        else {
            return Err(Error::msg(format!(
                "baked input mismatch: {} missing from manifest; rerun `infra-cli bake --out {}`",
                name.to_string_lossy(),
                out_dir.display()
            )));
        };
        if baked.bytes != current.bytes || baked.hash64 != current.hash64 {
            return Err(Error::msg(format!(
                "baked input mismatch: {} changed; rerun `infra-cli bake --out {}`",
                name.to_string_lossy(),
                out_dir.display()
            )));
        }
    }
    Ok(())
}

pub fn try_baked_trade_search(
    pool: &TradePool,
    skill_table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    filter: &SearchTripleFilter,
    combinations: u64,
    start: StdInstant,
) -> Result<Option<TradeSearchReport>> {
    let mode = options.bake_mode;
    if mode == BakeMode::Disabled {
        return Ok(None);
    }
    if filter
        .forbidden_pairs
        .iter()
        .any(|(a, b)| !crate::search::is_global_trade_forbidden_pair(a, b))
    {
        return baked_miss(mode, "trade forbidden-pair constraints are not represented");
    }
    if !baked_trade_compatible(pool, options, filter, skill_table) {
        return baked_miss(
            mode,
            trade_incompatibility_reason(pool, options, filter, skill_table),
        );
    }
    let Some(table) = load_runtime_baked_table(mode)? else {
        return baked_miss(
            mode,
            "catalog is missing, stale, or currently being generated",
        );
    };
    if !runtime_skill_table_matches(table, skill_table) {
        return baked_miss(mode, "skill table source differs from the catalog");
    }
    baked_trade_report(
        table,
        pool,
        options,
        order_kind,
        filter,
        combinations,
        start,
    )
}

/// 原查询组合逐个 rank lookup；只保留前 K 轻量项，最后物化输出。
fn baked_trade_report(
    table: &RuntimeBakedComboTable,
    pool: &TradePool,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    filter: &SearchTripleFilter,
    combinations: u64,
    start: StdInstant,
) -> Result<Option<TradeSearchReport>> {
    let mode = options.bake_mode;
    if filter.hit_filter.is_some() {
        return baked_miss(mode, "hit predicate requires live evaluation");
    }
    let ids = match canonical_trade_query_ids(table, pool) {
        Ok(ids) => ids,
        Err(reason) => return baked_miss(mode, reason),
    };
    let gold_lines = if options.layout.gold_manu_line_count > 0 {
        options.layout.gold_manu_line_count
    } else {
        options.gold_production_lines
    };
    let signature_slot =
        trade_signature_slot(options.trade_level, options.operator_capacity, order_kind)
            .filter(|_| !order_kind.is_gold() || gold_lines == table.trade_gold_lines);
    let Some(ranks) = signature_slot.and_then(|slot| table.rank_rows_by_signature[slot].as_ref())
    else {
        return baked_miss_lazy(mode, || {
            let key = trade_lookup_key(
                options.trade_level,
                options.operator_capacity,
                order_kind,
                gold_lines,
            );
            format!("signature {key:?} is absent")
        });
    };
    let k = options.operator_capacity;
    if !(1..=3).contains(&k) {
        return baked_miss(mode, "rank capacity is not represented");
    }
    let mut top = match light_heap(pool.entries.len(), k, options.top_k.max(1)) {
        Ok(top) => top,
        Err(error) => return baked_miss(mode, error.to_string()),
    };
    let must = filter.must_include_names();
    let mut evaluated = 0u64;
    for (ordinal, positions) in rank::combinations(pool.entries.len(), k).enumerate() {
        let names = positions[..k]
            .iter()
            .map(|idx| pool.entries[*idx].name.as_str());
        if must
            .iter()
            .any(|name| !names.clone().any(|candidate| candidate == name))
        {
            continue;
        }
        if filter.forbidden_pairs.iter().any(|(a, b)| {
            names.clone().any(|name| name == a) && names.clone().any(|name| name == b)
        }) {
            continue;
        }
        let Some(row) = ranked_row(table, ranks, &ids, &positions[..k])? else {
            continue;
        };
        let Some(eff) = row.trade_final_efficiency else {
            return baked_miss(mode, "ranked trade response is incomplete");
        };
        evaluated += 1;
        retain_light(
            &mut top,
            LightRow {
                eff,
                storage: 0,
                names: [""; 3],
                ordinal,
                row_id: row.row_id,
                positions,
            },
            options.top_k.max(1),
        );
    }
    let selected = sorted_light(top);
    if selected.is_empty() {
        return baked_miss(mode, "signature has no eligible trade combo");
    }
    let mut hits = selected
        .iter()
        .map(|light| {
            row_to_trade_hit(&table.table.rows[light.row_id])
                .ok_or_else(|| Error::msg("ranked trade row has incomplete response"))
        })
        .collect::<Result<Vec<_>>>()?;
    let best = hits[0].clone();
    hits.truncate(options.top_k);
    Ok(Some(TradeSearchReport {
        order_mode: TradeSearchOrderMode::Single(order_kind),
        best,
        top: hits,
        combinations,
        evaluated,
        elapsed: start.elapsed(),
        gold_order_line: None,
        originium_order_line: None,
    }))
}

pub fn try_baked_manufacture_search(
    pool: &ManuPool,
    skill_table: &SkillTable,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
    combinations: u64,
    start: StdInstant,
) -> Result<Option<ManuSearchReport>> {
    let mode = options.bake_mode;
    if mode == BakeMode::Disabled {
        return Ok(None);
    }
    if !baked_manufacture_compatible(pool, options, skill_table) {
        return baked_miss(
            mode,
            manufacture_incompatibility_reason(pool, options, skill_table),
        );
    }
    let Some(table) = load_runtime_baked_table(mode)? else {
        return baked_miss(
            mode,
            "catalog is missing, stale, or currently being generated",
        );
    };
    if !runtime_skill_table_matches(table, skill_table) {
        return baked_miss(mode, "skill table source differs from the catalog");
    }
    baked_manufacture_report(table, pool, options, recipe, combinations, start)
}

/// 制造沿 live 效率、仓库、组合姓名序列比较；零命中保留成功空报告。
fn baked_manufacture_report(
    table: &RuntimeBakedComboTable,
    pool: &ManuPool,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
    combinations: u64,
    start: StdInstant,
) -> Result<Option<ManuSearchReport>> {
    let mode = options.bake_mode;
    let ids = match canonical_manu_query_ids(table, pool) {
        Ok(ids) => ids,
        Err(reason) => return baked_miss(mode, reason),
    };
    let Some(ranks) = manufacture_signature_slot(options.level, options.operator_capacity, recipe)
        .and_then(|slot| table.rank_rows_by_signature[slot].as_ref())
    else {
        return baked_miss_lazy(mode, || {
            let key = manufacture_lookup_key(options.level, options.operator_capacity, recipe);
            format!("signature {key:?} is absent")
        });
    };
    let k = options.operator_capacity;
    if !(1..=3).contains(&k) {
        return baked_miss(mode, "rank capacity is not represented");
    }
    let mut top = match light_heap(pool.entries.len(), k, options.top_k.max(1)) {
        Ok(top) => top,
        Err(error) => return baked_miss(mode, error.to_string()),
    };
    let mut evaluated = 0u64;
    for (ordinal, positions) in rank::combinations(pool.entries.len(), k).enumerate() {
        let mut names = [""; 3];
        for (slot, idx) in positions[..k].iter().enumerate() {
            names[slot] = pool.entries[*idx].name.as_str();
        }
        if options
            .must_include_name
            .as_ref()
            .is_some_and(|name| !names[..k].iter().any(|candidate| candidate == name))
        {
            continue;
        }
        let Some(row) = ranked_row(table, ranks, &ids, &positions[..k])? else {
            continue;
        };
        let Some(eff) = row.manufacture_final_efficiency else {
            return baked_miss(mode, "ranked manufacture response is incomplete");
        };
        let storage = match recipe {
            RecipeKind::Gold | RecipeKind::BattleRecord => {
                row.manufacture_storage_limit.unwrap_or(0).max(0)
            }
            _ => 0,
        };
        evaluated += 1;
        retain_light(
            &mut top,
            LightRow {
                eff,
                storage,
                names,
                ordinal,
                row_id: row.row_id,
                positions,
            },
            options.top_k.max(1),
        );
    }
    let selected = sorted_light(top);
    let mut hits = selected
        .iter()
        .map(|light| {
            row_to_manu_hit(&table.table.rows[light.row_id])
                .ok_or_else(|| Error::msg("ranked manufacture row has incomplete response"))
        })
        .collect::<Result<Vec<_>>>()?;
    let best = hits.first().cloned().unwrap_or_else(empty_manufacture_hit);
    hits.truncate(options.top_k);
    Ok(Some(ManuSearchReport {
        recipe_mode: options.recipe_mode,
        best,
        top: hits,
        combinations,
        evaluated,
        elapsed: start.elapsed(),
        gold_line: None,
        battle_record_line: None,
    }))
}

fn empty_manufacture_hit() -> ManuSearchHit {
    ManuSearchHit {
        names: vec![],
        gold_names: vec![],
        battle_record_names: vec![],
        final_efficiency: Efficiency::ZERO,
        per_station: Default::default(),
        storage: Default::default(),
        breakdown: Default::default(),
    }
}

// BinaryHeap 的最大项为最差项；所有比较键均来自原 live 排序。
#[derive(Debug, Clone, PartialEq, Eq)]
struct LightRow<'a> {
    eff: Efficiency,
    storage: i32,
    names: [&'a str; 3],
    ordinal: usize,
    row_id: usize,
    positions: [usize; 3],
}

impl Ord for LightRow<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .eff
            .cmp(&self.eff)
            .then_with(|| other.storage.cmp(&self.storage))
            .then_with(|| self.names.cmp(&other.names))
            .then_with(|| self.ordinal.cmp(&other.ordinal))
            .then_with(|| self.positions.cmp(&other.positions))
            .then_with(|| self.row_id.cmp(&other.row_id))
    }
}
impl PartialOrd for LightRow<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn light_heap<'a>(n: usize, k: usize, limit: usize) -> Result<BinaryHeap<LightRow<'a>>> {
    let mut heap = BinaryHeap::new();
    heap.try_reserve(limit.min(rank::choose(n, k)?))
        .map_err(|error| Error::msg(format!("cannot allocate Bake top-K heap: {error}")))?;
    Ok(heap)
}
fn retain_light<'a>(heap: &mut BinaryHeap<LightRow<'a>>, row: LightRow<'a>, limit: usize) {
    if heap.len() < limit {
        heap.push(row);
    } else if heap.peek().is_some_and(|worst| row < *worst) {
        if let Some(mut worst) = heap.peek_mut() {
            *worst = row;
        }
    }
}
fn sorted_light(heap: BinaryHeap<LightRow<'_>>) -> Vec<LightRow<'_>> {
    heap.into_sorted_vec()
}

fn ranked_row<'a>(
    table: &'a RuntimeBakedComboTable,
    ranks: &[usize],
    ids: &[usize],
    positions: &[usize],
) -> Result<Option<&'a BakedComboRow>> {
    let mut combo_ids = [0; 3];
    for (slot, idx) in positions.iter().enumerate() {
        combo_ids[slot] = ids[*idx];
    }
    let combo_rank = rank::colex(&combo_ids[..positions.len()])?;
    let row_id = *ranks
        .get(combo_rank)
        .ok_or_else(|| Error::msg("Bake query rank out of range"))?;
    if row_id == rank::INVALID_ROW {
        return Ok(None);
    }
    table
        .table
        .rows
        .get(row_id)
        .map(Some)
        .ok_or_else(|| Error::msg("Bake row ID out of range"))
}

fn canonical_query_ids(
    table: &RuntimeBakedComboTable,
    states: impl IntoIterator<Item = OperatorState>,
    canonical: &HashMap<usize, usize>,
) -> std::result::Result<Vec<usize>, &'static str> {
    let states = states.into_iter().collect::<Vec<_>>();
    if !distinct_names(states.iter().map(|state| state.name.as_str())) {
        return Err("query contains duplicate operator names");
    }
    let mut ids = Vec::with_capacity(states.len());
    let mut previous = None;
    for state in states {
        let Some(id) = table.operator_index_by_state.get(&state).copied() else {
            return Err("complete operator state is not represented");
        };
        let Some(position) = canonical.get(&id).copied() else {
            return Err("operator state is not represented for this facility");
        };
        if previous.is_some_and(|prev| position <= prev) {
            return Err("query operator order is not canonical");
        }
        previous = Some(position);
        ids.push(id);
    }
    Ok(ids)
}

fn canonical_trade_query_ids(
    table: &RuntimeBakedComboTable,
    pool: &TradePool,
) -> std::result::Result<Vec<usize>, &'static str> {
    let ids = canonical_query_ids(
        table,
        pool.entries.iter().map(OperatorState::from_trade_entry),
        &table.trade_canonical,
    )?;
    for (id, entry) in ids.iter().zip(&pool.entries) {
        if !runtime_atoms_match(table, false, *id, &entry.compiled_atoms) {
            return Err("compiled trade atoms differ from the canonical state");
        }
    }
    Ok(ids)
}

fn canonical_manu_query_ids(
    table: &RuntimeBakedComboTable,
    pool: &ManuPool,
) -> std::result::Result<Vec<usize>, &'static str> {
    let ids = canonical_query_ids(
        table,
        pool.entries.iter().map(OperatorState::from_manu_entry),
        &table.manu_canonical,
    )?;
    for (id, entry) in ids.iter().zip(&pool.entries) {
        // None 在 live 中按已匹配原文的 SkillTable 补编译；Some（含空）是独立输入。
        if entry
            .compiled_atoms
            .as_ref()
            .is_some_and(|atoms| !runtime_atoms_match(table, true, *id, atoms))
        {
            return Err("compiled manufacture atoms differ from the canonical state");
        }
    }
    Ok(ids)
}

fn runtime_atoms_match(
    table: &RuntimeBakedComboTable,
    manufacture: bool,
    state_id: usize,
    atoms: &Arc<[CompiledAtom]>,
) -> bool {
    let canonical = if manufacture {
        &table.manu_atoms
    } else {
        &table.trade_atoms
    };
    let Some(expected) = canonical.get(&state_id) else {
        return false;
    };
    if Arc::ptr_eq(atoms, expected) {
        return true;
    }
    let key = (manufacture, state_id, atoms.as_ptr() as usize);
    let mut cache = table
        .atom_match_cache
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cache
        .entry(key)
        .or_insert_with(|| {
            // 完整结构比较拒绝非有限 NaN，Debug 的浮点文本进一步区分 signed zero。
            // 这是加载表内的一次保守门禁，不是持久化格式或长期 memo key。
            let matches = atoms.len() == expected.len()
                && atoms
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, b)| a.seq == b.seq && a.sort_key == b.sort_key && a.atom == b.atom)
                && format!("{atoms:?}") == format!("{expected:?}");
            (Arc::clone(atoms), matches)
        })
        .1
}

pub fn warm_runtime_baked_table() -> Result<bool> {
    Ok(load_runtime_baked_table(BakeMode::Auto)?.is_some())
}

static RUNTIME_BAKE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static RUNTIME_AUTO_CACHE: OnceLock<std::result::Result<RuntimeBakedComboTable, String>> =
    OnceLock::new();
static RUNTIME_REQUIRED_CACHE: OnceLock<std::result::Result<RuntimeBakedComboTable, String>> =
    OnceLock::new();

struct RuntimeBakeGuard;

impl RuntimeBakeGuard {
    fn acquire() -> Result<Self> {
        RUNTIME_BAKE_IN_PROGRESS
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::msg("runtime Bake generation already in progress"))?;
        Ok(Self)
    }
}

impl Drop for RuntimeBakeGuard {
    fn drop(&mut self) {
        RUNTIME_BAKE_IN_PROGRESS.store(false, Ordering::Release);
    }
}

fn load_runtime_baked_table(mode: BakeMode) -> Result<Option<&'static RuntimeBakedComboTable>> {
    if RUNTIME_BAKE_IN_PROGRESS.load(Ordering::Acquire) {
        return baked_miss(mode, "catalog is currently being generated");
    }
    if let Some(Ok(table)) = RUNTIME_AUTO_CACHE.get() {
        return Ok(Some(table));
    }
    if let Some(Ok(table)) = RUNTIME_REQUIRED_CACHE.get() {
        return Ok(Some(table));
    }
    let cached = if mode == BakeMode::Auto {
        RUNTIME_AUTO_CACHE.get_or_init(|| {
            let result = load_runtime_baked_table_inner(true).map_err(|error| error.to_string());
            if let Err(reason) = &result {
                eprintln!("[bake] runtime catalog unavailable; using live: {reason}");
            }
            result
        })
    } else {
        RUNTIME_REQUIRED_CACHE.get_or_init(|| {
            load_runtime_baked_table_inner(false).map_err(|error| error.to_string())
        })
    };
    match cached {
        Ok(table) => Ok(Some(table)),
        Err(reason) => baked_miss(mode, reason.as_str()),
    }
}

fn load_runtime_baked_table_inner(regenerate: bool) -> Result<RuntimeBakedComboTable> {
    let out_dir = default_baked_out_dir()?;
    let generator = current_exe_generator_fingerprint()?;
    match load_validated_baked_catalog(&out_dir, &generator) {
        Ok(validated) => return runtime_baked_table_from_disk(validated.disk),
        Err(error) if !regenerate => return Err(error),
        Err(_) => {}
    }
    let _guard = RuntimeBakeGuard::acquire()?;
    let options = BakeOptions {
        out_dir: out_dir.clone(),
        generator: Some(generator.clone()),
        ..BakeOptions::default()
    };
    publish_baked_catalogs(&options, true)?;
    let disk = load_validated_baked_catalog(&out_dir, &generator)?.disk;
    runtime_baked_table_from_disk(disk)
}

fn runtime_baked_table_from_disk(disk: BakedComboTableDisk) -> Result<RuntimeBakedComboTable> {
    let mut table = BakedComboTable::from(disk);
    for index in &table.indexes {
        for row in &mut table.rows[index.start..index.start + index.len] {
            row.signature_key = index.signature_key.clone();
            row.facility = row_facility_from_signature(&index.signature_key).to_string();
        }
    }
    for (idx, row) in table.rows.iter_mut().enumerate() {
        row.row_id = idx;
        if row.names.is_empty() && !table.operator_names.is_empty() {
            row.names = row
                .operator_indices
                .iter()
                .filter_map(|idx| table.operator_names.get(*idx).cloned())
                .collect();
        }
        if row.operator_mask.is_empty() {
            row.operator_mask = operator_mask_from_indices(&row.operator_indices, table.mask_words);
        }
    }
    let mut universe_mask_by_key: HashMap<String, Vec<u64>> = HashMap::new();
    for index in &table.indexes {
        let mut universe = vec![0u64; table.mask_words];
        for row in &table.rows[index.start..index.start + index.len] {
            for (slot, word) in universe.iter_mut().zip(&row.operator_mask) {
                *slot |= *word;
            }
        }
        universe_mask_by_key.insert(index.signature_key.clone(), universe);
    }
    let operator_index_by_state = table
        .operator_states
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, state)| (state, idx))
        .collect::<HashMap<_, _>>();
    let skill_table = SkillTable::load(&default_skill_table_path()?)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let pools = build_variant_pools(&instances, &skill_table)?;
    let trade_canonical = canonical_state_ranks(
        &pools.trade,
        OperatorState::from_trade_entry,
        &operator_index_by_state,
    )?;
    let manu_canonical = canonical_state_ranks(
        &pools.manufacture,
        OperatorState::from_manu_entry,
        &operator_index_by_state,
    )?;
    let trade_atoms = trade_canonical
        .iter()
        .map(|(id, position)| {
            (
                *id,
                Arc::clone(&pools.trade.entries[*position].compiled_atoms),
            )
        })
        .collect();
    let manu_atoms = manu_canonical
        .iter()
        .map(|(id, position)| {
            (
                *id,
                pools.manufacture.entries[*position]
                    .compiled_atoms
                    .as_ref()
                    .map(Arc::clone)
                    .unwrap_or_else(|| {
                        skill_table.compile_manufacture_atoms(
                            &pools.manufacture.entries[*position].buff_ids,
                        )
                    }),
            )
        })
        .collect();
    let trade_gold_lines = baked_search_baseline().gold_manu_line_count;
    let mut rank_rows_by_signature = std::array::from_fn(|_| None);
    for index in &table.indexes {
        let end = index
            .start
            .checked_add(index.len)
            .ok_or_else(|| Error::msg("Bake signature range overflow"))?;
        let rows = table
            .rows
            .get(index.start..end)
            .ok_or_else(|| Error::msg("Bake signature range is out of bounds"))?;
        let first = rows
            .first()
            .ok_or_else(|| Error::msg("empty Bake signature"))?;
        let capacity = first.operator_capacity;
        let (signature_slot, expected_key) = match (first.order_kind, first.recipe) {
            (Some(order), None) => (
                trade_signature_slot(first.room_level, capacity, order),
                trade_lookup_key(first.room_level, capacity, order, trade_gold_lines),
            ),
            (None, Some(recipe)) => (
                manufacture_signature_slot(first.room_level, capacity, recipe),
                manufacture_lookup_key(first.room_level, capacity, recipe),
            ),
            _ => {
                return Err(Error::msg(
                    "Bake signature has inconsistent facility fields",
                ))
            }
        };
        let signature_slot =
            signature_slot.ok_or_else(|| Error::msg("unsupported Bake signature"))?;
        if expected_key != index.signature_key {
            return Err(Error::msg("Bake signature key differs from its first row"));
        }
        if rank_rows_by_signature[signature_slot].is_some() {
            return Err(Error::msg("duplicate Bake signature slot"));
        }
        let mut ranks = rank::empty_rows(table.operator_count, capacity)?;
        for row in rows {
            let position = rank::colex(&row.operator_indices)?;
            let slot = ranks
                .get_mut(position)
                .ok_or_else(|| Error::msg("Bake row rank is out of range"))?;
            if *slot != rank::INVALID_ROW {
                return Err(Error::msg("duplicate Bake combo rank"));
            }
            *slot = row.row_id;
        }
        rank_rows_by_signature[signature_slot] = Some(ranks);
    }
    Ok(RuntimeBakedComboTable {
        table,
        operator_index_by_state,
        universe_mask_by_key,
        rank_rows_by_signature,
        trade_gold_lines,
        trade_canonical,
        manu_canonical,
        trade_atoms,
        manu_atoms,
        atom_match_cache: Mutex::new(HashMap::new()),
        skill_source_json: Arc::clone(skill_table.source_json()),
        skill_match_by_identity: Mutex::new(HashMap::new()),
    })
}

fn canonical_state_ranks<T>(
    pool: &PoolCore<T>,
    identity: fn(&T) -> OperatorState,
    state_index: &HashMap<OperatorState, usize>,
) -> Result<HashMap<usize, usize>> {
    pool.entries
        .iter()
        .enumerate()
        .map(|(rank, entry)| {
            let state = identity(entry);
            let id = state_index
                .get(&state)
                .copied()
                .ok_or_else(|| Error::msg("canonical Bake state missing"))?;
            Ok((id, rank))
        })
        .collect()
}

fn runtime_skill_table_matches(runtime: &RuntimeBakedComboTable, table: &SkillTable) -> bool {
    let mut cache = runtime
        .skill_match_by_identity
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *cache
        .entry(table.identity())
        .or_insert_with(|| runtime.skill_source_json.as_ref() == table.source_json().as_ref())
}

const BAKED_SIGNATURE_SLOTS: usize = 13;

/// 只接受磁盘完整验证允许的等级和容量，不把越界等级 clamp 成合法 signature。
fn trade_signature_slot(level: u8, capacity: usize, order: TradeOrderKind) -> Option<usize> {
    if !(1..=3).contains(&level) || capacity != usize::from(level) {
        return None;
    }
    match order {
        TradeOrderKind::Gold => Some(usize::from(level - 1)),
        TradeOrderKind::Originium if level == 3 => Some(3),
        TradeOrderKind::Originium => None,
    }
}

fn manufacture_signature_slot(level: u8, capacity: usize, recipe: RecipeKind) -> Option<usize> {
    if !(1..=3).contains(&level) || capacity != usize::from(level) {
        return None;
    }
    let recipe_index = match recipe {
        RecipeKind::Gold => 0,
        RecipeKind::BattleRecord => 1,
        RecipeKind::Originium => 2,
        RecipeKind::All => return None,
    };
    Some(4 + usize::from(level - 1) * 3 + recipe_index)
}

fn trade_lookup_key(
    room_level: u8,
    operator_capacity: usize,
    order_kind: TradeOrderKind,
    gold_lines: u32,
) -> String {
    match order_kind {
        TradeOrderKind::Gold => format!(
            "trade:level{}:cap{}:order_{:?}:gold_lines{}",
            room_level, operator_capacity, order_kind, gold_lines
        ),
        TradeOrderKind::Originium => format!(
            "trade:level{}:cap{}:order_{:?}",
            room_level, operator_capacity, order_kind
        ),
    }
    .to_ascii_lowercase()
}

fn manufacture_lookup_key(room_level: u8, operator_capacity: usize, recipe: RecipeKind) -> String {
    format!(
        "manufacture:level{}:cap{}:recipe_{:?}",
        room_level, operator_capacity, recipe
    )
    .to_ascii_lowercase()
}

/// The executable fingerprint (including failures) is computed once per process.
pub fn current_exe_generator_fingerprint() -> Result<BakeGeneratorFingerprint> {
    static CACHE: OnceLock<std::result::Result<BakeGeneratorFingerprint, String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let fingerprint = || -> Result<BakeGeneratorFingerprint> {
                let path = std::env::current_exe()?;
                let bytes = fs::read(&path)?;
                Ok(BakeGeneratorFingerprint {
                    kind: "infra-cli-exe".to_string(),
                    path: path.to_string_lossy().to_string(),
                    bytes: bytes.len() as u64,
                    hash64: hash_bytes(&bytes),
                })
            };
            fingerprint().map_err(|error| error.to_string())
        })
        .clone()
        .map_err(Error::msg)
}

fn mood_sensitive_pool<T>(
    pool: &PoolCore<T>,
    buffs: impl Fn(&T) -> &[String],
    table: &SkillTable,
) -> bool {
    pool.entries.iter().any(|entry| {
        buffs(entry).iter().any(|id| {
            table.get(id).is_some_and(|skill| {
                skill.atoms.iter().any(|atom| {
                    matches!(atom.selector, Some(Selector::OwnerMoodGap | Selector::Mood))
                        || matches!(
                            atom.condition,
                            Some(
                                Condition::OwnerMoodGapAbove { .. }
                                    | Condition::MoodAbove { .. }
                                    | Condition::MoodAboveOrEq { .. }
                                    | Condition::MoodBelow { .. }
                                    | Condition::MoodBelowOrEq { .. }
                            )
                        )
                })
            })
        })
    })
}

fn baked_trade_compatible(
    pool: &TradePool,
    options: &TradeSearchOptions,
    filter: &SearchTripleFilter,
    table: &SkillTable,
) -> bool {
    trade_incompatibility_reason(pool, options, filter, table).is_empty()
}
fn trade_incompatibility_reason(
    pool: &TradePool,
    options: &TradeSearchOptions,
    filter: &SearchTripleFilter,
    table: &SkillTable,
) -> &'static str {
    if filter.must_operator_override.is_some() {
        "operator override is not represented"
    } else if filter.hit_filter.is_some() {
        "hit predicate requires live evaluation"
    } else if !options.mood.is_finite() || options.mood <= 0.0 {
        "mood must be finite and positive"
    } else if !options.shift_hours.is_finite() {
        "shift duration must be finite"
    } else if !baked_layout_compatible(&options.layout) {
        "complete layout differs from the baked baseline"
    } else if mood_sensitive_pool(pool, |entry| &entry.buff_ids, table) {
        "mood-sensitive skills require live settlement"
    } else {
        ""
    }
}

fn baked_manufacture_compatible(
    pool: &ManuPool,
    options: &ManuSearchOptions,
    table: &SkillTable,
) -> bool {
    manufacture_incompatibility_reason(pool, options, table).is_empty()
}
fn manufacture_incompatibility_reason(
    pool: &ManuPool,
    options: &ManuSearchOptions,
    table: &SkillTable,
) -> &'static str {
    if !options.mood.is_finite() || options.mood <= 0.0 {
        "mood must be finite and positive"
    } else if options.shift_hours.is_some_and(|hours| !hours.is_finite()) {
        "shift duration must be finite"
    } else if !baked_layout_compatible(&options.layout) {
        "complete layout differs from the baked baseline"
    } else if mood_sensitive_pool(pool, |entry| &entry.buff_ids, table) {
        "mood-sensitive skills require live settlement"
    } else if options.pool_has_peer_reset(pool, table) {
        "peer-reset skills require live settlement"
    } else {
        ""
    }
}

fn baked_search_baseline() -> &'static LayoutContext {
    static BASELINE: OnceLock<LayoutContext> = OnceLock::new();
    BASELINE.get_or_init(LayoutContext::search_baseline)
}
fn baked_layout_compatible(layout: &LayoutContext) -> bool {
    let baseline = baked_search_baseline();
    let same_float = |actual: f64, expected: f64| {
        actual.is_finite() && expected.is_finite() && actual.to_bits() == expected.to_bits()
    };
    // PartialEq includes active_source_buffs; bit guards keep signed zero distinct.
    if layout != baseline
        || layout.global_inject.has_dynamic_trade_inject()
        || !layout.global_inject.manu_count_scaled().is_empty()
        || !layout
            .global_inject
            .exact_bake_compatible(&baseline.global_inject)
        || !same_float(layout.office_hire_spd_pct, baseline.office_hire_spd_pct)
        || !same_float(
            layout.control_office_hire_spd_pct,
            baseline.control_office_hire_spd_pct,
        )
        || !same_float(
            layout.control_meeting_speed_pct,
            baseline.control_meeting_speed_pct,
        )
    {
        return false;
    }
    if crate::global_resource::GlobalResourceKey::ALL
        .iter()
        .any(|key| !same_float(layout.global.get(*key), baseline.global.get(*key)))
    {
        return false;
    }
    let actual = &layout.global_inject;
    let expected = &baseline.global_inject;
    if !same_float(actual.trade_eff_pct(), expected.trade_eff_pct())
        || !same_float(
            actual.trade_global_flat_eff_pct(),
            expected.trade_global_flat_eff_pct(),
        )
        || !same_float(
            actual.manu_global_flat_eff_pct(),
            expected.manu_global_flat_eff_pct(),
        )
    {
        return false;
    }
    for recipe in [
        RecipeKind::All,
        RecipeKind::Gold,
        RecipeKind::BattleRecord,
        RecipeKind::Originium,
    ] {
        if !same_float(actual.manu_eff_for(recipe), expected.manu_eff_for(recipe)) {
            return false;
        }
    }
    if actual
        .manu_tagged()
        .iter()
        .zip(expected.manu_tagged())
        .any(|(a, b)| !same_float(a.value, b.value))
        || actual
            .manu_count_scaled()
            .iter()
            .zip(expected.manu_count_scaled())
            .any(|(a, b)| !same_float(a.value_per_operator, b.value_per_operator))
    {
        return false;
    }
    match (actual.karlan_precision(), expected.karlan_precision()) {
        (Some(a), Some(b)) => same_float(a.eff_per_karlan, b.eff_per_karlan),
        (None, None) => true,
        _ => false,
    }
}

fn row_to_trade_hit(row: &BakedComboRow) -> Option<TradeSearchHit> {
    if row.facility != "trade" {
        return None;
    }
    let final_efficiency = row.trade_final_efficiency?;
    let mechanic_equivalent_efficiency = row.trade_mechanic_equivalent_efficiency?;
    let unit_trade_per_day = row.unit_trade_per_day.unwrap_or(0.0);
    let unit_gold_per_day = row.unit_gold_per_day.unwrap_or(0.0);
    let unit_gold_consumption_per_day = row.unit_gold_consumption_per_day?;
    let unit_originium_per_day = row.unit_originium_per_day.unwrap_or(0.0);
    let total_efficiency = row.trade_total_efficiency?;
    let control_efficiency = row.trade_control_efficiency?;
    Some(TradeSearchHit {
        names: row.names.clone(),
        gold_names: vec![],
        originium_names: vec![],
        final_efficiency,
        mechanic_equivalent_efficiency,
        rule_id: row.rule_id.clone(),
        unit_trade_per_day,
        unit_gold_per_day,
        unit_originium_per_day,
        breakdown: Some(TradeEfficiencyBreakdown {
            base_efficiency: row.trade_base_efficiency?,
            occupancy_efficiency: row.trade_occupancy_efficiency?,
            skill_efficiency: row.trade_skill_efficiency?,
            control_efficiency,
            global_efficiency: control_efficiency,
            paper_efficiency: row.trade_paper_efficiency?,
            mechanic_equivalent_efficiency,
            unit_output_multiplier: row.trade_unit_output_multiplier?,
            final_efficiency,
            total_efficiency,
            equivalent_skill_efficiency: row.trade_equivalent_skill_efficiency?,
            trade_equivalent_efficiency: row.trade_equivalent_skill_efficiency?,
            gold_equivalent_efficiency: row.trade_gold_equivalent_efficiency?,
            combined_output_value: row.trade_combined_output_value?,
            unit_trade_per_day,
            unit_gold_per_day,
            trade_output_per_day: unit_trade_per_day * total_efficiency.as_f64(),
            gold_consumption_per_day: unit_gold_consumption_per_day
                * crate::trade::unit_output::GSL_GOLD_UNIT_SCALE
                * total_efficiency.as_f64(),
            originium_shards_per_day: unit_originium_per_day * total_efficiency.as_f64(),
            rule_id: row.rule_id.clone(),
        }),
    })
}

fn row_to_manu_hit(row: &BakedComboRow) -> Option<ManuSearchHit> {
    if row.facility != "manufacture" {
        return None;
    }
    let recipe = row.recipe?;
    let final_efficiency = row.manufacture_final_efficiency?;
    let storage_limit = row.manufacture_storage_limit.unwrap_or(0);
    let mut per_station = crate::manufacture::ManuProdBreakdown::default();
    let mut storage = crate::manufacture::ManuStorageBreakdown::default();
    let recipe_label = match recipe {
        RecipeKind::Gold => {
            per_station.gold = final_efficiency;
            storage.gold = storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = final_efficiency;
            storage.battle_record = storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = final_efficiency;
            storage.originium = storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };
    Some(ManuSearchHit {
        names: row.names.clone(),
        gold_names: vec![],
        battle_record_names: vec![],
        final_efficiency,
        per_station,
        storage,
        breakdown: ManuEfficiencyBreakdown {
            base_efficiency: row.manufacture_base_efficiency?,
            occupancy_efficiency: row.manufacture_occupancy_efficiency?,
            skill_efficiency: row.manufacture_skill_efficiency?,
            global_efficiency: row.manufacture_global_efficiency?,
            final_efficiency,
            unit_output_per_day: row.manufacture_unit_output_per_day?,
            output_per_day: row.manufacture_output_per_day?,
            storage_limit,
            recipe: recipe_label.to_string(),
        },
    })
}

fn build_combo_table_from_rows(
    operator_states: Vec<OperatorState>,
    mask_words: usize,
    mut rows: Vec<BakedComboRow>,
    generator: Option<BakeGeneratorFingerprint>,
) -> BakedComboTable {
    rows.sort_by(|a, b| {
        a.facility
            .cmp(&b.facility)
            .then_with(|| a.signature_key.cmp(&b.signature_key))
            .then_with(|| compare_baked_rows(a, b))
    });
    for (idx, row) in rows.iter_mut().enumerate() {
        row.row_id = idx;
    }

    let indexes = build_combo_indexes(&rows);
    BakedComboTable {
        schema_version: BAKE_SCHEMA_VERSION,
        generator,
        operator_count: operator_states.len(),
        operator_names: operator_states
            .iter()
            .map(|state| state.name.clone())
            .collect(),
        operator_states,
        mask_words,
        indexes,
        rows,
    }
}

fn build_combo_indexes(rows: &[BakedComboRow]) -> Vec<BakedComboIndex> {
    let mut indexes = Vec::new();
    let mut start = 0usize;
    while start < rows.len() {
        let key = rows[start].signature_key.clone();
        let mut end = start + 1;
        while end < rows.len() && rows[end].signature_key == key {
            end += 1;
        }
        indexes.push(BakedComboIndex {
            signature_key: key,
            start,
            len: end - start,
        });
        start = end;
    }
    indexes
}

fn trade_operator_state(operator: &TradeOperator) -> OperatorState {
    OperatorState {
        name: operator.name.clone(),
        elite: operator.elite,
        buff_ids: operator.buff_ids.clone(),
        tags: operator.tags.clone(),
    }
}

fn state_indices(
    states: impl IntoIterator<Item = OperatorState>,
    index: &HashMap<OperatorState, usize>,
) -> Result<Vec<usize>> {
    states
        .into_iter()
        .map(|state| {
            index
                .get(&state)
                .copied()
                .ok_or_else(|| Error::msg(format!("baked operator state is absent: {state:?}")))
        })
        .collect()
}

fn pool_state_index<T>(
    pool: &PoolCore<T>,
    identity: fn(&T) -> OperatorState,
) -> HashMap<OperatorState, usize> {
    pool.entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| (identity(entry), idx))
        .collect()
}

fn row_pool_indices(
    row: &BakedComboRowDisk,
    disk: &BakedComboTableDisk,
    index: &HashMap<OperatorState, usize>,
) -> Result<Vec<usize>> {
    row.operator_indices
        .iter()
        .map(|idx| {
            index
                .get(&disk.operator_states[*idx])
                .copied()
                .ok_or_else(|| {
                    Error::msg(format!(
                        "baked operator state is absent from live pool: {:?}",
                        disk.operator_states[*idx]
                    ))
                })
        })
        .collect()
}

fn operator_mask_from_indices(indices: &[usize], mask_words: usize) -> Vec<u64> {
    let mut mask = vec![0u64; mask_words];
    for idx in indices {
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(slot) = mask.get_mut(word) {
            *slot |= 1u64 << bit;
        }
    }
    mask
}

fn row_facility_from_signature(signature_key: &str) -> &'static str {
    if signature_key.starts_with("trade:") {
        "trade"
    } else if signature_key.starts_with("manufacture:") {
        "manufacture"
    } else {
        ""
    }
}

fn station_operator_capacity(level: u8) -> usize {
    level.clamp(1, 3) as usize
}

fn bake_input_fingerprints() -> Result<Vec<BakeInputFingerprint>> {
    let mut paths = BTreeMap::new();
    for name in [
        "operator_instances.json",
        "skill_table.json",
        "standalone_roster.json",
        "trade_shortcuts.json",
        "trade_segments.json",
        "base_systems.json",
        "layout/243_use_this_.json",
    ] {
        let path = data_path(name)?;
        paths.insert(name, path);
    }
    paths
        .into_values()
        .map(|path| fingerprint_file(&path))
        .collect()
}

fn fingerprint_file(path: &Path) -> Result<BakeInputFingerprint> {
    let bytes = fs::read(path)?;
    Ok(BakeInputFingerprint {
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len() as u64,
        hash64: hash_bytes(&bytes),
    })
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let file = File::create(&path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)
        .map_err(|e| Error::msg(format!("write {}: {e}", path.display())))
}
