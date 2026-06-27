use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::layout::LayoutContext;
use crate::manufacture::ManuSearchRecipeMode;
use crate::pool::{build_manufacture_pool, build_trade_pool};
use crate::roster::{OperatorProgress, Roster};
use crate::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchHit, ManuSearchOptions,
    TradeSearchHit, TradeSearchOptions,
};
use crate::skill_table::{data_path, default_skill_table_path, SkillTable};
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use crate::types::RecipeKind;

pub const BAKE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct BakeOptions {
    pub out_dir: PathBuf,
    pub include_trade: bool,
    pub include_manufacture: bool,
    pub limit_per_signature: Option<usize>,
    pub generator: Option<BakeGeneratorFingerprint>,
}

impl Default for BakeOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("data/baked"),
            include_trade: true,
            include_manufacture: true,
            limit_per_signature: None,
            generator: None,
        }
    }
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

#[derive(Debug, Serialize)]
struct BakedComboTable {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    mask_words: usize,
    indexes: Vec<BakedComboIndex>,
    rows: Vec<BakedComboRow>,
}

#[derive(Debug, Serialize)]
struct BakedComboIndex {
    signature_key: String,
    start: usize,
    len: usize,
}

#[derive(Debug, Serialize)]
struct BakedComboRow {
    row_id: usize,
    facility: &'static str,
    signature_key: String,
    room_level: u8,
    operator_capacity: usize,
    names: Vec<String>,
    operator_indices: Vec<usize>,
    operator_mask: Vec<u64>,
    sort_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_kind: Option<TradeOrderKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recipe: Option<RecipeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trade_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gold_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shortcut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit_trade_per_day: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit_gold_per_day: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manu_prod_total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Serialize)]
struct BakedTradeCatalog {
    schema_version: u32,
    signatures: Vec<BakedTradeSignature>,
}

#[derive(Debug, Serialize)]
struct BakedTradeSignature {
    room_level: u8,
    operator_capacity: usize,
    order_kind: TradeOrderKind,
    gold_production_lines: u32,
    hits: Vec<TradeSearchHit>,
}

#[derive(Debug, Serialize)]
struct BakedManufactureCatalog {
    schema_version: u32,
    signatures: Vec<BakedManufactureSignature>,
}

#[derive(Debug, Serialize)]
struct BakedManufactureSignature {
    room_level: u8,
    operator_capacity: usize,
    recipe: RecipeKind,
    hits: Vec<ManuSearchHit>,
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

    write_json(options.out_dir.join("operators.json"), &operators)?;

    let mut trade_signatures = 0usize;
    let mut trade_hits = 0usize;
    let trade_catalog = if options.include_trade {
        let catalog = bake_trade_catalog(&roster, &instances, &table, options)?;
        trade_signatures = catalog.signatures.len();
        trade_hits = catalog.signatures.iter().map(|s| s.hits.len()).sum();
        write_json(options.out_dir.join("trade_combos.json"), &catalog)?;
        Some(catalog)
    } else {
        None
    };

    let mut manufacture_signatures = 0usize;
    let mut manufacture_hits = 0usize;
    let manufacture_catalog = if options.include_manufacture {
        let catalog = bake_manufacture_catalog(&roster, &instances, &table, options)?;
        manufacture_signatures = catalog.signatures.len();
        manufacture_hits = catalog.signatures.iter().map(|s| s.hits.len()).sum();
        write_json(options.out_dir.join("manufacture_combos.json"), &catalog)?;
        Some(catalog)
    } else {
        None
    };

    let combo_table = build_combo_table(
        &operators,
        trade_catalog.as_ref(),
        manufacture_catalog.as_ref(),
        options.generator.clone(),
    );
    let combo_table_rows = combo_table.rows.len();
    write_json(options.out_dir.join("combo_table.json"), &combo_table)?;

