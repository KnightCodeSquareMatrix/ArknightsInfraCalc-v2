mod compact;
mod rank;
mod state;
mod stream;

use state::{build_variant_pools, distinct_names, OperatorState};

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::fs::{self, File};
use std::hash::Hash;
use std::io::{BufReader, BufWriter, Write};
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
use crate::pool::{build_trade_combo_operators_vec, n_choose_k_u64};
use crate::pool::{ManuPool, PoolCore, TradePool};
use crate::profile::{BakeColdPhase, BakeMissKind};
use crate::search::{
    ManuEfficiencyBreakdown, ManuSearchHit, ManuSearchOptions, ManuSearchReport,
    SearchTripleFilter, TradeEfficiencyBreakdown, TradeSearchHit, TradeSearchOptions,
    TradeSearchReport,
};
use crate::skill_table::{default_skill_table_path, SkillTable};
use crate::trade::input::{TradeOperator, TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use crate::trade::solver::solve_trade_with_shift_prevalidated;
use crate::types::{Action, CompiledAtom, Condition, RecipeKind, Selector};

static BAKE_ENABLED: AtomicBool = AtomicBool::new(true);

/// Process-wide CLI switch; set before starting searches or warming the table.
pub fn set_bake_enabled(enabled: bool) {
    BAKE_ENABLED.store(enabled, Ordering::Relaxed);
}

pub const BAKE_SCHEMA_VERSION: u32 = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BakeMode {
    #[default]
    Auto,
    Disabled,
    Required,
}

impl BakeMissKind {
    fn reason(self) -> &'static str {
        match self {
            Self::ForbiddenPairs => "trade forbidden-pair constraints are not represented",
            Self::Override => "operator override is not represented",
            Self::Predicate => "hit predicate requires live evaluation",
            Self::InvalidMood => "mood must be finite and positive",
            Self::InvalidShift => "shift duration must be finite",
            Self::Layout => "complete layout differs from the baked baseline",
            Self::MoodSensitive => "mood-sensitive skills require live settlement",
            Self::Ramp => "shift-dependent ramp skills require live settlement",
            Self::PeerReset => "peer-reset skills require live settlement",
            Self::Generating => "catalog is currently being generated",
            Self::Unavailable => "catalog is missing, stale, or unavailable",
            Self::SkillTable => "skill table source differs from the catalog",
            Self::DuplicateNames => "query contains duplicate operator names",
            Self::StateAbsent => "complete operator state is not represented",
            Self::FacilityStateAbsent => "operator state is not represented for this facility",
            Self::NonCanonicalOrder => "query operator order is not canonical",
            Self::TradeAtoms => "compiled trade atoms differ from the canonical state",
            Self::ManufactureAtoms => "compiled manufacture atoms differ from the canonical state",
            Self::SignatureAbsent => "signature is absent",
            Self::Capacity => "rank capacity is not represented",
            Self::HeapUnavailable => "Bake top-K heap is unavailable",
            Self::TradeIncomplete => "ranked trade response is incomplete",
            Self::ManufactureIncomplete => "ranked manufacture response is incomplete",
            Self::NoEligibleTrade => "signature has no eligible trade combo",
        }
    }
}

fn baked_miss<T>(
    mode: BakeMode,
    kind: BakeMissKind,
    miss: &mut Option<BakeMissKind>,
) -> Result<Option<T>> {
    baked_miss_lazy(mode, kind, miss, || kind.reason().to_string())
}

