use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use std::time::Instant as StdInstant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::layout::LayoutContext;
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::solver::solve_manufacture;
use crate::pool::{
    build_manufacture_pool, build_trade_combo_operators_vec, build_trade_pool,
    combinations_indices, n_choose_k_u64,
};
use crate::pool::{standalone_names_for, StandaloneFilter};
use crate::pool::{HasName, ManuPool, PoolCore, TradePool};
use crate::roster::{OperatorProgress, Roster};
use crate::search::{
    ManuScoreBreakdown, ManuSearchHit, ManuSearchOptions, ManuSearchReport, SearchTripleFilter,
    TradeScoreBreakdown, TradeSearchHit, TradeSearchOptions, TradeSearchReport,
};
use crate::skill_table::{data_path, default_skill_table_path, SkillTable};
use crate::trade::input::{TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use crate::trade::shortcut::trade_station_exclusive_violation;
use crate::trade::solver::solve_trade_with_shift_prevalidated;
use crate::types::RecipeKind;
use crate::FacilityKind;

pub const BAKE_SCHEMA_VERSION: u32 = 5;

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
    pub manufacture_signatures: usize,
    pub manufacture_hits: usize,
    pub combo_table_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<BakeGeneratorFingerprint>,
    pub elapsed_ms: u128,
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

#[derive(Debug, Serialize, Deserialize)]
struct BakedComboRowDisk {
    room_level: u8,
    operator_capacity: usize,
    operator_indices: Vec<usize>,
    sort_score: f64,
    order_kind: Option<TradeOrderKind>,
    recipe: Option<RecipeKind>,
    trade_pct: Option<f64>,
    gold_pct: Option<f64>,
    shortcut: Option<String>,
    unit_trade_per_day: Option<f64>,
    unit_gold_per_day: Option<f64>,
    unit_originium_per_day: Option<f64>,
    output_multiplier: Option<f64>,
    trade_order_eff_base: Option<f64>,
    trade_order_eff_skill: Option<f64>,
    trade_order_eff_global: Option<f64>,
    trade_effective_eff_multiplier: Option<f64>,
    manu_prod_total: Option<f64>,
    manu_prod_base: Option<f64>,
    manu_prod_skill: Option<f64>,
    manu_prod_global: Option<f64>,
    manu_storage_limit: Option<i32>,
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
            sort_score: value.sort_score,
            order_kind: value.order_kind,
            recipe: value.recipe,
            trade_pct: value.trade_pct,
            gold_pct: value.gold_pct,
            shortcut: value.shortcut.clone(),
            unit_trade_per_day: value.unit_trade_per_day,
            unit_gold_per_day: value.unit_gold_per_day,
            unit_originium_per_day: value.unit_originium_per_day,
            output_multiplier: value.output_multiplier,
            trade_order_eff_base: value.trade_order_eff_base,
            trade_order_eff_skill: value.trade_order_eff_skill,
            trade_order_eff_global: value.trade_order_eff_global,
            trade_effective_eff_multiplier: value.trade_effective_eff_multiplier,
            manu_prod_total: value.manu_prod_total,
            manu_prod_base: value.manu_prod_base,
            manu_prod_skill: value.manu_prod_skill,
            manu_prod_global: value.manu_prod_global,
            manu_storage_limit: value.manu_storage_limit,
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
            sort_score: value.sort_score,
            order_kind: value.order_kind,
            recipe: value.recipe,
            trade_pct: value.trade_pct,
            gold_pct: value.gold_pct,
            shortcut: value.shortcut,
            unit_trade_per_day: value.unit_trade_per_day,
            unit_gold_per_day: value.unit_gold_per_day,
            unit_originium_per_day: value.unit_originium_per_day,
            output_multiplier: value.output_multiplier,
            trade_order_eff_base: value.trade_order_eff_base,
            trade_order_eff_skill: value.trade_order_eff_skill,
            trade_order_eff_global: value.trade_order_eff_global,
            trade_effective_eff_multiplier: value.trade_effective_eff_multiplier,
            manu_prod_total: value.manu_prod_total,
            manu_prod_base: value.manu_prod_base,
            manu_prod_skill: value.manu_prod_skill,
            manu_prod_global: value.manu_prod_global,
            manu_storage_limit: value.manu_storage_limit,
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
    sort_score: f64,
    #[serde(rename = "ok", skip_serializing_if = "Option::is_none")]
    order_kind: Option<TradeOrderKind>,
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    recipe: Option<RecipeKind>,
    #[serde(rename = "tp", skip_serializing_if = "Option::is_none")]
    trade_pct: Option<f64>,
    #[serde(rename = "gp", skip_serializing_if = "Option::is_none")]
    gold_pct: Option<f64>,
    #[serde(rename = "sc", skip_serializing_if = "Option::is_none")]
    shortcut: Option<String>,
    #[serde(rename = "utd", skip_serializing_if = "Option::is_none")]
    unit_trade_per_day: Option<f64>,
    #[serde(rename = "ugd", skip_serializing_if = "Option::is_none")]
    unit_gold_per_day: Option<f64>,
    #[serde(rename = "uod", default, skip_serializing_if = "Option::is_none")]
    unit_originium_per_day: Option<f64>,
    #[serde(rename = "out", default, skip_serializing_if = "Option::is_none")]
    output_multiplier: Option<f64>,
    #[serde(rename = "teb", default, skip_serializing_if = "Option::is_none")]
    trade_order_eff_base: Option<f64>,
    #[serde(rename = "tes", default, skip_serializing_if = "Option::is_none")]
    trade_order_eff_skill: Option<f64>,
    #[serde(rename = "teg", default, skip_serializing_if = "Option::is_none")]
    trade_order_eff_global: Option<f64>,
    #[serde(rename = "tem", default, skip_serializing_if = "Option::is_none")]
    trade_effective_eff_multiplier: Option<f64>,
    #[serde(rename = "mpt", skip_serializing_if = "Option::is_none")]
    manu_prod_total: Option<f64>,
    #[serde(rename = "mpb", skip_serializing_if = "Option::is_none")]
    manu_prod_base: Option<f64>,
    #[serde(rename = "mps", default, skip_serializing_if = "Option::is_none")]
    manu_prod_skill: Option<f64>,
    #[serde(rename = "mpg", default, skip_serializing_if = "Option::is_none")]
    manu_prod_global: Option<f64>,
    #[serde(rename = "msl", default, skip_serializing_if = "Option::is_none")]
    manu_storage_limit: Option<i32>,
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
    write_binary(
        options.out_dir.join("combo_table.bin"),
        &BakedComboTableDisk::from(&combo_table),
    )?;

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
        sort_score: result.order_eff_total,
        order_kind: Some(order_kind),
        recipe: None,
        trade_pct: Some(result.order_eff_total),
        gold_pct: Some(result.order_mechanic.mechanic_equiv_eff_pct),
        shortcut: result.trade_shortcut.clone(),
        unit_trade_per_day: Some(result.production.unit.unit_trade_per_day),
        unit_gold_per_day: Some(result.production.unit.unit_gold_per_day),
        unit_originium_per_day: Some(result.production.unit.unit_originium_per_day),
        output_multiplier: Some(result.production.unit.multiplier_vs_lv3_regular),
        trade_order_eff_base: Some(result.order_eff_base),
        trade_order_eff_skill: Some(result.order_eff_skill),
        trade_order_eff_global: Some(
            result.order_eff_total - result.order_eff_base - result.order_eff_skill,
        ),
        trade_effective_eff_multiplier: Some(result.effective_eff_multiplier),
        manu_prod_total: None,
        manu_prod_base: None,
        manu_prod_skill: None,
        manu_prod_global: None,
        manu_storage_limit: None,
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
        sort_score: result.prod_total,
        order_kind: None,
        recipe: Some(recipe),
        trade_pct: None,
        gold_pct: None,
        shortcut: None,
        unit_trade_per_day: None,
        unit_gold_per_day: None,
        unit_originium_per_day: None,
        output_multiplier: None,
        trade_order_eff_base: None,
        trade_order_eff_skill: None,
        trade_order_eff_global: None,
        trade_effective_eff_multiplier: None,
        manu_prod_total: Some(result.prod_total),
        manu_prod_base: Some(result.prod_base),
        manu_prod_skill: Some(result.prod_skill),
        manu_prod_global: Some(result.prod_total - result.prod_base - result.prod_skill),
        manu_storage_limit: Some(result.storage_limit),
    })
}