    let manifest = BakeManifest {
        schema_version: BAKE_SCHEMA_VERSION,
        generated_by: "infra-core::bake".to_string(),
        model: "baseline_tier_up_operator_catalog".to_string(),
        generator: options.generator.clone(),
        inputs: bake_input_fingerprints()?,
        options: BakeManifestOptions {
            include_trade: options.include_trade,
            include_manufacture: options.include_manufacture,
            limit_per_signature: options.limit_per_signature,
            roster_model: "all modelled trade/manufacture operators at elite2 level1 rarity6"
                .to_string(),
            layout_model: "LayoutContext::search_baseline".to_string(),
        },
    };
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
    write_json(options.out_dir.join("summary.json"), &report)?;
    Ok(report)
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

fn build_combo_table(
    operators: &[BakedOperator],
    trade_catalog: Option<&BakedTradeCatalog>,
    manufacture_catalog: Option<&BakedManufactureCatalog>,
    generator: Option<BakeGeneratorFingerprint>,
) -> BakedComboTable {
    let operator_index: HashMap<&str, usize> = operators
        .iter()
        .enumerate()
        .map(|(idx, op)| (op.name.as_str(), idx))
        .collect();
    let mask_words = operators.len().div_ceil(64).max(1);
    let mut rows = Vec::new();

    if let Some(catalog) = trade_catalog {
        for signature in &catalog.signatures {
            let signature_key = trade_signature_key(signature);
            for hit in &signature.hits {
                let (operator_indices, operator_mask) =
                    operator_index_and_mask(&hit.names, &operator_index, mask_words);
                rows.push(BakedComboRow {
                    row_id: 0,
                    facility: "trade",
                    signature_key: signature_key.clone(),
                    room_level: signature.room_level,
                    operator_capacity: signature.operator_capacity,
                    names: hit.names.clone(),
                    operator_indices,
                    operator_mask,
                    sort_score: hit.trade_pct,
                    order_kind: Some(signature.order_kind),
                    recipe: None,
                    trade_pct: Some(hit.trade_pct),
                    gold_pct: Some(hit.gold_pct),
                    shortcut: hit.shortcut.clone(),
                    unit_trade_per_day: Some(hit.unit_trade_per_day),
                    unit_gold_per_day: Some(hit.unit_gold_per_day),
                    manu_prod_total: None,
                    manu_storage_limit: None,
                });
            }
        }
    }

    if let Some(catalog) = manufacture_catalog {
        for signature in &catalog.signatures {
            let signature_key = manufacture_signature_key(signature);
            for hit in &signature.hits {
                let (operator_indices, operator_mask) =
                    operator_index_and_mask(&hit.names, &operator_index, mask_words);
                rows.push(BakedComboRow {
                    row_id: 0,
                    facility: "manufacture",
                    signature_key: signature_key.clone(),
                    room_level: signature.room_level,
                    operator_capacity: signature.operator_capacity,
                    names: hit.names.clone(),
                    operator_indices,
                    operator_mask,
                    sort_score: hit.composite_score,
                    order_kind: None,
                    recipe: Some(signature.recipe),
                    trade_pct: None,
                    gold_pct: None,
                    shortcut: None,
                    unit_trade_per_day: None,
                    unit_gold_per_day: None,
                    manu_prod_total: Some(hit.breakdown.prod_total),
                    manu_storage_limit: Some(hit.breakdown.storage_limit),
                });
            }
        }
    }

    rows.sort_by(|a, b| {
        a.facility
            .cmp(b.facility)
            .then_with(|| a.signature_key.cmp(&b.signature_key))
            .then_with(|| b.operator_capacity.cmp(&a.operator_capacity))
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
        operator_count: operators.len(),
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

fn trade_signature_key(signature: &BakedTradeSignature) -> String {
    format!(
        "trade:level{}:cap{}:order_{:?}:gold_lines{}",
        signature.room_level,
        signature.operator_capacity,
        signature.order_kind,
        signature.gold_production_lines
    )
    .to_ascii_lowercase()
}

fn manufacture_signature_key(signature: &BakedManufactureSignature) -> String {
    format!(
        "manufacture:level{}:cap{}:recipe_{:?}",
        signature.room_level, signature.operator_capacity, signature.recipe
    )
    .to_ascii_lowercase()
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

fn bake_trade_catalog(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &BakeOptions,
) -> Result<BakedTradeCatalog> {
    let pool = build_trade_pool(roster, instances, table)?;
    let mut signatures = Vec::new();

    for room_level in 1..=3 {
        for order_kind in [TradeOrderKind::Gold, TradeOrderKind::Originium] {
            let search_opts = TradeSearchOptions {
                trade_level: room_level,
                operator_capacity: station_operator_capacity(room_level),
                top_k: options.limit_per_signature.unwrap_or(usize::MAX),
                layout: std::sync::Arc::new(LayoutContext::search_baseline()),
                order_mode: TradeSearchOrderMode::Single(order_kind),
                ..TradeSearchOptions::default()
            };
            let report = search_trade_triples(&pool, table, &search_opts)?;
            signatures.push(BakedTradeSignature {
                room_level,
                operator_capacity: search_opts.operator_capacity,
                order_kind,
                gold_production_lines: search_opts.gold_production_lines,
                hits: report.top,
            });
        }
    }

    Ok(BakedTradeCatalog {
        schema_version: BAKE_SCHEMA_VERSION,
        signatures,
    })
}

fn bake_manufacture_catalog(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &BakeOptions,
) -> Result<BakedManufactureCatalog> {
    let pool = build_manufacture_pool(roster, instances, table)?;
    let mut signatures = Vec::new();

    for room_level in 1..=3 {
        for recipe in [
            RecipeKind::Gold,
            RecipeKind::BattleRecord,
            RecipeKind::Originium,
        ] {
            let search_opts = ManuSearchOptions {
                level: room_level,
                operator_capacity: station_operator_capacity(room_level),
                recipe_mode: ManuSearchRecipeMode::Single(recipe),
                top_k: options.limit_per_signature.unwrap_or(usize::MAX),
                layout: std::sync::Arc::new(LayoutContext::search_baseline()),
                ..ManuSearchOptions::default()
            };
            let report = search_manufacture_triples(&pool, table, &search_opts)?;
            signatures.push(BakedManufactureSignature {
                room_level,
                operator_capacity: search_opts.operator_capacity,
                recipe,
                hits: report.top,
            });
        }
    }

    Ok(BakedManufactureCatalog {
        schema_version: BAKE_SCHEMA_VERSION,
        signatures,
    })
}

fn bake_roster(instances: &OperatorInstances) -> Roster {
    let mut roster = Roster::default();
    for name in modelled_production_operator_names(instances) {
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

fn modelled_production_operator_names(instances: &OperatorInstances) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (_, instance) in instances.iter() {
        if instance.facilities.contains_key("trade")
            || instance.facilities.contains_key("manufacture")
        {
            names.insert(instance.name.clone());
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
    serde_json::to_writer_pretty(file, value)
        .map_err(|e| Error::msg(format!("write {}: {e}", path.display())))
}