fn baked_miss_lazy<T>(
    mode: BakeMode,
    kind: BakeMissKind,
    miss: &mut Option<BakeMissKind>,
    reason: impl FnOnce() -> String,
) -> Result<Option<T>> {
    *miss = Some(kind);
    if mode == BakeMode::Required {
        Err(Error::msg(format!("required Bake miss: {}", reason())))
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
    pub progress: Option<BakeProgressCallback>,
}

impl Default for BakeOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("data/baked"),
            include_trade: true,
            include_manufacture: true,
            limit_per_signature: None,
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
struct BakeGeneratorFingerprint {
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
    table: compact::Table,
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
    combo_table: BakeComboTableSummary,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakeComboTableSummary {
    bytes: u64,
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

#[derive(Debug)]
struct RuntimeBakedComboTable {
    table: compact::Table,
    operator_index_by_state: HashMap<OperatorState, usize>,
    /// 贸易四种与制造九种合法 signature；磁盘仍保留原 String key。
    rank_rows_by_signature: [Option<Vec<u32>>; BAKED_SIGNATURE_SLOTS],
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
    #[serde(rename = "f", default, skip_serializing)]
    facility: String,
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
    let mut cold = crate::profile::start_bake_cold(BakeColdPhase::Generate);
    let result = bake_catalogs_measured(options);
    cold.finish(&result);
    result
}

fn bake_catalogs_measured(options: &BakeOptions) -> Result<BakeReport> {
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
    let mut row_blocks = stream::SignatureRowBlocks::new(&options.out_dir);
    if options.include_trade {
        let baked = bake_trade_rows(
            &pools.trade,
            &table,
            options,
            &operator_index,
            mask_words,
            &mut row_blocks,
        )?;
        trade_signatures = baked.signatures;
        trade_hits = baked.row_count;
        trade_mechanism_rows = baked.mechanism_rows;
        trade_mechanism_signatures = baked.mechanism_signatures.len();
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
            &mut row_blocks,
        )?;
        manufacture_signatures = baked.signatures;
        manufacture_hits = baked.row_count;
    }

    let combo_table_rows = row_blocks.row_count();
    emit_progress(
        options,
        BakeProgressEvent::Writing {
            path: options.out_dir.join("combo_table.bin"),
            rows: Some(combo_table_rows),
        },
    );
    let combo_table = row_blocks.write_table(&operators, mask_words)?;

    let manifest = BakeManifest {
        schema_version: BAKE_SCHEMA_VERSION,
        generated_by: "infra-core::bake".to_string(),
        model: "binary_single_room_combo_table".to_string(),
        generator: None,
        inputs: Vec::new(),
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
        combo_table,
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

/// Generate a fresh immutable generation, then atomically
/// publish CURRENT. Never replaces or edits the generation readers are using.
pub fn bake_and_publish_catalogs(options: &BakeOptions) -> Result<BakeReport> {
    publish_baked_catalogs(options)
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

fn publish_baked_catalogs(options: &BakeOptions) -> Result<BakeReport> {
    let mut cold = crate::profile::start_bake_cold(BakeColdPhase::Publish);
    let result = publish_baked_catalogs_measured(options);
    cold.finish(&result);
    result
}

fn publish_baked_catalogs_measured(options: &BakeOptions) -> Result<BakeReport> {
    if options.limit_per_signature.is_some() {
        return Err(Error::msg("limited catalogs cannot be published"));
    }
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

    let lock_path = options.out_dir.join(".publish-lock");
    if let Err(error) = fs::create_dir(&lock_path) {
        // Never wait behind another publisher on a runtime request.
        if published_catalog_exists(&options.out_dir) {
            report.out_dir = options.out_dir.clone();
            return Ok(report);
        }
        return Err(Error::msg(format!("Bake publication unavailable: {error}")));
    }
    let _lock = BakePublishLock(lock_path);
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

fn published_catalog_exists(out_dir: &Path) -> bool {
    resolved_baked_catalog_dir(out_dir).is_ok_and(|dir| dir.join("combo_table.bin").is_file())
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

#[derive(Default)]
struct BakedRows {
    signatures: usize,
    row_count: usize,
    mechanism_rows: usize,
    mechanism_signatures: BTreeSet<TradeRoomMechanismSignature>,
}

fn bake_trade_rows(
    pool: &TradePool,
    table: &SkillTable,
    options: &BakeOptions,
    operator_index: &HashMap<OperatorState, usize>,
    mask_words: usize,
    row_blocks: &mut stream::SignatureRowBlocks,
) -> Result<BakedRows> {
    let layout = Arc::new(LayoutContext::search_baseline());
    let gold_lines = if layout.gold_manu_line_count > 0 {
        layout.gold_manu_line_count
    } else {
        TradeSearchOptions::default().gold_production_lines
    };
    let mut baked = BakedRows::default();

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

            let rows = stream_signature_rows(
                row_blocks,
                "trade",
                signature_key.clone(),
                rank::combinations(pool.entries.len(), operator_capacity),
                |combo| {
                    trade_combo_row(
                        pool,
                        table,
                        &row_options,
                        order_kind,
                        &combo[..operator_capacity],
                        operator_index,
                        mask_words,
                    )
                },
                options.limit_per_signature,
            )?;
            baked.row_count += rows.row_count;
            baked.mechanism_rows += rows.mechanism_rows;
            baked.mechanism_signatures.extend(rows.mechanism_signatures);
            emit_progress(
                options,
                BakeProgressEvent::SignatureFinished {
                    facility: "trade",
                    signature_key,
                    rows: rows.row_count,
                    elapsed_ms: sig_start.elapsed().as_millis(),
                },
            );
            baked.signatures += 1;
        }
    }

    Ok(baked)
}

#[allow(clippy::too_many_arguments)]
fn trade_combo_row(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    combo: &[usize],
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
        options.trade_level,
        options.operator_capacity,
        Some(order_kind),
        None,
        trade_mechanism_signature,
        &indices,
        mask_words,
    ))
}

/// 把 live 命中结构完整保存为磁盘行。
#[allow(clippy::too_many_arguments)]
fn trade_row_from_hit(
    hit: &TradeSearchHit,
    unit_gold_consumption_per_day: f64,
    facility: &str,
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
        facility: facility.to_string(),
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
    row_blocks: &mut stream::SignatureRowBlocks,
) -> Result<BakedRows> {
    let layout = Arc::new(LayoutContext::search_baseline());
    let mut baked = BakedRows::default();

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

            let rows = stream_signature_rows(
                row_blocks,
                "manufacture",
                signature_key.clone(),
                rank::combinations(pool.entries.len(), operator_capacity),
                |combo| {
                    manufacture_combo_row(
                        pool,
                        table,
                        &row_options,
                        recipe,
                        &combo[..operator_capacity],
                        operator_index,
                        mask_words,
                    )
                },
                options.limit_per_signature,
            )?;
            baked.row_count += rows.row_count;
            emit_progress(
                options,
                BakeProgressEvent::SignatureFinished {
                    facility: "manufacture",
                    signature_key,
                    rows: rows.row_count,
                    elapsed_ms: sig_start.elapsed().as_millis(),
                },
            );
            baked.signatures += 1;
        }
    }

    Ok(baked)
}

#[allow(clippy::too_many_arguments)]
fn manufacture_combo_row(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
    combo: &[usize],
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
        options.level,
        options.operator_capacity,
        recipe,
        &indices,
        mask_words,
    ))
}