fn sort_signature_rows(rows: &mut [BakedComboRow]) {
    rows.sort_by(|a, b| {
        b.sort_score
            .partial_cmp(&a.sort_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.names.cmp(&b.names))
    });
}

pub fn validate_baked_catalog(out_dir: &Path, generator: &BakeGeneratorFingerprint) -> Result<()> {
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
    if !baked_trade_compatible(pool, options, filter) {
        return Ok(None);
    }
    let Some(table) = load_runtime_baked_table()? else {
        return Ok(None);
    };
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
        return Ok(None);
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
        return Ok(None);
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
    if !baked_manufacture_compatible(pool, options) {
        return Ok(None);
    }
    let Some(table) = load_runtime_baked_table()? else {
        return Ok(None);
    };
    let key = manufacture_lookup_key(options.level, options.operator_capacity, recipe);
    let Some((start_idx, len)) = table.index_by_key.get(&key).copied() else {
        return Ok(None);
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
        return Ok(None);
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

fn load_runtime_baked_table() -> Result<Option<&'static RuntimeBakedComboTable>> {
    static CACHE: OnceLock<Option<RuntimeBakedComboTable>> = OnceLock::new();
    if let Some(table) = CACHE.get() {
        return Ok(table.as_ref());
    }
    match load_runtime_baked_table_inner() {
        Ok(table) => {
            let _ = CACHE.set(Some(table));
            Ok(CACHE.get().and_then(|t| t.as_ref()))
        }
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("data/baked/manifest.json not found")
                || msg.contains("file not found")
                || msg.contains("No such file or directory")
                || msg.contains("baked schema mismatch")
                || msg.contains("baked generator mismatch")
            {
                Ok(None)
            } else {
                Err(err)
            }
        }
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
    validate_baked_catalog(&out_dir, &generator)?;
    let combo_path = out_dir.join("combo_table.bin");
    let raw = fs::read(&combo_path)?;
    let disk: BakedComboTableDisk = bincode::deserialize(&raw)
        .map_err(|e| Error::msg(format!("read {}: {e}", combo_path.display())))?;
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
        && baked_layout_search_compatible(&options.layout)
        && pool.entries.iter().all(|entry| entry.progress.elite >= 2)
}

fn baked_manufacture_compatible(pool: &ManuPool, options: &ManuSearchOptions) -> bool {
    (options.mood - 24.0).abs() < f64::EPSILON
        && baked_layout_search_compatible(&options.layout)
        && pool.entries.iter().all(|entry| entry.progress.elite >= 2)
}

fn baked_layout_search_compatible(layout: &LayoutContext) -> bool {
    let _ = layout;
    true
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
    let trade_pct = row.trade_pct?;
    let gold_pct = row.gold_pct?;
    let unit_trade_per_day = row.unit_trade_per_day.unwrap_or(0.0);
    let unit_gold_per_day = row.unit_gold_per_day.unwrap_or(0.0);
    let output_multiplier = row.output_multiplier.unwrap_or(0.0);
    let eff_factor = 1.0 + trade_pct / 100.0;
    let mech_factor = 1.0 + gold_pct / 100.0;
    Some(TradeSearchHit {
        names: row.names.clone(),
        gold_names: vec![],
        originium_names: vec![],
        score: trade_pct,
        trade_pct,
        gold_pct,
        shortcut: row.shortcut.clone(),
        unit_trade_per_day,
        unit_gold_per_day,
        unit_originium_per_day: row.unit_originium_per_day.unwrap_or(0.0),
        output_multiplier,
        breakdown: TradeScoreBreakdown {
            order_eff_base: row
                .trade_order_eff_base
                .unwrap_or(row.operator_capacity as f64),
            order_eff_skill: row.trade_order_eff_skill.unwrap_or(0.0),
            order_eff_global: row.trade_order_eff_global.unwrap_or(0.0),
            order_eff_total_pct: trade_pct,
            mechanic_equiv_eff_pct: gold_pct,
            eff_factor,
            mech_factor,
            effective_eff_multiplier: row
                .trade_effective_eff_multiplier
                .unwrap_or(output_multiplier),
            unit_trade_per_day,
            unit_gold_per_day,
            shortcut_id: row.shortcut.clone(),
        },
    })
}

fn row_to_manu_hit(row: &BakedComboRow) -> Option<ManuSearchHit> {
    if row.facility != "manufacture" {
        return None;
    }
    let recipe = row.recipe?;
    let prod_total = row.manu_prod_total?;
    let storage_limit = row.manu_storage_limit.unwrap_or(0);
    let mut per_station = crate::manufacture::ManuProdBreakdown::default();
    let mut storage = crate::manufacture::ManuStorageBreakdown::default();
    let recipe_label = match recipe {
        RecipeKind::Gold => {
            per_station.gold = prod_total;
            storage.gold = storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = prod_total;
            storage.battle_record = storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = prod_total;
            storage.originium = storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };
    Some(ManuSearchHit {
        names: row.names.clone(),
        gold_names: vec![],
        battle_record_names: vec![],
        composite_score: prod_total,
        per_station,
        storage,
        breakdown: ManuScoreBreakdown {
            prod_base: row.manu_prod_base.unwrap_or(row.operator_capacity as f64),
            prod_skill: row.manu_prod_skill.unwrap_or(0.0),
            prod_global: row.manu_prod_global.unwrap_or(0.0),
            prod_total,
            storage_limit,
            recipe: recipe_label.to_string(),
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
            .then_with(|| {
                b.sort_score
                    .partial_cmp(&a.sort_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.names.cmp(&b.names))
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
    let mut indices: Vec<_> = names
        .iter()
        .filter_map(|name| operator_index.get(name.as_str()).copied())
        .collect();
    indices.sort_unstable();

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
        "trade_shortcuts.json",
        "trade_segments.json",
        "base_systems.json",
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
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(BakeInputFingerprint {
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len() as u64,
        hash64: format!("{:016x}", hasher.finish()),
    })
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let file = File::create(&path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)
        .map_err(|e| Error::msg(format!("write {}: {e}", path.display())))
}

fn write_binary(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let bytes = bincode::serialize(value)
        .map_err(|e| Error::msg(format!("encode {}: {e}", path.display())))?;
    fs::write(&path, bytes).map_err(|e| Error::msg(format!("write {}: {e}", path.display())))
}
