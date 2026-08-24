use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use std::time::Instant as StdInstant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::layout::LayoutContext;
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::solver::solve_manufacture;
use crate::pool::{
    build_manufacture_pool, build_trade_combo_operators_vec, build_trade_pool,
    combinations_indices, n_choose_k_u64, trade_pool_requires_candidate_projection,
};
use crate::pool::{standalone_names_for, StandaloneFilter};
use crate::pool::{HasName, ManuPool, PoolCore, TradePool};
use crate::roster::{OperatorProgress, Roster};
use crate::search::{
    ManuEfficiencyBreakdown, ManuSearchHit, ManuSearchOptions, ManuSearchReport,
    SearchTripleFilter, TradeEfficiencyBreakdown, TradeSearchHit, TradeSearchOptions,
    TradeSearchReport,
};
use crate::skill_table::{data_path, default_skill_table_path, SkillTable};
use crate::trade::input::{TradeOperator, TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use crate::trade::shortcut::trade_station_exclusive_violation;
use crate::trade::solver::solve_trade_with_shift_prevalidated;
use crate::types::{Condition, RecipeKind, Selector};
use crate::FacilityKind;

pub const BAKE_SCHEMA_VERSION: u32 = 12;

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
    let roots = default_baked_data_roots()?;
    for root in &roots {
        let out_dir = root.join("baked");
        if out_dir.join("manifest.json").exists() {
            return Ok(out_dir);
        }
    }
    Ok(roots
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("data"))
        .join("baked"))
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