/// 把 live 命中结构完整保存为磁盘行。
#[allow(clippy::too_many_arguments)]
fn manufacture_row_from_hit(
    hit: &ManuSearchHit,
    facility: &str,
    room_level: u8,
    operator_capacity: usize,
    recipe: RecipeKind,
    operator_indices: &[usize],
    mask_words: usize,
) -> BakedComboRow {
    let operator_mask = operator_mask_from_indices(operator_indices, mask_words);
    BakedComboRow {
        facility: facility.to_string(),
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

/// 有界收集签名行。
///
/// 全表生成只评价一个有界组合批次，排序块立即写盘并由 stream owner 外部归并。
/// limited 入口沿原比较键保留前缀，不改变完整表生成的候选集合。
fn stream_signature_rows<I, F>(
    blocks: &mut stream::SignatureRowBlocks,
    facility: &'static str,
    signature_key: String,
    mut combinations: I,
    solve: F,
    limit: Option<usize>,
) -> Result<stream::RowSummary>
where
    I: Iterator<Item = [usize; 3]> + Send,
    F: Fn([usize; 3]) -> Option<BakedComboRow> + Sync + Send,
{
    if let Some(limit) = limit {
        let rows = combinations
            .par_bridge()
            .filter_map(solve)
            .fold(
                || BoundedRows::with_limit(limit.max(1)),
                |mut acc, row| {
                    acc.push(row);
                    acc
                },
            )
            .reduce(|| BoundedRows::with_limit(limit.max(1)), BoundedRows::merge)
            .into_sorted_vec();
        return blocks.push_chunks(facility, signature_key, std::iter::once(rows));
    }
    const COMBOS_PER_RUN: usize = 32_768;
    let chunks = std::iter::from_fn(|| {
        let combos = combinations
            .by_ref()
            .take(COMBOS_PER_RUN)
            .collect::<Vec<_>>();
        if combos.is_empty() {
            return None;
        }
        Some(
            combos
                .into_par_iter()
                .filter_map(&solve)
                .collect::<Vec<_>>(),
        )
    });
    blocks.push_chunks(facility, signature_key, chunks)
}

/// 固定容量的最大堆：只保留按 `compare_baked_rows` 排序后最靠前的 `limit` 行。
struct BoundedRows {
    limit: usize,
    heap: BinaryHeap<HeapRow>,
}

struct HeapRow(BakedComboRow);

impl BoundedRows {
    fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            heap: BinaryHeap::with_capacity(limit.min(1024)),
        }
    }

    /// 先放入再弹出堆顶，堆顶恒为当前最差的一行，因此只需常数级额外内存。
    fn push(&mut self, row: BakedComboRow) {
        self.heap.push(HeapRow(row));
        if self.heap.len() > self.limit {
            self.heap.pop();
        }
    }

    fn merge(mut self, mut other: Self) -> Self {
        if other.heap.len() > self.heap.len() {
            std::mem::swap(&mut self, &mut other);
        }
        for HeapRow(row) in other.heap {
            self.push(row);
        }
        self
    }

    fn into_sorted_vec(self) -> Vec<BakedComboRow> {
        let mut rows: Vec<BakedComboRow> = self.heap.into_iter().map(|HeapRow(row)| row).collect();
        sort_signature_rows(&mut rows);
        rows
    }
}

impl Ord for HeapRow {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        compare_baked_rows(&self.0, &other.0)
    }
}

impl PartialOrd for HeapRow {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapRow {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for HeapRow {}

fn sort_signature_rows(rows: &mut [BakedComboRow]) {
    rows.sort_by(compare_baked_rows);
}

fn compare_baked_rows(a: &BakedComboRow, b: &BakedComboRow) -> std::cmp::Ordering {
    compare_row_keys(
        a.sort_efficiency.millis(),
        a.manufacture_storage_limit,
        &a.operator_indices,
        b.sort_efficiency.millis(),
        b.manufacture_storage_limit,
        &b.operator_indices,
    )
}

fn compare_row_keys(
    a_eff: i32,
    a_storage: Option<i32>,
    a_ids: &[usize],
    b_eff: i32,
    b_storage: Option<i32>,
    b_ids: &[usize],
) -> std::cmp::Ordering {
    b_eff
        .cmp(&a_eff)
        .then_with(|| {
            b_storage
                .unwrap_or_default()
                .cmp(&a_storage.unwrap_or_default())
        })
        .then_with(|| a_ids.cmp(b_ids))
}

fn load_baked_catalog(out_dir: &Path) -> Result<compact::Table> {
    let dir = resolved_baked_catalog_dir(out_dir)?;
    let mut reader = BufReader::new(File::open(dir.join("combo_table.bin"))?);
    let (header, _) = stream::read_header(&mut reader)?;
    compact::read(header, &mut reader)
}

pub fn load_complete_baked_trade_catalog(out_dir: &Path) -> Result<CompleteBakedTradeCatalog> {
    let table = load_baked_catalog(out_dir)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let skill_table = SkillTable::load(&default_skill_table_path()?)?;
    let trade_pool = build_variant_pools(&instances, &skill_table)?.trade;
    let trade_state_index = pool_state_index(&trade_pool, OperatorState::from_trade_entry);
    Ok(CompleteBakedTradeCatalog {
        table,
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
        let disk = &self.table.header;
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
        let signature = self
            .table
            .header
            .indexes
            .iter()
            .position(|entry| entry.signature_key == signature_key)
            .ok_or_else(|| Error::msg("baked trade signature metadata is absent"))?;
        for offset in 0..index.len {
            let row = self.table.materialize(signature, offset)?;
            let names = row
                .operator_indices
                .iter()
                .map(|operator| disk.operator_names[*operator].clone())
                .collect::<Vec<_>>();
            let operators = runtime_row_pool_indices(&row, disk, &self.trade_state_index)?
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
            .table
            .header
            .indexes
            .iter()
            .find(|index| index.signature_key == row_id.signature_key)
            .ok_or_else(|| Error::msg("baked trade row signature is absent"))?;
        if row_id.offset >= index.len {
            return Err(Error::msg("baked trade row offset is out of range"));
        }
        let signature = self
            .table
            .header
            .indexes
            .iter()
            .position(|entry| entry.signature_key == row_id.signature_key)
            .ok_or_else(|| Error::msg("baked trade signature metadata is absent"))?;
        let row = self.table.materialize(signature, row_id.offset)?;
        let operators =
            runtime_row_pool_indices(&row, &self.table.header, &self.trade_state_index)?
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
    if mode == BakeMode::Disabled || !BAKE_ENABLED.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut miss = None;
    let result = (|| {
        if filter
            .forbidden_pairs
            .iter()
            .any(|(a, b)| !crate::search::is_global_trade_forbidden_pair(a, b))
        {
            return baked_miss(mode, BakeMissKind::ForbiddenPairs, &mut miss);
        }
        if let Some(kind) = baked_trade_compatible(pool, options, filter, skill_table) {
            return baked_miss(mode, kind, &mut miss);
        }
        let Some(table) = load_runtime_baked_table(mode, &mut miss)? else {
            return Ok(None);
        };
        if !runtime_skill_table_matches(table, skill_table) {
            return baked_miss(mode, BakeMissKind::SkillTable, &mut miss);
        }
        baked_trade_report(
            table,
            pool,
            options,
            order_kind,
            filter,
            combinations,
            start,
            &mut miss,
        )
    })();
    crate::profile::record_bake_query(
        crate::profile::SearchOperation::Trade,
        miss,
        result.is_err(),
    );
    result
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
    miss: &mut Option<BakeMissKind>,
) -> Result<Option<TradeSearchReport>> {
    let mode = options.bake_mode;
    if filter.hit_filter.is_some() {
        return baked_miss(mode, BakeMissKind::Predicate, miss);
    }
    let ids = match canonical_trade_query_ids(table, pool) {
        Ok(ids) => ids,
        Err(reason) => return baked_miss(mode, reason, miss),
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
        return baked_miss_lazy(mode, BakeMissKind::SignatureAbsent, miss, || {
            let key = trade_lookup_key(
                options.trade_level,
                options.operator_capacity,
                order_kind,
                gold_lines,
            );
            format!("signature {key:?} is absent")
        });
    };
    let signature = table.table.signature_by_slot[signature_slot.unwrap()]
        .ok_or_else(|| Error::msg("Bake signature metadata is absent"))?;
    let compact::Rows::Trade(rows) = &table.table.signatures[signature].rows else {
        return Err(Error::msg("Bake trade signature has wrong row domain"));
    };
    let k = options.operator_capacity;
    if !(1..=3).contains(&k) {
        return baked_miss(mode, BakeMissKind::Capacity, miss);
    }
    let mut top = match light_heap(pool.entries.len(), k, options.top_k.max(1)) {
        Ok(top) => top,
        Err(error) => {
            return baked_miss_lazy(mode, BakeMissKind::HeapUnavailable, miss, || {
                error.to_string()
            })
        }
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
        let Some(row_id) = ranked_row(ranks, &ids, &positions[..k])? else {
            continue;
        };
        let row = rows
            .get(row_id)
            .ok_or_else(|| Error::msg("Bake row ID out of range"))?;
        let eff = Efficiency::from_millis(row.efficiencies[7]);
        evaluated += 1;
        retain_light(
            &mut top,
            LightRow {
                eff,
                storage: 0,
                names: [""; 3],
                ordinal,
                row_id,
                positions,
            },
            options.top_k.max(1),
        );
    }
    let selected = sorted_light(top);
    if selected.is_empty() {
        return baked_miss(mode, BakeMissKind::NoEligibleTrade, miss);
    }
    let mut hits = selected
        .iter()
        .map(|light| {
            row_to_trade_hit(&table.table.materialize(signature, light.row_id)?)
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
    if mode == BakeMode::Disabled || !BAKE_ENABLED.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut miss = None;
    let result = (|| {
        if let Some(kind) = baked_manufacture_compatible(pool, options, skill_table) {
            return baked_miss(mode, kind, &mut miss);
        }
        let Some(table) = load_runtime_baked_table(mode, &mut miss)? else {
            return Ok(None);
        };
        if !runtime_skill_table_matches(table, skill_table) {
            return baked_miss(mode, BakeMissKind::SkillTable, &mut miss);
        }
        baked_manufacture_report(table, pool, options, recipe, combinations, start, &mut miss)
    })();
    crate::profile::record_bake_query(
        crate::profile::SearchOperation::Manufacture,
        miss,
        result.is_err(),
    );
    result
}

/// 制造沿 live 效率、仓库、组合姓名序列比较；零命中保留成功空报告。
fn baked_manufacture_report(
    table: &RuntimeBakedComboTable,
    pool: &ManuPool,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
    combinations: u64,
    start: StdInstant,
    miss: &mut Option<BakeMissKind>,
) -> Result<Option<ManuSearchReport>> {
    let mode = options.bake_mode;
    let ids = match canonical_manu_query_ids(table, pool) {
        Ok(ids) => ids,
        Err(reason) => return baked_miss(mode, reason, miss),
    };
    let Some(ranks) = manufacture_signature_slot(options.level, options.operator_capacity, recipe)
        .and_then(|slot| table.rank_rows_by_signature[slot].as_ref())
    else {
        return baked_miss_lazy(mode, BakeMissKind::SignatureAbsent, miss, || {
            let key = manufacture_lookup_key(options.level, options.operator_capacity, recipe);
            format!("signature {key:?} is absent")
        });
    };
    let signature = table.table.signature_by_slot
        [manufacture_signature_slot(options.level, options.operator_capacity, recipe).unwrap()]
    .ok_or_else(|| Error::msg("Bake signature metadata is absent"))?;
    let compact::Rows::Manufacture(rows) = &table.table.signatures[signature].rows else {
        return Err(Error::msg(
            "Bake manufacture signature has wrong row domain",
        ));
    };
    let k = options.operator_capacity;
    if !(1..=3).contains(&k) {
        return baked_miss(mode, BakeMissKind::Capacity, miss);
    }
    let mut top = match light_heap(pool.entries.len(), k, options.top_k.max(1)) {
        Ok(top) => top,
        Err(error) => {
            return baked_miss_lazy(mode, BakeMissKind::HeapUnavailable, miss, || {
                error.to_string()
            })
        }
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
        let Some(row_id) = ranked_row(ranks, &ids, &positions[..k])? else {
            continue;
        };
        let row = rows
            .get(row_id)
            .ok_or_else(|| Error::msg("Bake row ID out of range"))?;
        let eff = Efficiency::from_millis(row.efficiencies[4]);
        let storage = match recipe {
            RecipeKind::Gold | RecipeKind::BattleRecord => row.storage.max(0),
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
                row_id,
                positions,
            },
            options.top_k.max(1),
        );
    }
    let selected = sorted_light(top);
    let mut hits = selected
        .iter()
        .map(|light| {
            row_to_manu_hit(&table.table.materialize(signature, light.row_id)?)
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

fn ranked_row(ranks: &[u32], ids: &[usize], positions: &[usize]) -> Result<Option<usize>> {
    let mut combo_ids = [0; 3];
    for (slot, idx) in positions.iter().enumerate() {
        combo_ids[slot] = ids[*idx];
    }
    let combo_rank = rank::colex(&combo_ids[..positions.len()])?;
    let row_id = *ranks
        .get(combo_rank)
        .ok_or_else(|| Error::msg("Bake query rank out of range"))?;
    Ok((row_id != rank::INVALID_ROW).then_some(row_id as usize))
}

fn canonical_query_ids(
    table: &RuntimeBakedComboTable,
    states: impl IntoIterator<Item = OperatorState>,
    canonical: &HashMap<usize, usize>,
) -> std::result::Result<Vec<usize>, BakeMissKind> {
    let states = states.into_iter().collect::<Vec<_>>();
    if !distinct_names(states.iter().map(|state| state.name.as_str())) {
        return Err(BakeMissKind::DuplicateNames);
    }
    let mut ids = Vec::with_capacity(states.len());
    let mut previous = None;
    for state in states {
        let Some(id) = table.operator_index_by_state.get(&state).copied() else {
            return Err(BakeMissKind::StateAbsent);
        };
        let Some(position) = canonical.get(&id).copied() else {
            return Err(BakeMissKind::FacilityStateAbsent);
        };
        if previous.is_some_and(|prev| position <= prev) {
            return Err(BakeMissKind::NonCanonicalOrder);
        }
        previous = Some(position);
        ids.push(id);
    }
    Ok(ids)
}

fn canonical_trade_query_ids(
    table: &RuntimeBakedComboTable,
    pool: &TradePool,
) -> std::result::Result<Vec<usize>, BakeMissKind> {
    let ids = canonical_query_ids(
        table,
        pool.entries.iter().map(OperatorState::from_trade_entry),
        &table.trade_canonical,
    )?;
    for (id, entry) in ids.iter().zip(&pool.entries) {
        if !runtime_atoms_match(table, false, *id, &entry.compiled_atoms) {
            return Err(BakeMissKind::TradeAtoms);
        }
    }
    Ok(ids)
}

fn canonical_manu_query_ids(
    table: &RuntimeBakedComboTable,
    pool: &ManuPool,
) -> std::result::Result<Vec<usize>, BakeMissKind> {
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
            return Err(BakeMissKind::ManufactureAtoms);
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
    if !BAKE_ENABLED.load(Ordering::Relaxed) {
        return Ok(false);
    }
    Ok(load_runtime_baked_table(BakeMode::Auto, &mut None)?.is_some())
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

fn load_runtime_baked_table(
    mode: BakeMode,
    miss: &mut Option<BakeMissKind>,
) -> Result<Option<&'static RuntimeBakedComboTable>> {
    if RUNTIME_BAKE_IN_PROGRESS.load(Ordering::Acquire) {
        return baked_miss(mode, BakeMissKind::Generating, miss);
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
        Err(reason) => baked_miss_lazy(mode, BakeMissKind::Unavailable, miss, || reason.clone()),
    }
}

fn load_runtime_baked_table_inner(regenerate: bool) -> Result<RuntimeBakedComboTable> {
    let mut cold = crate::profile::start_bake_cold(BakeColdPhase::Init);
    let result = load_runtime_baked_table_inner_measured(regenerate);
    cold.finish(&result);
    result
}

fn load_runtime_baked_table_inner_measured(regenerate: bool) -> Result<RuntimeBakedComboTable> {
    let out_dir = default_baked_out_dir()?;
    match load_baked_catalog(&out_dir) {
        Ok(table) => return runtime_baked_table_from_compact(table),
        Err(error) if !regenerate => return Err(error),
        Err(_) => {}
    }
    let _guard = RuntimeBakeGuard::acquire()?;
    let options = BakeOptions {
        out_dir: out_dir.clone(),
        ..BakeOptions::default()
    };
    publish_baked_catalogs(&options)?;
    runtime_baked_table_from_compact(load_baked_catalog(&out_dir)?)
}

fn runtime_baked_table_from_compact(table: compact::Table) -> Result<RuntimeBakedComboTable> {
    let mut cold = crate::profile::start_bake_cold(BakeColdPhase::Index);
    let result = runtime_baked_table_from_compact_measured(table);
    cold.finish(&result);
    result
}

fn runtime_baked_table_from_compact_measured(
    table: compact::Table,
) -> Result<RuntimeBakedComboTable> {
    let operator_index_by_state = table
        .header
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
    for signature in &table.signatures {
        let signature_slot = signature.slot()?;
        let mut ranks = rank::empty_rows(table.header.operator_count, signature.capacity)?;
        for offset in 0..signature.len() {
            let mut ids = [0usize; 3];
            for (slot, id) in signature.indices(offset).iter().enumerate() {
                ids[slot] = usize::from(*id);
            }
            let position = rank::colex(&ids[..signature.capacity])?;
            let slot = ranks
                .get_mut(position)
                .ok_or_else(|| Error::msg("Bake row rank is out of range"))?;
            *slot = u32::try_from(offset)
                .ok()
                .filter(|id| *id != rank::INVALID_ROW)
                .ok_or_else(|| Error::msg("Bake row ID exceeds u32"))?;
        }
        rank_rows_by_signature[signature_slot] = Some(ranks);
    }
    Ok(RuntimeBakedComboTable {
        table,
        operator_index_by_state,
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

/// 只接受支持的等级和容量，不把越界等级 clamp 成合法 signature。
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
) -> Option<BakeMissKind> {
    if filter.must_operator_override.is_some() {
        Some(BakeMissKind::Override)
    } else if filter.hit_filter.is_some() {
        Some(BakeMissKind::Predicate)
    } else if !options.mood.is_finite() || options.mood <= 0.0 {
        Some(BakeMissKind::InvalidMood)
    } else if !options.shift_hours.is_finite() {
        Some(BakeMissKind::InvalidShift)
    } else if !baked_layout_compatible(&options.layout) {
        Some(BakeMissKind::Layout)
    } else if mood_sensitive_pool(pool, |entry| &entry.buff_ids, table) {
        Some(BakeMissKind::MoodSensitive)
    } else {
        None
    }
}

fn baked_manufacture_compatible(
    pool: &ManuPool,
    options: &ManuSearchOptions,
    table: &SkillTable,
) -> Option<BakeMissKind> {
    if !options.mood.is_finite() || options.mood <= 0.0 {
        Some(BakeMissKind::InvalidMood)
    } else if options.shift_hours.is_some_and(|hours| !hours.is_finite()) {
        Some(BakeMissKind::InvalidShift)
    } else if !baked_layout_compatible(&options.layout) {
        Some(BakeMissKind::Layout)
    } else if mood_sensitive_pool(pool, |entry| &entry.buff_ids, table) {
        Some(BakeMissKind::MoodSensitive)
    } else if options.shift_hours.is_some() && manufacture_ramp_pool(pool, table) {
        Some(BakeMissKind::Ramp)
    } else if options.pool_has_peer_reset(pool, table) {
        Some(BakeMissKind::PeerReset)
    } else {
        None
    }
}

fn manufacture_ramp_pool(pool: &ManuPool, table: &SkillTable) -> bool {
    pool.entries.iter().any(|entry| {
        entry.buff_ids.iter().any(|id| {
            table.get(id).is_some_and(|skill| {
                skill
                    .atoms
                    .iter()
                    .any(|atom| matches!(atom.action, Action::AddEffRamp { .. }))
            })
        })
    })
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

fn runtime_row_pool_indices(
    row: &BakedComboRow,
    disk: &BakedComboTableDisk,
    index: &HashMap<OperatorState, usize>,
) -> Result<Vec<usize>> {
    row.operator_indices
        .iter()
        .map(|id| {
            index
                .get(
                    disk.operator_states
                        .get(*id)
                        .ok_or_else(|| Error::msg("Bake state ID out of range"))?,
                )
                .copied()
                .ok_or_else(|| Error::msg("baked operator state is absent from live pool"))
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

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let file = File::create(&path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)
        .map_err(|e| Error::msg(format!("write {}: {e}", path.display())))
}