#[derive(Debug, Clone, Serialize)]
struct BakedOperator {
    name: String,
    progress: OperatorProgressJson,
    facilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakedComboTable {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    #[serde(default)]
    operator_names: Vec<String>,
    mask_words: usize,
    indexes: Vec<BakedComboIndex>,
    rows: Vec<BakedComboRow>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BakedComboTableDisk {
    schema_version: u32,
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    operator_names: Vec<String>,
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
    rule_id: Option<String>,
    unit_trade_per_day: Option<f64>,
    unit_gold_per_day: Option<f64>,
    unit_originium_per_day: Option<f64>,
    manufacture_base_efficiency_millis: Option<i32>,
    manufacture_occupancy_efficiency_millis: Option<i32>,
    manufacture_skill_efficiency_millis: Option<i32>,
    manufacture_global_efficiency_millis: Option<i32>,
    manufacture_final_efficiency_millis: Option<i32>,
    manufacture_storage_limit: Option<i32>,
}

impl From<&BakedComboTable> for BakedComboTableDisk {
    fn from(value: &BakedComboTable) -> Self {
        Self {
            schema_version: value.schema_version,
            generator: value.generator.clone(),
            operator_count: value.operator_count,
            operator_names: value.operator_names.clone(),
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
            rule_id: value.rule_id.clone(),
            unit_trade_per_day: value.unit_trade_per_day,
            unit_gold_per_day: value.unit_gold_per_day,
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
            rule_id: value.rule_id,
            unit_trade_per_day: value.unit_trade_per_day,
            unit_gold_per_day: value.unit_gold_per_day,
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
        }
    }
}

#[derive(Debug)]
struct RuntimeBakedComboTable {
    table: BakedComboTable,
    index_by_key: HashMap<String, (usize, usize)>,
    operator_index_by_name: HashMap<String, usize>,
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
    #[serde(rename = "ri", skip_serializing_if = "Option::is_none")]
    rule_id: Option<String>,
    #[serde(rename = "utd", skip_serializing_if = "Option::is_none")]
    unit_trade_per_day: Option<f64>,
    #[serde(rename = "ugd", skip_serializing_if = "Option::is_none")]
    unit_gold_per_day: Option<f64>,
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

#[derive(Debug, Clone, Copy, Serialize)]
struct OperatorProgressJson {
    elite: u8,
    level: u32,
    rarity: u8,
}

impl From<OperatorProgress> for OperatorProgressJson {
    fn from(value: OperatorProgress) -> Self {
        Self {
            elite: value.elite,
            level: value.level,
            rarity: value.rarity,
        }
    }
}

pub fn bake_catalogs(options: &BakeOptions) -> Result<BakeReport> {
    let start = Instant::now();
    fs::create_dir_all(&options.out_dir)?;

    let instances_path = default_instances_path()?;
    let skill_table_path = default_skill_table_path()?;
    let instances = OperatorInstances::load(&instances_path)?;
    let table = SkillTable::load(&skill_table_path)?;
    let roster = bake_roster(&instances);
    let operators = baked_operators(&instances, &roster);
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

    let operator_index: HashMap<&str, usize> = operators
        .iter()
        .enumerate()
        .map(|(idx, op)| (op.name.as_str(), idx))
        .collect();
    let mask_words = operators.len().div_ceil(64).max(1);

    let mut trade_signatures = 0usize;
    let mut trade_hits = 0usize;
    let mut trade_mechanism_rows = 0usize;
    let mut trade_mechanism_signatures = 0usize;
    let mut rows = Vec::new();
    if options.include_trade {
        let baked = bake_trade_rows(
            &roster,
            &instances,
            &table,
            options,
            &operator_index,
            mask_words,
        )?;
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
            &roster,
            &instances,
            &table,
            options,
            &operator_index,
            mask_words,
        )?;
        manufacture_signatures = baked.signatures;
        manufacture_hits = baked.rows.len();
        rows.extend(baked.rows);
    }

    let operator_names = operators.iter().map(|op| op.name.clone()).collect();
    let combo_table =
        build_combo_table_from_rows(operator_names, mask_words, rows, options.generator.clone());
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
                "standalone_roster.json trade/manufacture entries at elite2 level1 rarity6"
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

fn filter_pool_to_names<T: HasName + Clone>(
    pool: &PoolCore<T>,
    names: Option<BTreeSet<String>>,
) -> PoolCore<T> {
    let Some(names) = names else {
        return PoolCore {
            entries: Vec::new(),
            skipped: pool.skipped.clone(),
        };
    };
    PoolCore {
        entries: pool
            .entries
            .iter()
            .filter(|entry| names.contains(entry.pool_name()))
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}

fn bake_trade_rows(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &BakeOptions,
    operator_index: &HashMap<&str, usize>,
    mask_words: usize,
) -> Result<BakedRows> {
    let pool = build_trade_pool(roster, instances, table)?;
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

            let mut rows: Vec<_> = combinations_indices(pool.entries.len(), operator_capacity)
                .collect::<Vec<_>>()
                .par_iter()
                .filter_map(|combo| {
                    trade_combo_row(
                        &pool,
                        table,
                        room_level,
                        operator_capacity,
                        order_kind,
                        gold_lines,
                        &layout,
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
    room_level: u8,
    operator_capacity: usize,
    order_kind: TradeOrderKind,
    gold_lines: u32,
    layout: &Arc<LayoutContext>,
    combo: &[usize],
    signature_key: &str,
    operator_index: &HashMap<&str, usize>,
    mask_words: usize,
) -> Option<BakedComboRow> {
    let ops = build_trade_combo_operators_vec(pool, combo, None, None);
    if trade_station_exclusive_violation(&ops, table) {
        return None;
    }
    let input = TradeRoomInput {
        level: room_level,
        operators: ops,
        order_count: None,
        mood: 24.0,
        gold_production_lines: Some(gold_lines),
        durin_virtual_lines: None,
        human_fireworks: None,
        layout: Arc::clone(layout),
        active_order_kind: order_kind,
    };
    let result = solve_trade_with_shift_prevalidated(&input, table, 24.0).ok()?;
    let trade_mechanism_signature = trade_room_mechanism_signature(&input.operators);
    let names: Vec<String> = input.operators.iter().map(|op| op.name.clone()).collect();
    let (operator_indices, operator_mask) =
        operator_index_and_mask(&names, operator_index, mask_words);
    Some(BakedComboRow {
        row_id: 0,
        facility: "trade".to_string(),
        signature_key: signature_key.to_string(),
        room_level,
        operator_capacity,
        names,
        operator_indices,
        operator_mask,
        sort_efficiency: result.efficiency.final_efficiency,
        order_kind: Some(order_kind),
        recipe: None,
        trade_mechanism_signature,
        trade_base_efficiency: Some(result.efficiency.paper.base_efficiency),
        trade_occupancy_efficiency: Some(result.efficiency.paper.occupancy_efficiency),
        trade_skill_efficiency: Some(result.efficiency.paper.skill_efficiency),
        trade_control_efficiency: Some(result.efficiency.paper.control_efficiency),
        trade_paper_efficiency: Some(result.efficiency.paper.paper_efficiency),
        trade_mechanic_equivalent_efficiency: Some(
            result.order_mechanic.mechanic_equivalent_efficiency,
        ),
        trade_unit_output_multiplier: Some(
            result.efficiency.production_basis.unit_output_multiplier,
        ),
        trade_final_efficiency: Some(result.efficiency.final_efficiency),
        trade_equivalent_skill_efficiency: Some(result.efficiency.equivalent_skill_efficiency),
        rule_id: result.rule_id.clone(),
        unit_trade_per_day: Some(result.production.unit.unit_trade_per_day),
        unit_gold_per_day: Some(result.production.unit.unit_gold_per_day),
        unit_originium_per_day: Some(result.production.unit.unit_originium_per_day),
        manufacture_base_efficiency: None,
        manufacture_occupancy_efficiency: None,
        manufacture_skill_efficiency: None,
        manufacture_global_efficiency: None,
        manufacture_final_efficiency: None,
        manufacture_storage_limit: None,
    })
}

fn bake_manufacture_rows(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &BakeOptions,
    operator_index: &HashMap<&str, usize>,
    mask_words: usize,
) -> Result<BakedRows> {
    let full_pool = build_manufacture_pool(roster, instances, table)?;
    let layout = Arc::new(LayoutContext::search_baseline());
    let mut all_rows = Vec::new();
    let mut signatures = 0usize;

    for room_level in 1..=3 {
        for recipe in [
            RecipeKind::Gold,
            RecipeKind::BattleRecord,
            RecipeKind::Originium,
        ] {
            let pool = filter_pool_to_names(
                &full_pool,
                standalone_names_for(FacilityKind::Factory, StandaloneFilter::for_recipe(recipe)),
            );
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

            let mut rows: Vec<_> = combinations_indices(pool.entries.len(), operator_capacity)
                .collect::<Vec<_>>()
                .par_iter()
                .filter_map(|combo| {
                    manufacture_combo_row(
                        &pool,
                        table,
                        room_level,
                        operator_capacity,
                        recipe,
                        &layout,
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
    room_level: u8,
    operator_capacity: usize,
    recipe: RecipeKind,
    layout: &Arc<LayoutContext>,
    combo: &[usize],
    signature_key: &str,
    operator_index: &HashMap<&str, usize>,
    mask_words: usize,
) -> Option<BakedComboRow> {
    let operators: Vec<_> = combo
        .iter()
        .map(|idx| pool.entries[*idx].to_manu_operator())
        .collect();
    let names: Vec<String> = operators.iter().map(|op| op.name.clone()).collect();
    let input = ManuRoomInput {
        level: room_level,
        operators,
        active_recipe: recipe,
        mood: 24.0,
        layout: Arc::clone(layout),
    };
    let result = solve_manufacture(&input, table).ok()?;
    let (operator_indices, operator_mask) =
        operator_index_and_mask(&names, operator_index, mask_words);
    Some(BakedComboRow {
        row_id: 0,
        facility: "manufacture".to_string(),
        signature_key: signature_key.to_string(),
        room_level,
        operator_capacity,
        names,
        operator_indices,
        operator_mask,
        sort_efficiency: result.final_efficiency,
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
        rule_id: None,
        unit_trade_per_day: None,
        unit_gold_per_day: None,
        unit_originium_per_day: None,
        manufacture_base_efficiency: Some(result.base_efficiency),
        manufacture_occupancy_efficiency: Some(result.occupancy_efficiency),
        manufacture_skill_efficiency: Some(result.skill_efficiency),
        manufacture_global_efficiency: Some(result.global_efficiency),
        manufacture_final_efficiency: Some(result.final_efficiency),
        manufacture_storage_limit: Some(result.storage_limit),
    })
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
    let (trade_pool, trade_skill_table) = if manifest.options.include_trade {
        let instances = OperatorInstances::load(&default_instances_path()?)?;
        let table = SkillTable::load(&default_skill_table_path()?)?;
        let roster = bake_roster(&instances);
        let trade_pool = build_trade_pool(&roster, &instances, &table)?;
        validate_trade_mechanism_signatures(&disk, &trade_pool)?;
        validate_complete_trade_row_universe(&disk, &trade_pool, &table)?;
        (Some(trade_pool), Some(table))
    } else {
        (None, None)
    };
    Ok(ValidatedBakedCatalog {
        disk,
        trade_pool,
        trade_skill_table,
    })
}

pub fn verify_baked_catalog_responses(
    out_dir: &Path,
    generator: &BakeGeneratorFingerprint,
) -> Result<usize> {
    let validated = load_validated_baked_catalog(out_dir, generator)?;
    let disk = validated.disk;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let roster = bake_roster(&instances);
    let trade_pool = build_trade_pool(&roster, &instances, &table)?;
    let manufacture_pool = build_manufacture_pool(&roster, &instances, &table)?;
    let operator_index: HashMap<&str, usize> = disk
        .operator_names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    let layout = Arc::new(LayoutContext::search_baseline());
    let mut verified = 0usize;

    for index in &disk.indexes {
        let mut sample_offsets = vec![0, index.len / 2, index.len - 1];
        sample_offsets.sort_unstable();
        sample_offsets.dedup();
        for offset in sample_offsets {
            let actual = &disk.rows[index.start + offset];
            let names: Vec<_> = actual
                .operator_indices
                .iter()
                .map(|operator| disk.operator_names[*operator].as_str())
                .collect();
            let expected = if let Some(order) = actual.order_kind {
                let combo = pool_indices_for_names(&trade_pool, &names)?;
                trade_combo_row(
                    &trade_pool,
                    &table,
                    actual.room_level,
                    actual.operator_capacity,
                    order,
                    layout.gold_manu_line_count,
                    &layout,
                    &combo,
                    &index.signature_key,
                    &operator_index,
                    disk.mask_words,
                )
            } else if let Some(recipe) = actual.recipe {
                let pool = filter_pool_to_names(
                    &manufacture_pool,
                    standalone_names_for(
                        FacilityKind::Factory,
                        StandaloneFilter::for_recipe(recipe),
                    ),
                );
                let combo = pool_indices_for_names(&pool, &names)?;
                manufacture_combo_row(
                    &pool,
                    &table,
                    actual.room_level,
                    actual.operator_capacity,
                    recipe,
                    &layout,
                    &combo,
                    &index.signature_key,
                    &operator_index,
                    disk.mask_words,
                )
            } else {
                None
            }
            .ok_or_else(|| {
                Error::msg(format!(
                    "live solver rejected sampled baked row in {:?}",
                    index.signature_key
                ))
            })?;
            if BakedComboRowDisk::from(&expected) != *actual {
                return Err(Error::msg(format!(
                    "baked/live response mismatch in {:?} at row {}",
                    index.signature_key,
                    index.start + offset
                )));
            }
            verified += 1;
        }
    }
    Ok(verified)
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
    Ok(CompleteBakedTradeCatalog {
        disk: validated.disk,
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
            let operators = names
                .iter()
                .map(|name| {
                    self.trade_pool
                        .entry(name)
                        .map(|entry| entry.to_trade_operator())
                        .ok_or_else(|| {
                            Error::msg(format!("baked trade row operator {name:?} missing"))
                        })
                })
                .collect::<Result<Vec<_>>>()?;
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
        let operators = disk_row
            .operator_indices
            .iter()
            .map(|operator| {
                let name = &self.disk.operator_names[*operator];
                self.trade_pool
                    .entry(name)
                    .map(|entry| entry.to_trade_operator())
                    .ok_or_else(|| Error::msg(format!("baked trade row operator {name:?} missing")))
            })
            .collect::<Result<Vec<_>>>()?;
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
                paper_efficiency: efficiency.paper.paper_efficiency,
                mechanic_equivalent_efficiency: result
                    .order_mechanic
                    .mechanic_equivalent_efficiency,
                unit_output_multiplier: efficiency.production_basis.unit_output_multiplier,
                final_efficiency: efficiency.final_efficiency,
                equivalent_skill_efficiency: efficiency.equivalent_skill_efficiency,
                unit_trade_per_day: result.production.unit.unit_trade_per_day,
                unit_gold_per_day: result.production.unit.unit_gold_per_day,
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
    for index in disk
        .indexes
        .iter()
        .filter(|index| index.signature_key.starts_with("trade:"))
    {
        let first = disk
            .rows
            .get(index.start)
            .ok_or_else(|| Error::msg("trade signature has no rows"))?;
        let capacity = first.operator_capacity;
        let mut expected = BTreeSet::new();
        for combo in combinations_indices(trade_pool.entries.len(), capacity) {
            let operators = build_trade_combo_operators_vec(trade_pool, &combo, None, None);
            if trade_station_exclusive_violation(&operators, table) {
                continue;
            }
            let names = operators
                .iter()
                .map(|operator| operator.name.as_str())
                .collect::<Vec<_>>();
            let indices = names
                .iter()
                .map(|name| {
                    disk.operator_names
                        .iter()
                        .position(|candidate| candidate == name)
                        .ok_or_else(|| {
                            Error::msg(format!("trade universe operator {name:?} missing"))
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            expected.insert(indices);
        }
        let actual = disk.rows[index.start..index.start + index.len]
            .iter()
            .map(|row| row.operator_indices.clone())
            .collect::<BTreeSet<_>>();
        if actual != expected || index.len != expected.len() {
            return Err(Error::msg(format!(
                "baked trade row universe is incomplete for {:?}",
                index.signature_key
            )));
        }
    }
    Ok(())
}

fn validate_trade_mechanism_signatures(
    disk: &BakedComboTableDisk,
    trade_pool: &TradePool,
) -> Result<()> {
    for actual in disk.rows.iter().filter(|row| row.order_kind.is_some()) {
        let names: Vec<_> = actual
            .operator_indices
            .iter()
            .map(|operator| disk.operator_names[*operator].as_str())
            .collect();
        let combo = pool_indices_for_names(trade_pool, &names)?;
        let operators = build_trade_combo_operators_vec(trade_pool, &combo, None, None);
        if trade_room_mechanism_signature(&operators) != actual.trade_mechanism_signature {
            return Err(Error::msg(format!(
                "baked trade mechanism signature mismatch for {names:?}"
            )));
        }
    }
    Ok(())
}

fn pool_indices_for_names<T: HasName>(pool: &PoolCore<T>, names: &[&str]) -> Result<Vec<usize>> {
    names
        .iter()
        .map(|name| {
            pool.entries
                .iter()
                .position(|entry| entry.pool_name() == *name)
                .ok_or_else(|| {
                    Error::msg(format!("baked operator {name:?} is absent from live pool"))
                })
        })
        .collect()
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
    if table.operator_count != table.operator_names.len() {
        return Err(Error::msg("baked table operator count mismatch"));
    }
    let unique_names: std::collections::HashSet<_> = table.operator_names.iter().collect();
    if unique_names.len() != table.operator_names.len() {
        return Err(Error::msg("baked table contains duplicate operator names"));
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
            for order in [TradeOrderKind::Gold, TradeOrderKind::Originium] {
                keys.insert(trade_lookup_key(
                    level,
                    station_operator_capacity(level),
                    order,
                    gold_lines,
                ));
            }
        }
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
    let final_efficiency = row
        .trade_final_efficiency_millis
        .or(row.manufacture_final_efficiency_millis)
        .ok_or_else(|| Error::msg(format!("baked row has no final efficiency in {key:?}")))?;
    if row.sort_efficiency_millis != final_efficiency {
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
        && row.unit_trade_per_day.is_some()
        && row.unit_gold_per_day.is_some()
        && row.unit_originium_per_day.is_some()
}

fn manufacture_response_fields_present(row: &BakedComboRowDisk) -> bool {
    row.manufacture_base_efficiency_millis.is_some()
        && row.manufacture_occupancy_efficiency_millis.is_some()
        && row.manufacture_skill_efficiency_millis.is_some()
        && row.manufacture_global_efficiency_millis.is_some()
        && row.manufacture_final_efficiency_millis.is_some()
        && row.manufacture_storage_limit.is_some()
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
        || row.unit_trade_per_day.is_some()
        || row.unit_gold_per_day.is_some()
        || row.unit_originium_per_day.is_some()
}

fn any_manufacture_response_field_present(row: &BakedComboRowDisk) -> bool {
    row.manufacture_base_efficiency_millis.is_some()
        || row.manufacture_occupancy_efficiency_millis.is_some()
        || row.manufacture_skill_efficiency_millis.is_some()
        || row.manufacture_global_efficiency_millis.is_some()
        || row.manufacture_final_efficiency_millis.is_some()
        || row.manufacture_storage_limit.is_some()
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
    _skill_table: &SkillTable,
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
    if !filter.forbidden_pairs.is_empty() {
        return baked_miss(mode, "trade forbidden-pair constraints are not represented");
    }
    if !baked_trade_compatible(pool, options, filter) {
        return baked_miss(mode, trade_incompatibility_reason(pool, options, filter));
    }
    let Some(table) = load_runtime_baked_table()? else {
        return baked_miss(
            mode,
            "catalog is missing, stale, or currently being generated",
        );
    };
    if !baked_table_covers_pool_names(
        &table.operator_index_by_name,
        pool.entries.iter().map(|entry| entry.name.as_str()),
    ) {
        return baked_miss(mode, "catalog does not cover every live candidate");
    }
    let gold_lines = if options.layout.gold_manu_line_count > 0 {
        options.layout.gold_manu_line_count
    } else {
        options.gold_production_lines
    };
    let key = trade_lookup_key(
        options.trade_level,
        options.operator_capacity,
        order_kind,
        gold_lines,
    );
    let Some((start_idx, len)) = table.index_by_key.get(&key).copied() else {
        return baked_miss(mode, format!("signature {key:?} is absent"));
    };
    let available_mask = available_operator_mask(
        &table.operator_index_by_name,
        pool.entries.iter().map(|entry| entry.name.as_str()),
        table.table.mask_words,
    );
    let must_names = filter.must_include_names();
    let mut hits = Vec::new();
    for row in &table.table.rows[start_idx..start_idx + len] {
        if !mask_subset(&row.operator_mask, &available_mask) {
            continue;
        }
        if must_names
            .iter()
            .any(|name| !row.names.iter().any(|row_name| row_name == name))
        {
            continue;
        }
        let hit = row_to_trade_hit(row);
        let Some(hit) = hit else { continue };
        if filter.hit_filter.is_some_and(|f| !f(&hit)) {
            continue;
        }
        hits.push(hit);
        if hits.len() >= options.top_k.max(1) {
            break;
        }
    }
    if hits.is_empty() {
        return baked_miss(mode, format!("signature {key:?} has no eligible row"));
    }
    let best = hits[0].clone();
    let evaluated = hits.len() as u64;
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
    _skill_table: &SkillTable,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
    combinations: u64,
    start: StdInstant,
) -> Result<Option<ManuSearchReport>> {
    let mode = options.bake_mode;
    if mode == BakeMode::Disabled {
        return Ok(None);
    }
    if !baked_manufacture_compatible(pool, options) {
        return baked_miss(mode, manufacture_incompatibility_reason(pool, options));
    }
    let Some(table) = load_runtime_baked_table()? else {
        return baked_miss(
            mode,
            "catalog is missing, stale, or currently being generated",
        );
    };
    if !baked_table_covers_pool_names(
        &table.operator_index_by_name,
        pool.entries.iter().map(|entry| entry.name.as_str()),
    ) {
        return baked_miss(mode, "catalog does not cover every live candidate");
    }
    let key = manufacture_lookup_key(options.level, options.operator_capacity, recipe);
    let Some((start_idx, len)) = table.index_by_key.get(&key).copied() else {
        return baked_miss(mode, format!("signature {key:?} is absent"));
    };
    let available_mask = available_operator_mask(
        &table.operator_index_by_name,
        pool.entries.iter().map(|entry| entry.name.as_str()),
        table.table.mask_words,
    );
    let mut hits = Vec::new();
    for row in &table.table.rows[start_idx..start_idx + len] {
        if !mask_subset(&row.operator_mask, &available_mask) {
            continue;
        }
        if options
            .must_include_name
            .as_ref()
            .is_some_and(|name| !row.names.iter().any(|row_name| row_name == name))
        {
            continue;
        }
        let hit = row_to_manu_hit(row);
        let Some(hit) = hit else { continue };
        hits.push(hit);
        if hits.len() >= options.top_k.max(1) {
            break;
        }
    }
    if hits.is_empty() {
        return baked_miss(mode, format!("signature {key:?} has no eligible row"));
    }
    let best = hits[0].clone();
    let evaluated = hits.len() as u64;
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

pub fn warm_runtime_baked_table() -> Result<bool> {
    Ok(load_runtime_baked_table()?.is_some())
}

static RUNTIME_BAKE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn set_runtime_bake_in_progress(in_progress: bool) {
    RUNTIME_BAKE_IN_PROGRESS.store(in_progress, Ordering::Release);
}

fn load_runtime_baked_table() -> Result<Option<&'static RuntimeBakedComboTable>> {
    // The background baker writes several files into the catalog directory. Do
    // not cache or consume a catalog until that write set is complete.
    if RUNTIME_BAKE_IN_PROGRESS.load(Ordering::Acquire) {
        return Ok(None);
    }
    static CACHE: OnceLock<Option<RuntimeBakedComboTable>> = OnceLock::new();
    if let Some(table) = CACHE.get() {
        return Ok(table.as_ref());
    }
    match load_runtime_baked_table_inner() {
        Ok(table) => {
            let _ = CACHE.set(Some(table));
            Ok(CACHE.get().and_then(|t| t.as_ref()))
        }
        Err(_) => Ok(None),
    }
}

fn load_runtime_baked_table_inner() -> Result<RuntimeBakedComboTable> {
    let Ok(manifest_path) = data_path("baked/manifest.json") else {
        return Err(Error::msg("data/baked/manifest.json not found"));
    };
    let out_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("data/baked"));
    let generator = current_exe_generator_fingerprint()?;
    let disk = load_validated_baked_catalog(&out_dir, &generator)?.disk;
    let mut table = BakedComboTable::from(disk);
    let mut index_by_key = HashMap::new();
    for index in &table.indexes {
        index_by_key.insert(index.signature_key.clone(), (index.start, index.len));
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
    let operator_index_by_name = table
        .operator_names
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.clone(), idx))
        .collect();
    Ok(RuntimeBakedComboTable {
        table,
        index_by_key,
        operator_index_by_name,
    })
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

fn current_exe_generator_fingerprint() -> Result<BakeGeneratorFingerprint> {
    let path = std::env::current_exe()?;
    let bytes = fs::read(&path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(BakeGeneratorFingerprint {
        kind: "infra-cli-exe".to_string(),
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len() as u64,
        hash64: format!("{:016x}", hasher.finish()),
    })
}

fn baked_trade_compatible(
    pool: &TradePool,
    options: &TradeSearchOptions,
    filter: &SearchTripleFilter,
) -> bool {
    filter.must_operator_override.is_none()
        && (options.mood - 24.0).abs() < f64::EPSILON
        && (options.shift_hours - 24.0).abs() < f64::EPSILON
        && baked_trade_layout_compatible(&options.layout)
        && !trade_pool_requires_candidate_projection(pool)
        && pool.entries.iter().all(|entry| entry.progress.elite >= 2)
}

fn trade_incompatibility_reason(
    pool: &TradePool,
    options: &TradeSearchOptions,
    filter: &SearchTripleFilter,
) -> &'static str {
    if filter.must_operator_override.is_some() {
        "operator override is not represented"
    } else if (options.mood - 24.0).abs() >= f64::EPSILON {
        "mood is not 24"
    } else if (options.shift_hours - 24.0).abs() >= f64::EPSILON {
        "shift duration is not 24 hours"
    } else if !baked_trade_layout_compatible(&options.layout) {
        "layout or trade inject differs from the baked baseline"
    } else if trade_pool_requires_candidate_projection(pool) {
        "candidate projection is required"
    } else {
        "the candidate pool contains an unsupported tier"
    }
}

fn baked_manufacture_compatible(pool: &ManuPool, options: &ManuSearchOptions) -> bool {
    !options.full_pool
        && (options.mood - 24.0).abs() < f64::EPSILON
        && options.shift_hours.is_none()
        && baked_manufacture_layout_compatible(&options.layout)
        && pool.entries.iter().all(|entry| entry.progress.elite >= 2)
}

fn manufacture_incompatibility_reason(
    pool: &ManuPool,
    options: &ManuSearchOptions,
) -> &'static str {
    if options.full_pool {
        "full manufacture pool is not represented"
    } else if (options.mood - 24.0).abs() >= f64::EPSILON {
        "mood is not 24"
    } else if options.shift_hours.is_some() {
        "timed manufacture settlement is not represented"
    } else if !baked_manufacture_layout_compatible(&options.layout) {
        "layout or manufacture inject differs from the baked baseline"
    } else if pool.entries.iter().any(|entry| entry.progress.elite < 2) {
        "the candidate pool contains an unsupported tier"
    } else {
        "query is incompatible with the baked model"
    }
}

fn baked_search_baseline() -> &'static LayoutContext {
    static BASELINE: OnceLock<LayoutContext> = OnceLock::new();
    BASELINE.get_or_init(LayoutContext::search_baseline)
}

fn baked_layout_common_compatible(layout: &LayoutContext) -> bool {
    let mut actual = layout.clone();
    actual.global_inject = Default::default();
    let mut baseline = baked_search_baseline().clone();
    baseline.global_inject = Default::default();
    actual == baseline
}

fn baked_trade_layout_compatible(layout: &LayoutContext) -> bool {
    let baseline = baked_search_baseline();
    baked_layout_common_compatible(layout)
        && layout
            .global_inject
            .same_trade_effects_as(&baseline.global_inject)
}

fn baked_manufacture_layout_compatible(layout: &LayoutContext) -> bool {
    let baseline = baked_search_baseline();
    baked_layout_common_compatible(layout)
        && layout
            .global_inject
            .same_manufacture_effects_as(&baseline.global_inject)
}

fn baked_table_covers_pool_names<'a>(
    operator_index_by_name: &HashMap<String, usize>,
    mut names: impl Iterator<Item = &'a str>,
) -> bool {
    names.all(|name| operator_index_by_name.contains_key(name))
}

fn available_operator_mask<'a>(
    operator_index_by_name: &HashMap<String, usize>,
    names: impl Iterator<Item = &'a str>,
    mask_words: usize,
) -> Vec<u64> {
    let mut mask = vec![0u64; mask_words];
    for name in names {
        let Some(idx) = operator_index_by_name.get(name).copied() else {
            continue;
        };
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(slot) = mask.get_mut(word) {
            *slot |= 1u64 << bit;
        }
    }
    mask
}

fn mask_subset(row_mask: &[u64], available_mask: &[u64]) -> bool {
    row_mask.iter().enumerate().all(|(idx, word)| {
        let available = available_mask.get(idx).copied().unwrap_or(0);
        word & !available == 0
    })
}

fn row_to_trade_hit(row: &BakedComboRow) -> Option<TradeSearchHit> {
    if row.facility != "trade" {
        return None;
    }
    let final_efficiency = row.trade_final_efficiency?;
    let mechanic_equivalent_efficiency = row.trade_mechanic_equivalent_efficiency?;
    let unit_trade_per_day = row.unit_trade_per_day.unwrap_or(0.0);
    let unit_gold_per_day = row.unit_gold_per_day.unwrap_or(0.0);
    Some(TradeSearchHit {
        names: row.names.clone(),
        gold_names: vec![],
        originium_names: vec![],
        final_efficiency,
        mechanic_equivalent_efficiency,
        rule_id: row.rule_id.clone(),
        unit_trade_per_day,
        unit_gold_per_day,
        unit_originium_per_day: row.unit_originium_per_day.unwrap_or(0.0),
        breakdown: Some(TradeEfficiencyBreakdown {
            base_efficiency: {
                let base = row.trade_base_efficiency?;
                if base == Efficiency::ONE {
                    base + row.trade_occupancy_efficiency?
                } else {
                    base
                }
            },
            occupancy_efficiency: row.trade_occupancy_efficiency?,
            skill_efficiency: row.trade_skill_efficiency?,
            control_efficiency: row.trade_control_efficiency?,
            paper_efficiency: row.trade_paper_efficiency?,
            mechanic_equivalent_efficiency,
            unit_output_multiplier: row.trade_unit_output_multiplier?,
            final_efficiency,
            equivalent_skill_efficiency: row.trade_equivalent_skill_efficiency?,
            unit_trade_per_day,
            unit_gold_per_day,
            rule_id: row.rule_id.clone(),
            ..TradeEfficiencyBreakdown::default()
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
            base_efficiency: {
                let base = row.manufacture_base_efficiency?;
                if base == Efficiency::ONE {
                    base + row.manufacture_occupancy_efficiency?
                } else {
                    base
                }
            },
            occupancy_efficiency: row.manufacture_occupancy_efficiency?,
            skill_efficiency: row.manufacture_skill_efficiency?,
            global_efficiency: row.manufacture_global_efficiency?,
            final_efficiency,
            storage_limit,
            recipe: recipe_label.to_string(),
            ..ManuEfficiencyBreakdown::default()
        },
    })
}

fn build_combo_table_from_rows(
    operator_names: Vec<String>,
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
        operator_count: operator_names.len(),
        operator_names,
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

fn operator_index_and_mask(
    names: &[String],
    operator_index: &HashMap<&str, usize>,
    mask_words: usize,
) -> (Vec<usize>, Vec<u64>) {
    let indices: Vec<_> = names
        .iter()
        .filter_map(|name| operator_index.get(name.as_str()).copied())
        .collect();

    let mut mask = vec![0u64; mask_words];
    for idx in &indices {
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(slot) = mask.get_mut(word) {
            *slot |= 1u64 << bit;
        }
    }
    (indices, mask)
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

fn bake_roster(instances: &OperatorInstances) -> Roster {
    let mut roster = Roster::default();
    for name in bake_operator_names(instances) {
        let has_production_binding = instances
            .get(&name, crate::tier::PromotionTier::TierUp)
            .is_some_and(|i| {
                i.facilities.contains_key("trade") || i.facilities.contains_key("manufacture")
            })
            || instances
                .get(&name, crate::tier::PromotionTier::Tier0)
                .is_some_and(|i| {
                    i.facilities.contains_key("trade") || i.facilities.contains_key("manufacture")
                });
        if !has_production_binding {
            continue;
        }
        roster.insert(name, OperatorProgress::new(2, 1, 6));
    }
    roster
}

fn station_operator_capacity(level: u8) -> usize {
    level.clamp(1, 3) as usize
}

fn baked_operators(instances: &OperatorInstances, roster: &Roster) -> Vec<BakedOperator> {
    let mut operators: Vec<_> = roster
        .names()
        .map(|name| {
            let mut facilities = Vec::new();
            for facility in ["trade", "manufacture"] {
                if instances
                    .get(name, crate::tier::PromotionTier::TierUp)
                    .and_then(|i| i.facilities.get(facility))
                    .is_some()
                    || instances
                        .get(name, crate::tier::PromotionTier::Tier0)
                        .and_then(|i| i.facilities.get(facility))
                        .is_some()
                {
                    facilities.push(facility.to_string());
                }
            }
            BakedOperator {
                name: name.clone(),
                progress: roster
                    .progress(name)
                    .unwrap_or_else(|| OperatorProgress::new(2, 1, 6))
                    .into(),
                facilities,
            }
        })
        .collect();
    operators.sort_by(|a, b| a.name.cmp(&b.name));
    operators
}

fn bake_operator_names(instances: &OperatorInstances) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (_, instance) in instances.iter() {
        if instance.facilities.contains_key("trade") {
            names.insert(instance.name.clone());
        }
    }
    for recipe in [
        RecipeKind::Gold,
        RecipeKind::BattleRecord,
        RecipeKind::Originium,
    ] {
        if let Some(set) =
            standalone_names_for(FacilityKind::Factory, StandaloneFilter::for_recipe(recipe))
        {
            names.extend(set);
        }
    }
    names
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{solve_control, ControlOperator, ControlRoomInput};
    use crate::instances::resolve_buff_ids;
    use crate::tier::PromotionTier;

    fn complete_trade_catalog() -> &'static CompleteBakedTradeCatalog {
        static CATALOG: OnceLock<CompleteBakedTradeCatalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            let out_dir = std::env::temp_dir().join(format!(
                "infra-bake-complete-trade-rows-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&out_dir);
            let generator = BakeGeneratorFingerprint {
                kind: "test".to_string(),
                path: "test".to_string(),
                bytes: 1,
                hash64: "complete-trade-rows".to_string(),
            };
            bake_catalogs(&BakeOptions {
                out_dir: out_dir.clone(),
                include_trade: true,
                include_manufacture: false,
                generator: Some(generator.clone()),
                ..Default::default()
            })
            .unwrap();
            let catalog = load_complete_baked_trade_catalog(&out_dir, &generator).unwrap();
            fs::remove_dir_all(out_dir).unwrap();
            catalog
        })
    }

    fn control_op(name: &str) -> ControlOperator {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let tier = PromotionTier::TierUp;
        let binding = instances
            .get(name, tier)
            .and_then(|instance| instance.facilities.get("control"));
        let tier0 = instances
            .get(name, PromotionTier::Tier0)
            .and_then(|instance| instance.facilities.get("control"));
        let buff_ids = binding
            .map(|binding| resolve_buff_ids(tier, binding, tier0))
            .unwrap_or_default();
        let tags = instances
            .get(name, tier)
            .map(|instance| instance.tags.clone())
            .unwrap_or_default();
        ControlOperator {
            name: name.to_string(),
            elite: 2,
            buff_ids,
            tags,
        }
    }

    fn layout_with_control_producer(
        catalog: &CompleteBakedTradeCatalog,
        name: &str,
        mut layout: LayoutContext,
    ) -> Arc<LayoutContext> {
        let result = solve_control(
            &ControlRoomInput {
                operators: vec![control_op(name)],
                mood: 24.0,
                layout: layout.clone(),
            },
            &catalog.skill_table,
        );
        layout.global = result.global;
        layout.global_inject = result.inject;
        Arc::new(layout)
    }

    fn control_efficiency(hit: &TradeSearchHit) -> Efficiency {
        hit.breakdown.as_ref().unwrap().control_efficiency
    }

    fn trade_ops(names: &[&str]) -> Vec<TradeOperator> {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster =
            Roster::from_elite_map(names.iter().map(|name| ((*name).to_string(), 2)).collect());
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        names
            .iter()
            .map(|name| pool.entry(name).unwrap().to_trade_operator())
            .collect()
    }

    fn glasgow_peer(name: &str) -> TradeOperator {
        TradeOperator {
            name: name.to_string(),
            tags: vec!["cc.g.glasgow".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn trade_mechanism_signature_extracts_only_observed_room_state() {
        let vina_alone = trade_room_mechanism_signature(&trade_ops(&["维娜·维多利亚"]));
        assert_eq!(
            vina_alone,
            Some(TradeRoomMechanismSignature {
                glasgow_peer_present: Some(false),
                ..Default::default()
            })
        );
        let mut vina_one_peer_ops = trade_ops(&["维娜·维多利亚"]);
        vina_one_peer_ops.push(glasgow_peer("glasgow_peer_a"));
        let vina_one_peer = trade_room_mechanism_signature(&vina_one_peer_ops);
        let mut vina_two_peer_ops = vina_one_peer_ops;
        vina_two_peer_ops.push(glasgow_peer("glasgow_peer_b"));
        let vina_two_peers = trade_room_mechanism_signature(&vina_two_peer_ops);
        assert_eq!(
            vina_one_peer,
            Some(TradeRoomMechanismSignature {
                glasgow_peer_present: Some(true),
                ..Default::default()
            })
        );
        assert_eq!(vina_two_peers, vina_one_peer);

        let morgan_alone = trade_room_mechanism_signature(&trade_ops(&["摩根"]));
        assert_eq!(
            morgan_alone,
            Some(TradeRoomMechanismSignature {
                glasgow_count: Some(1),
                king_of_victoria_present: Some(false),
                ..Default::default()
            })
        );
        let mut morgan_two_ops = trade_ops(&["摩根"]);
        morgan_two_ops.push(glasgow_peer("glasgow_peer"));
        let morgan_two = trade_room_mechanism_signature(&morgan_two_ops);
        assert_eq!(
            morgan_two,
            Some(TradeRoomMechanismSignature {
                glasgow_count: Some(2),
                king_of_victoria_present: Some(false),
                ..Default::default()
            })
        );
        let mut morgan_three_ops = trade_ops(&["摩根"]);
        morgan_three_ops.push(glasgow_peer("推进之王"));
        morgan_three_ops.push(glasgow_peer("glasgow_peer"));
        let morgan_three = trade_room_mechanism_signature(&morgan_three_ops);
        assert_eq!(
            morgan_three,
            Some(TradeRoomMechanismSignature {
                glasgow_count: Some(3),
                king_of_victoria_present: Some(true),
                ..Default::default()
            })
        );

        let firewhistle = trade_room_mechanism_signature(&trade_ops(&["火哨", "古米", "夜刀"]));
        assert_eq!(
            firewhistle,
            Some(TradeRoomMechanismSignature {
                peer_count: Some(2),
                ..Default::default()
            })
        );

        let zilan = trade_room_mechanism_signature(&trade_ops(&["焰狐龙梓兰", "古米"]));
        assert_eq!(
            zilan,
            Some(TradeRoomMechanismSignature {
                snhunt_count: Some(1),
                ..Default::default()
            })
        );

        let exusiai = trade_room_mechanism_signature(&trade_ops(&["新约能天使", "古米"]));
        assert_eq!(
            exusiai,
            Some(TradeRoomMechanismSignature {
                laterano_count: Some(1),
                ..Default::default()
            })
        );

        assert_eq!(trade_room_mechanism_signature(&trade_ops(&["古米"])), None);
    }

    #[test]
    fn catalog_validation_rejects_semantically_wrong_trade_mechanism_signature() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("维娜·维多利亚".to_string(), 2), ("古米".to_string(), 2)]
                .into_iter()
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let names = ["维娜·维多利亚", "古米"];
        let combo = pool_indices_for_names(&pool, &names).unwrap();
        let operator_names = names
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        let operator_index = operator_names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), index))
            .collect::<HashMap<_, _>>();
        let layout = Arc::new(LayoutContext::search_baseline());
        let key = trade_lookup_key(2, 2, TradeOrderKind::Gold, layout.gold_manu_line_count);
        let row = trade_combo_row(
            &pool,
            &table,
            2,
            2,
            TradeOrderKind::Gold,
            layout.gold_manu_line_count,
            &layout,
            &combo,
            &key,
            &operator_index,
            1,
        )
        .unwrap();
        let table = build_combo_table_from_rows(operator_names, 1, vec![row], None);
        let mut disk = BakedComboTableDisk::from(&table);
        disk.rows[0].trade_mechanism_signature = Some(TradeRoomMechanismSignature {
            glasgow_peer_present: Some(true),
            ..Default::default()
        });

        let err = validate_trade_mechanism_signatures(&disk, &pool).unwrap_err();
        assert!(err
            .to_string()
            .contains("baked trade mechanism signature mismatch"));
    }

    #[test]
    fn complete_trade_row_adapter_returns_untruncated_rows_and_faction_counts() {
        let catalog = complete_trade_catalog();
        let rows = catalog.rows_for(3, TradeOrderKind::Gold).unwrap();
        assert!(
            rows.rows.len() > 20,
            "complete adapter must not apply top_k"
        );
        assert!(rows.rows.iter().any(|row| row.siracusa_count > 0));
        assert!(rows.rows.iter().any(|row| row.glasgow_count > 0));
        assert!(rows.rows.iter().any(|row| row.karlan_count >= 3));
        assert!(rows.rows.iter().all(|row| {
            row.names.len() == 3
                && !row.logical_mask.is_empty()
                && row.baseline_response.names == row.names
        }));
    }

    #[test]
    fn complete_trade_row_live_resolve_matches_baked_baseline_response() {
        let catalog = complete_trade_catalog();
        let rows = catalog.rows_for(3, TradeOrderKind::Gold).unwrap();
        let row = rows.rows.iter().find(|row| row.glasgow_count == 2).unwrap();

        let resolved = catalog
            .resolve_row_live(
                &row.row_id,
                3,
                TradeOrderKind::Gold,
                Arc::new(LayoutContext::search_baseline()),
                24.0,
                24.0,
            )
            .unwrap();

        assert_eq!(
            serde_json::to_value(resolved).unwrap(),
            serde_json::to_value(&row.baseline_response).unwrap()
        );
    }

    #[test]
    fn complete_trade_row_live_resolve_observes_dynamic_producer_scopes() {
        let catalog = complete_trade_catalog();
        let rows = catalog.rows_for(3, TradeOrderKind::Gold).unwrap();
        let plain_row = rows
            .rows
            .iter()
            .find(|row| row.siracusa_count == 0 && row.glasgow_count == 0)
            .unwrap();
        let glasgow_row = rows.rows.iter().find(|row| row.glasgow_count == 2).unwrap();

        let baseline_plain = catalog
            .resolve_row_live(
                &plain_row.row_id,
                3,
                TradeOrderKind::Gold,
                Arc::new(LayoutContext::search_baseline()),
                24.0,
                24.0,
            )
            .unwrap();
        let baseline_glasgow = catalog
            .resolve_row_live(
                &glasgow_row.row_id,
                3,
                TradeOrderKind::Gold,
                Arc::new(LayoutContext::search_baseline()),
                24.0,
                24.0,
            )
            .unwrap();

        let mut haru_layout = LayoutContext::search_baseline();
        haru_layout
            .trade_tagged_count_sum
            .insert("cc.g.siracusa".to_string(), 2);
        let haru = catalog
            .resolve_row_live(
                &plain_row.row_id,
                3,
                TradeOrderKind::Gold,
                layout_with_control_producer(catalog, "八幡海铃", haru_layout),
                24.0,
                24.0,
            )
            .unwrap();
        assert_eq!(
            control_efficiency(&haru) - control_efficiency(&baseline_plain),
            Efficiency::from_decimal(0.10),
            "Haru must consume the all-trade Siracusa total"
        );

        let mut daifeen_plain_layout = LayoutContext::search_baseline();
        daifeen_plain_layout
            .trade_tagged_count_sum
            .insert("cc.g.glasgow".to_string(), 3);
        let daifeen_plain = catalog
            .resolve_row_live(
                &plain_row.row_id,
                3,
                TradeOrderKind::Gold,
                layout_with_control_producer(catalog, "戴菲恩", daifeen_plain_layout),
                24.0,
                24.0,
            )
            .unwrap();
        assert_eq!(
            control_efficiency(&daifeen_plain),
            control_efficiency(&baseline_plain),
            "Daifeen must ignore Glasgow operators in other trade rooms"
        );
        let daifeen_glasgow = catalog
            .resolve_row_live(
                &glasgow_row.row_id,
                3,
                TradeOrderKind::Gold,
                layout_with_control_producer(catalog, "戴菲恩", LayoutContext::search_baseline()),
                24.0,
                24.0,
            )
            .unwrap();
        assert_eq!(
            control_efficiency(&daifeen_glasgow) - control_efficiency(&baseline_glasgow),
            Efficiency::from_decimal(0.20),
            "Daifeen must count Glasgow operators in the resolved room"
        );

        let mut silverash_layout = LayoutContext::search_baseline();
        silverash_layout.trade_stations_tagged_gte.insert(
            crate::layout::trade_station_tagged_gte_key("cc.g.karlan", 3),
            2,
        );
        let silverash = catalog
            .resolve_row_live(
                &plain_row.row_id,
                3,
                TradeOrderKind::Gold,
                layout_with_control_producer(catalog, "凛御银灰", silverash_layout),
                24.0,
                24.0,
            )
            .unwrap();
        assert_eq!(
            control_efficiency(&silverash) - control_efficiency(&baseline_plain),
            Efficiency::from_decimal(0.20),
            "SilverAsh must consume the number of qualifying Karlan stations"
        );
    }

    #[test]
    fn complete_trade_row_validation_rejects_a_synchronously_removed_row() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = bake_roster(&instances);
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let operators = baked_operators(&instances, &roster);
        let names = operators
            .iter()
            .map(|operator| operator.name.clone())
            .collect::<Vec<_>>();
        let operator_index = operators
            .iter()
            .enumerate()
            .map(|(index, operator)| (operator.name.as_str(), index))
            .collect::<HashMap<_, _>>();
        let rows = bake_trade_rows(
            &roster,
            &instances,
            &table,
            &BakeOptions::default(),
            &operator_index,
            operators.len().div_ceil(64).max(1),
        )
        .unwrap();
        let combo = build_combo_table_from_rows(
            names,
            operator_index.len().div_ceil(64).max(1),
            rows.rows,
            None,
        );
        let mut disk = BakedComboTableDisk::from(&combo);
        let first_trade = disk.indexes.first_mut().unwrap();
        disk.rows.remove(first_trade.start);
        first_trade.len -= 1;
        for index in disk.indexes.iter_mut().skip(1) {
            index.start -= 1;
        }

        let err = validate_complete_trade_row_universe(&disk, &pool, &table).unwrap_err();
        assert!(err
            .to_string()
            .contains("baked trade row universe is incomplete"));
    }

    #[test]
    fn required_bake_rejects_incompatible_trade_query() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = TradePool {
            entries: vec![],
            skipped: vec![],
        };
        let err = try_baked_trade_search(
            &pool,
            &table,
            &TradeSearchOptions {
                shift_hours: 12.0,
                bake_mode: BakeMode::Required,
                ..Default::default()
            },
            TradeOrderKind::Gold,
            &SearchTripleFilter::default(),
            0,
            StdInstant::now(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("shift duration is not 24 hours"));
    }

    #[test]
    fn required_bake_rejects_full_manufacture_pool() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };
        let err = try_baked_manufacture_search(
            &pool,
            &table,
            &ManuSearchOptions {
                full_pool: true,
                bake_mode: BakeMode::Required,
                ..Default::default()
            },
            RecipeKind::Gold,
            0,
            StdInstant::now(),
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("full manufacture pool is not represented"));
    }

    #[test]
    fn auto_bake_preserves_incompatible_query_fallback() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };
        let result = try_baked_manufacture_search(
            &pool,
            &table,
            &ManuSearchOptions {
                full_pool: true,
                ..Default::default()
            },
            RecipeKind::Gold,
            0,
            StdInstant::now(),
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn baked_table_must_cover_every_candidate_name() {
        let operator_index_by_name = HashMap::from([
            ("Miss.Christine".to_string(), 0usize),
            ("酒神".to_string(), 1usize),
            ("弑君者".to_string(), 2usize),
        ]);

        assert!(baked_table_covers_pool_names(
            &operator_index_by_name,
            ["Miss.Christine", "酒神"].into_iter()
        ));
        assert!(
            !baked_table_covers_pool_names(
                &operator_index_by_name,
                ["Miss.Christine", "酒神", "红云"].into_iter()
            ),
            "baked search must fall back when a stale catalog omits a live candidate"
        );
    }

    #[test]
    fn bake_manifest_inputs_include_search_baseline_layout() {
        let inputs = bake_input_fingerprints().unwrap();
        assert!(inputs.iter().any(|input| {
            input
                .path
                .replace('\\', "/")
                .ends_with("layout/243_use_this_.json")
        }));
    }

    #[test]
    fn assignment_full_manufacture_pool_structurally_rejects_standalone_bake() {
        let pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };
        assert!(baked_manufacture_compatible(
            &pool,
            &ManuSearchOptions::default()
        ));
        assert!(!baked_manufacture_compatible(
            &pool,
            &ManuSearchOptions {
                full_pool: true,
                ..Default::default()
            }
        ));
        assert!(!baked_manufacture_compatible(
            &pool,
            &ManuSearchOptions {
                shift_hours: Some(24.0),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn dynamic_trade_inject_rejects_only_trade_bake() {
        let mut layout = LayoutContext::search_baseline();
        layout.global_inject.record_trade_tagged(
            "八幡海铃",
            "control_tra_limit&spd2[000]",
            "dynamic_test",
            "cc.g.siracusa",
            5.0,
            0,
            crate::global_resource::TradeTaggedCountScope::AllTradeRooms,
        );
        let trade_pool = TradePool {
            entries: vec![],
            skipped: vec![],
        };
        assert!(!baked_trade_compatible(
            &trade_pool,
            &TradeSearchOptions {
                layout: Arc::new(layout.clone()),
                ..Default::default()
            },
            &SearchTripleFilter::default(),
        ));
        let manu_pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };
        assert!(baked_manufacture_compatible(
            &manu_pool,
            &ManuSearchOptions {
                layout: Arc::new(layout),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn static_injects_only_reject_their_own_facility_bake() {
        let trade_pool = TradePool {
            entries: vec![],
            skipped: vec![],
        };
        let manu_pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };

        let mut trade_layout = LayoutContext::search_baseline();
        trade_layout
            .global_inject
            .record_trade("bake_trade_test", 1.0);
        assert!(!baked_trade_compatible(
            &trade_pool,
            &TradeSearchOptions {
                layout: Arc::new(trade_layout.clone()),
                ..Default::default()
            },
            &SearchTripleFilter::default(),
        ));
        assert!(baked_manufacture_compatible(
            &manu_pool,
            &ManuSearchOptions {
                layout: Arc::new(trade_layout),
                ..Default::default()
            }
        ));

        let mut manu_layout = LayoutContext::search_baseline();
        manu_layout
            .global_inject
            .record_manu("bake_manu_test", Some(RecipeKind::Gold), 1.0);
        assert!(baked_trade_compatible(
            &trade_pool,
            &TradeSearchOptions {
                layout: Arc::new(manu_layout.clone()),
                ..Default::default()
            },
            &SearchTripleFilter::default(),
        ));
        assert!(!baked_manufacture_compatible(
            &manu_pool,
            &ManuSearchOptions {
                layout: Arc::new(manu_layout),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn non_baseline_common_layout_rejects_both_facility_bakes() {
        let mut layout = LayoutContext::search_baseline();
        layout.base_workforce.push("额外干员".to_string());
        let trade_pool = TradePool {
            entries: vec![],
            skipped: vec![],
        };
        let manu_pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };
        assert!(!baked_trade_compatible(
            &trade_pool,
            &TradeSearchOptions {
                layout: Arc::new(layout.clone()),
                ..Default::default()
            },
            &SearchTripleFilter::default(),
        ));
        assert!(!baked_manufacture_compatible(
            &manu_pool,
            &ManuSearchOptions {
                layout: Arc::new(layout),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn trade_producer_flag_rejects_only_trade_bake() {
        let mut layout = LayoutContext::search_baseline();
        layout
            .global_inject
            .record_active_source_buff("control_tra_limit&spd2[000]");
        let trade_pool = TradePool {
            entries: vec![],
            skipped: vec![],
        };
        let manu_pool = ManuPool {
            entries: vec![],
            skipped: vec![],
        };
        assert!(!baked_trade_compatible(
            &trade_pool,
            &TradeSearchOptions {
                layout: Arc::new(layout.clone()),
                ..Default::default()
            },
            &SearchTripleFilter::default(),
        ));
        assert!(baked_manufacture_compatible(
            &manu_pool,
            &ManuSearchOptions {
                layout: Arc::new(layout),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn operator_in_base_trade_pool_rejects_legacy_bake_precisely() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let sensitive = build_trade_pool(
            &Roster::from_elite_map([("贝洛内".to_string(), 2)].into_iter().collect()),
            &instances,
            &table,
        )
        .unwrap();
        assert!(trade_pool_requires_candidate_projection(&sensitive));

        let plain = build_trade_pool(
            &Roster::from_elite_map([("古米".to_string(), 2)].into_iter().collect()),
            &instances,
            &table,
        )
        .unwrap();
        assert!(!trade_pool_requires_candidate_projection(&plain));
    }
}
