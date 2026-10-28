use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::global_resource::GlobalInjectManifest;
use crate::skill_table::data_path;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::input::{TradeOperator, TradeOperatorRef, TradeOrderKind};
use crate::trade::order_mechanic::{GoldDistribution, OrderMechanicResult, SpecialOrderKind};
use crate::types::Action;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutTailorTier {
    Regular,
    Alpha,
    Beta,
    Docus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutMatchRule {
    pub kind: String,
    #[serde(default)]
    pub station_bonus_efficiency_anchor: Option<Efficiency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeShortcutEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub mechanic_equivalent_efficiency: Efficiency,
    #[serde(default = "default_tailor_tier")]
    pub tailor_tier: ShortcutTailorTier,
    #[serde(default)]
    pub r#match: Option<ShortcutMatchRule>,
    /// 公孙工具人表单位贸易产出（L3 产量锚，L2 未展开巫恋核等时使用）。
    #[serde(default)]
    pub unit_trade_anchor: Option<f64>,
    #[serde(default)]
    pub unit_gsl_gold_anchor: Option<f64>,
    #[serde(default)]
    pub unit_output: Option<CommunityUnitOutputRule>,
    /// 社区数值在视觉表或需求文档中的可复核位置。
    #[serde(default)]
    pub unit_output_source: Option<String>,
    /// 区分原始精确单位产出与从取整等效效率反算的派生值。
    #[serde(default)]
    pub unit_output_precision: Option<CommunityValuePrecision>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommunityValuePrecision {
    Exact,
    DerivedFromRoundedEquivalent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommunityUnitOutputRule {
    UnitOutputMultiplier { value: f64 },
    EnhancedUnitOutputPerDay { value: f64 },
    EnhancedUnitOutputByLevel { lv1: f64, lv2: f64, lv3: f64 },
}

impl CommunityUnitOutputRule {
    pub fn unit_trade_per_day(&self, level: u8, reference: f64) -> f64 {
        match self {
            Self::UnitOutputMultiplier { value } => reference * value,
            Self::EnhancedUnitOutputPerDay { value } => *value,
            Self::EnhancedUnitOutputByLevel { lv1, lv2, lv3 } => match level {
                1 => *lv1,
                2 => *lv2,
                _ => *lv3,
            },
        }
    }

    fn validate(&self) -> std::result::Result<(), String> {
        let values: Vec<f64> = match self {
            Self::UnitOutputMultiplier { value } | Self::EnhancedUnitOutputPerDay { value } => {
                vec![*value]
            }
            Self::EnhancedUnitOutputByLevel { lv1, lv2, lv3 } => vec![*lv1, *lv2, *lv3],
        };
        if values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err("unit_output values must be finite and positive".to_string());
        }
        Ok(())
    }
}

fn default_tailor_tier() -> ShortcutTailorTier {
    ShortcutTailorTier::Regular
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TradeShortcutFile {
    pub version: u32,
    pub source: String,
    pub entries: Vec<TradeShortcutEntry>,
}

#[derive(Debug, Clone)]
pub struct TradeShortcutMatch {
    pub entry: TradeShortcutEntry,
}

pub fn load_trade_shortcuts(path: &Path) -> Result<Vec<TradeShortcutEntry>> {
    crate::profile::record_shortcut_json_load();
    let raw = std::fs::read_to_string(path)?;
    let file: TradeShortcutFile = serde_json::from_str(&raw)?;
    if file.version != 4 {
        return Err(crate::error::Error::msg(format!(
            "unsupported trade_shortcuts version {}, expected 4",
            file.version
        )));
    }
    if file.source.trim().is_empty() {
        return Err(crate::error::Error::msg(
            "trade_shortcuts source must not be empty",
        ));
    }
    validate_trade_shortcuts(&file.entries).map_err(crate::error::Error::msg)?;
    Ok(file.entries)
}

fn entry_requires_community_unit_output(entry: &TradeShortcutEntry) -> bool {
    matches!(
        entry.r#match.as_ref().map(|rule| rule.kind.as_str()),
        Some("witch" | "closure" | "blackkey_closure" | "docus" | "docus_syracusa")
    )
}

fn validate_trade_shortcuts(entries: &[TradeShortcutEntry]) -> std::result::Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        if !ids.insert(entry.id.as_str()) {
            return Err(format!("duplicate trade shortcut id {}", entry.id));
        }
        if entry_requires_community_unit_output(entry) && entry.unit_output.is_none() {
            return Err(format!(
                "{} changes trade unit output but has no community unit_output",
                entry.id
            ));
        }
        if let Some(rule) = &entry.unit_output {
            rule.validate()
                .map_err(|reason| format!("{}: {reason}", entry.id))?;
            if entry
                .unit_output_source
                .as_deref()
                .is_none_or(|source| source.trim().is_empty())
            {
                return Err(format!("{} has unit_output without source", entry.id));
            }
            if entry.unit_output_precision.is_none() {
                return Err(format!("{} has unit_output without precision", entry.id));
            }
        }
    }
    Ok(())
}

pub fn default_shortcuts_path() -> Result<std::path::PathBuf> {
    data_path("trade_shortcuts.json")
}

pub(crate) struct TradeShortcutCache {
    entries: Vec<TradeShortcutEntry>,
    by_id: HashMap<String, usize>,
    closure_indices: Vec<usize>,
}

impl TradeShortcutCache {
    fn build(entries: Vec<TradeShortcutEntry>) -> Self {
        let mut by_id = HashMap::new();
        let mut closure_indices = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            by_id.insert(entry.id.clone(), i);
            if entry.r#match.as_ref().is_some_and(|m| m.kind == "closure") {
                closure_indices.push(i);
            }
        }
        Self {
            entries,
            by_id,
            closure_indices,
        }
    }

    pub(crate) fn get_by_id(&self, id: &str) -> Option<&TradeShortcutEntry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    fn closure_entries(&self) -> impl Iterator<Item = &TradeShortcutEntry> {
        self.closure_indices.iter().map(|&i| &self.entries[i])
    }
}

pub(crate) fn required_community_unit_trade_per_day_by_id(
    id: &str,
    level: u8,
    reference: f64,
) -> Result<f64> {
    let cache = trade_shortcut_cache_result()?;
    let entry = cache
        .get_by_id(id)
        .ok_or_else(|| crate::error::Error::msg(format!("missing trade shortcut {id}")))?;
    entry
        .unit_output
        .as_ref()
        .map(|rule| rule.unit_trade_per_day(level, reference))
        .ok_or_else(|| crate::error::Error::msg(format!("{id} has no community unit_output")))
}

static TRADE_SHORTCUT_CACHE: OnceLock<std::result::Result<TradeShortcutCache, String>> =
    OnceLock::new();

fn trade_shortcut_cache_result() -> Result<&'static TradeShortcutCache> {
    TRADE_SHORTCUT_CACHE
        .get_or_init(|| {
            let path = default_shortcuts_path().map_err(|err| err.to_string())?;
            let entries = load_trade_shortcuts(&path).map_err(|err| err.to_string())?;
            Ok(TradeShortcutCache::build(entries))
        })
        .as_ref()
        .map_err(|message| crate::error::Error::msg(message.clone()))
}

pub(crate) fn ensure_trade_shortcuts_loaded() -> Result<()> {
    trade_shortcut_cache_result().map(|_| ())
}

pub(crate) fn trade_shortcut_cache() -> Option<&'static TradeShortcutCache> {
    trade_shortcut_cache_result().ok()
}

/// 巫恋核 / 龙舌兰投资 / 裁缝 α/β（与但书、可露希尔互斥的「巫恋侧」机制）。
pub fn room_has_witch_side_group(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    room_has_witch_side_group_refs(&ops, table)
}

pub(crate) fn room_has_witch_side_group_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    if has_witch_e2(ops, table) {
        return true;
    }
    if ops
        .iter()
        .any(|o| o.name == "龙舌兰" && has_long_invest_buff(o, table))
    {
        return true;
    }
    ops.iter()
        .any(|o| has_tailor_beta(o, table) || has_tailor_alpha(o, table))
}

/// 但书（合同法/违约）与巫恋侧机制 **不得同站**。
pub fn docus_tailor_exclusive_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    docus_tailor_exclusive_violation_refs(&ops, table)
}

pub(crate) fn docus_tailor_exclusive_violation_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    room_has_docus_mechanic(ops, table) && room_has_witch_side_group_refs(ops, table)
}

/// 佩佩独占站：同房不得进驻提供订单获取效率% 的干员（工具人位应填上限/心情等）。
pub fn pepe_station_trade_eff_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    pepe_station_trade_eff_violation_refs(&ops, table)
}

pub(crate) fn pepe_station_trade_eff_violation_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    if !room_has_pepe_exclusive(ops, table) {
        return false;
    }
    ops.iter()
        .any(|o| o.name != "佩佩" && operator_constant_trade_flat_eff(o, table) > 0.0)
}

/// 贸易站同房互斥（公孙长乐）：违约 / 特别订单 / 巫恋低语 / 佩佩+效率人 不得混排。
pub fn trade_station_exclusive_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    trade_station_exclusive_violation_refs(&ops, table)
}

pub(crate) fn trade_station_exclusive_violation_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    crate::profile::record_exclusive_check();
    let docus = room_has_docus_mechanic(ops, table);
    let closure = has_closure(ops, table);
    let witch = has_witch_e2(ops, table);
    let pepe = room_has_pepe_exclusive(ops, table);

    if docus_tailor_exclusive_violation_refs(ops, table) {
        return true;
    }
    if pepe_station_trade_eff_violation_refs(ops, table) {
        return true;
    }
    if pepe && (docus || closure || room_has_witch_side_group_refs(ops, table)) {
        return true;
    }
    if docus && closure {
        return true;
    }
    if witch && closure {
        return true;
    }
    false
}

/// 但书 + 工具人三人站（搜索/轮换均为 C(n,3)）。
pub fn is_docus_solo_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_docus_solo_station_refs(&ops, table)
}

pub(crate) fn is_docus_solo_station_refs(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.len() >= 3
        && room_has_docus_mechanic(ops, table)
        && !room_has_witch_side_group_refs(ops, table)
}

/// 叙拉古但书链段 consumer：但书 + 伺夜 + 贝洛内（无巫恋侧）。
pub fn is_docus_syracusa_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_docus_syracusa_station_refs(&ops, table)
}

pub(crate) fn is_docus_syracusa_station_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    is_docus_solo_station_refs(ops, table)
        && ops.iter().any(|o| o.name == "伺夜")
        && ops.iter().any(|o| o.name == "贝洛内")
}

/// 链段 producer 已满足（中枢八幡海铃 E2）。
pub fn docus_syracusa_segment_active(inject: &GlobalInjectManifest) -> bool {
    inject.has_active_source_buff("control_tra_limit&spd2[000]")
}

/// 可露希尔特别订单站（不含但书/巫恋低语）。
pub fn is_closure_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_closure_station_refs(&ops, table)
}

pub(crate) fn is_closure_station_refs(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    has_closure(ops, table) && !has_witch_e2(ops, table) && !room_has_docus_mechanic(ops, table)
}

/// 公孙：但书单走最终效率 = 纸面效率 × 社区单位产出倍率。
pub const DOCUS_MECHANIC_GOLD_PCT: f64 = 55.0;

/// **L3 组合短路**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.7）：工具人表最优解查表。
pub fn resolve_trade_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    resolve_trade_shortcut_refs(&ops, table, order_eff_pre, trade_level, inject)
}

pub(crate) fn resolve_trade_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    resolve_trade_shortcut_inner(ops, table, order_eff_pre, trade_level, inject, true)
}

pub(crate) fn resolve_trade_shortcut_prevalidated_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    resolve_trade_shortcut_inner(ops, table, order_eff_pre, trade_level, inject, false)
}

fn resolve_trade_shortcut_inner(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
    check_exclusivity: bool,
) -> Option<TradeShortcutMatch> {
    if check_exclusivity && trade_station_exclusive_violation_refs(ops, table) {
        return None;
    }
    if let Some(m) = crate::trade::segment::match_registered_trade_segment_refs(ops, table, inject)
    {
        return Some(m);
    }
    if let Some(m) = match_docus_solo_shortcut_refs(ops, table) {
        return Some(m);
    }
    if let Some(m) = match_witch_group_shortcut_refs(ops, table, trade_level) {
        return Some(m);
    }
    if let Some(m) = match_blackkey_closure_shortcut_refs(ops, table) {
        return Some(m);
    }
    // 推王龙门的 135% 贸易效率是戴菲恩中枢 producer 激活后的 vault 满配值，
    // 只能通过 trade_segments 的 producer-gated 路径命中；无戴菲恩 fallback 需另建锚点。
    if let Some(m) = match_penguin_exusiai_lemuen_shortcut_refs(ops, table) {
        return Some(m);
    }
    if let Some(m) = match_penguin_texangel_e2_shortcut_refs(ops, table) {
        return Some(m);
    }
    if let Some(m) = match_penguin_texlap_e0_shortcut_refs(ops, table) {
        return Some(m);
    }
    match_closure_shortcut(ops, table, order_eff_pre, trade_level)
}

/// 公孙感知链贸站：黑键 E2（乐感）+ 可露希尔 closure，第三人任意高效散件；不与但书/巫恋同房。
pub fn is_blackkey_closure_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_blackkey_closure_station_refs(&ops, table)
}

pub(crate) fn is_blackkey_closure_station_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    ops.len() >= 3
        && room_has_blackkey_perception(ops, table)
        && has_closure(ops, table)
        && !room_has_witch_side_group_refs(ops, table)
        && !room_has_docus_mechanic(ops, table)
}

pub fn match_blackkey_closure_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_blackkey_closure_shortcut_refs(&ops, table)
}

pub(crate) fn match_blackkey_closure_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_blackkey_closure_station_refs(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_blackkey_closure")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 德克萨斯 E2「默契」；企鹅物流德能 bond 识别用。
const TEX_MOQI_BUFF: &str = "trade_ord_limit&cost_P[010]";
/// 黑键·乐感（宿舍人数 → 感知）；E2 组合短路识别用。
const BLACKKEY_PERCEPTION_BUFF: &str = "trade_ord_spd_bd_n1[000]";

/// 德狼「恩怨」+ 拉普兰德；第三人任意。
///
/// 公孙 vault 明确德克萨斯精 2 不会失去「恩怨」，所以这里不再要求
/// `TEX_ENEMY_BUFF` 只存在于 E0，也不因 E2「默契」排除德狼路线。
pub fn is_penguin_texlap_e0_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_penguin_texlap_e0_station_refs(&ops, table)
}

pub(crate) fn is_penguin_texlap_e0_station_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    ops.len() >= 3
        && ops.iter().any(|o| o.name == "德克萨斯")
        && ops.iter().any(|o| o.name == "拉普兰德")
        && !penguin_bond_excluded(ops, table)
}

pub fn match_penguin_texlap_e0_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_penguin_texlap_e0_shortcut_refs(&ops, table)
}

pub(crate) fn match_penguin_texlap_e0_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_penguin_texlap_e0_station_refs(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_penguin_texlap_e0")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 德克萨斯 E2「默契」+ 能天使；不含蕾缪安 E2 相伴链。
pub fn is_penguin_texangel_e2_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_penguin_texangel_e2_station_refs(&ops, table)
}

pub(crate) fn is_penguin_texangel_e2_station_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    ops.len() >= 3
        && room_has_named_buff(ops, "德克萨斯", TEX_MOQI_BUFF)
        && ops.iter().any(|o| o.name == "能天使")
        && !ops
            .iter()
            .any(|o| o.name == "蕾缪安" && o.tier() == PromotionTier::TierUp)
        && !penguin_bond_excluded(ops, table)
}

pub fn match_penguin_texangel_e2_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_penguin_texangel_e2_shortcut_refs(&ops, table)
}

pub(crate) fn match_penguin_texangel_e2_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_penguin_texangel_e2_station_refs(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_penguin_texangel_e2")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 能天使 + 蕾缪安 E2「相伴」；第三人任意。
pub fn is_penguin_exusiai_lemuen_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_penguin_exusiai_lemuen_station_refs(&ops, table)
}

pub(crate) fn is_penguin_exusiai_lemuen_station_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> bool {
    ops.len() >= 3
        && ops.iter().any(|o| o.name == "能天使")
        && ops
            .iter()
            .any(|o| o.name == "蕾缪安" && o.tier() == PromotionTier::TierUp)
        && !penguin_bond_excluded(ops, table)
}

pub fn match_penguin_exusiai_lemuen_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_penguin_exusiai_lemuen_shortcut_refs(&ops, table)
}

pub(crate) fn match_penguin_exusiai_lemuen_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_penguin_exusiai_lemuen_station_refs(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_penguin_exusiai_lemuen")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 推王组：推进之王 + 摩根 + 维娜·维多利亚（需中枢戴菲恩 E2 producer）。
pub fn is_vina_lungmen_station(ops: &[TradeOperator], _table: &SkillTable) -> bool {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    is_vina_lungmen_station_refs(&ops, _table)
}

pub(crate) fn is_vina_lungmen_station_refs(
    ops: &[TradeOperatorRef<'_>],
    _table: &SkillTable,
) -> bool {
    ops.len() >= 3
        && ops.iter().any(|o| o.name == "推进之王")
        && ops.iter().any(|o| o.name == "摩根")
        && ops.iter().any(|o| o.name == "维娜·维多利亚")
}

pub fn match_vina_lungmen_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_vina_lungmen_shortcut_refs(&ops, table)
}

pub(crate) fn match_vina_lungmen_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_vina_lungmen_station_refs(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_vina_lungmen")?.clone();
    Some(TradeShortcutMatch { entry })
}

pub fn match_docus_solo_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_docus_solo_shortcut_refs(&ops, table)
}

pub(crate) fn match_docus_solo_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_docus_solo_station_refs(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let docus_e2 = docus_operator_is_e2_refs(ops, table);
    let shortcut_id = if docus_e2 {
        "gsl_docus_solo"
    } else {
        "gsl_docus_solo_e0"
    };
    let entry = cache.get_by_id(shortcut_id)?.clone();
    Some(TradeShortcutMatch { entry })
}

pub(crate) fn docus_operator_is_e2_refs(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.iter()
        .find(|operator| has_docus_buff(operator, table))
        .is_some_and(|operator| operator.tier() == PromotionTier::TierUp)
}

/// 叙拉古但书链段：委托 `trade/segment` 注册表（保留 API 供测试对照）。
pub fn match_docus_syracusa_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_docus_syracusa_shortcut_refs(&ops, table, inject)
}

pub(crate) fn match_docus_syracusa_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    crate::trade::segment::match_registered_trade_segment_refs(ops, table, inject)
        .filter(|m| m.entry.id == "gsl_docus_syracusa")
}

pub fn match_witch_group_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    trade_level: u8,
) -> Option<TradeShortcutMatch> {
    let ops: SmallVec<[_; 5]> = ops.iter().map(TradeOperator::as_ref).collect();
    match_witch_group_shortcut_refs(&ops, table, trade_level)
}

pub(crate) fn match_witch_group_shortcut_refs(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
    trade_level: u8,
) -> Option<TradeShortcutMatch> {
    if trade_level != 3 {
        return None;
    }
    let kind = classify_witch_room(ops, table)?;
    let id = match kind {
        WitchRoomKind::LongTierUpBeta => "gsl_witch_long_beta",
        WitchRoomKind::LongTierUpAlpha => "gsl_witch_long_alpha",
        WitchRoomKind::LongTierUpBlank => "gsl_witch_long_blank",
        WitchRoomKind::LongTier0Beta => "gsl_witch_long0_beta",
        WitchRoomKind::LongTier0Alpha => "gsl_witch_long0_alpha",
        WitchRoomKind::LongTier0Blank => "gsl_witch_long0_blank",
        WitchRoomKind::BetaBlankNoLongE2 => "gsl_witch_beta_blank",
    };
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id(id)?.clone();
    Some(TradeShortcutMatch { entry })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WitchRoomKind {
    LongTierUpBeta,
    LongTierUpAlpha,
    LongTierUpBlank,
    LongTier0Beta,
    LongTier0Alpha,
    LongTier0Blank,
    BetaBlankNoLongE2,
}

fn classify_witch_room(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> Option<WitchRoomKind> {
    if ops.len() < 3 {
        return None;
    }
    if !has_witch_e2(ops, table) {
        return None;
    }
    // 但书单走 / 可露希尔：同房有则不进巫恋核 L3。
    if room_has_docus_mechanic(ops, table) || has_closure(ops, table) {
        return None;
    }

    let long = find_op(ops, "龙舌兰");
    // 龙舌兰是 5 星：精 0 / 精 1 都解析为运行时 tier_0，精 2 才是 tier_up。
    let long_tier_up = long.is_some_and(|o| o.tier() == PromotionTier::TierUp);
    let long_tier_0 = long.is_some_and(|o| o.tier() == PromotionTier::Tier0);

    let has_beta = ops
        .iter()
        .any(|o| o.name != "巫恋" && has_tailor_beta(o, table));
    let has_alpha = ops
        .iter()
        .any(|o| o.name != "巫恋" && has_tailor_alpha(o, table));

    if long_tier_up && has_beta {
        return Some(WitchRoomKind::LongTierUpBeta);
    }
    if long_tier_up && has_alpha && !has_beta {
        return Some(WitchRoomKind::LongTierUpAlpha);
    }
    if long_tier_up && !has_beta && !has_alpha {
        return Some(WitchRoomKind::LongTierUpBlank);
    }
    if long_tier_0 && has_beta {
        return Some(WitchRoomKind::LongTier0Beta);
    }
    if long_tier_0 && has_alpha && !has_beta {
        return Some(WitchRoomKind::LongTier0Alpha);
    }
    if long_tier_0 && !has_beta && !has_alpha {
        return Some(WitchRoomKind::LongTier0Blank);
    }
    if !long_tier_up && has_beta && has_blank_third(ops, table) {
        return Some(WitchRoomKind::BetaBlankNoLongE2);
    }
    None
}

fn room_has_docus_mechanic(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.iter().any(|o| has_docus_buff(o, table))
}

fn room_has_named_buff(ops: &[TradeOperatorRef<'_>], name: &str, buff: &str) -> bool {
    ops.iter()
        .any(|o| o.name == name && o.buff_ids.iter().any(|b| b == buff))
}

fn penguin_bond_excluded(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    room_has_docus_mechanic(ops, table)
        || room_has_witch_side_group_refs(ops, table)
        || is_blackkey_closure_station_refs(ops, table)
        || has_closure(ops, table)
}

fn room_has_blackkey_perception(ops: &[TradeOperatorRef<'_>], _table: &SkillTable) -> bool {
    ops.iter().any(|op| {
        op.tier() == PromotionTier::TierUp
            && op.buff_ids.iter().any(|b| b == BLACKKEY_PERCEPTION_BUFF)
    })
}

fn room_has_pepe_exclusive(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.iter().any(|o| has_pepe_exclusive_buff(o, table))
}

fn has_pepe_exclusive_buff(op: &TradeOperatorRef<'_>, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        bid.starts_with("trade_ord_pepe")
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "pepe_exclusive"
                    )
                })
            })
    })
}

/// `constant` 阶段 `AddFlatEff` 之和（佩佩站互斥判定用）。
fn operator_constant_trade_flat_eff(op: &TradeOperatorRef<'_>, table: &SkillTable) -> f64 {
    let mut flat = 0.0;
    for bid in op.buff_ids {
        let Some(skill) = table.get(bid) else {
            continue;
        };
        for atom in &skill.atoms {
            if atom.phase == crate::types::Phase::Constant {
                if let Action::AddFlatEff { value, .. } = atom.action {
                    flat += value;
                }
            }
        }
    }
    flat
}

fn has_long_invest_buff(op: &TradeOperatorRef<'_>, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        bid.starts_with("trade_ord_long")
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(a.action, Action::AddOrderLmdBonus { .. })
                        || matches!(
                            a.action,
                            Action::TagOrder { ref tag } if tag == "long_invest"
                        )
                })
            })
    })
}

fn find_op<'a, 'b>(
    ops: &'a [TradeOperatorRef<'b>],
    name: &str,
) -> Option<&'a TradeOperatorRef<'b>> {
    ops.iter().find(|o| o.name == name)
}

fn has_witch_e2(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.iter().any(|o| {
        o.name == "巫恋"
            && o.tier() == PromotionTier::TierUp
            && o.buff_ids
                .iter()
                .any(|bid| has_witch_peer_absorb(bid, table))
    })
}

/// 巫恋·低语：`PeerEffAbsorb` 且 `rate_per_peer > 0`（与佩佩 rate=0 区分）。
fn has_witch_peer_absorb(bid: &str, table: &SkillTable) -> bool {
    peer_absorb_rate(bid, table).is_some_and(|r| r > 0.0)
}

fn peer_absorb_rate(bid: &str, table: &SkillTable) -> Option<f64> {
    table.get(bid).and_then(|s| {
        s.atoms.iter().find_map(|a| match a.action {
            Action::PeerEffAbsorb { rate_per_peer } => Some(rate_per_peer),
            _ => None,
        })
    })
}

fn has_docus_buff(op: &TradeOperatorRef<'_>, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        bid == "trade_ord_law[000]"
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "breach"
                    )
                })
            })
    })
}

fn is_tailor_beta_id(bid: &str) -> bool {
    matches!(
        bid,
        "trade_ord_wt&cost[010]" | "trade_ord_wt&cost[011]" | "trade_ord_wt&cost[012]"
    )
}

fn is_tailor_alpha_id(bid: &str) -> bool {
    matches!(
        bid,
        "trade_ord_wt&cost[000]"
            | "trade_ord_wt&cost[001]"
            | "trade_ord_wt&cost[002]"
            | "trade_ord_wt&cost[003]"
    )
}

fn has_tailor_beta(op: &TradeOperatorRef<'_>, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        is_tailor_beta_id(bid)
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "tailor_beta"
                    )
                })
            })
    })
}

fn has_tailor_alpha(op: &TradeOperatorRef<'_>, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        is_tailor_alpha_id(bid)
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "tailor_alpha"
                    )
                })
            })
    })
}

fn is_mechanic_filler(op: &TradeOperatorRef<'_>, table: &SkillTable) -> bool {
    if op.name == "巫恋" || op.name == "龙舌兰" {
        return false;
    }
    !has_tailor_beta(op, table) && !has_tailor_alpha(op, table) && !has_docus_buff(op, table)
}

fn has_blank_third(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.iter().any(|o| is_mechanic_filler(o, table))
}

fn has_closure(ops: &[TradeOperatorRef<'_>], table: &SkillTable) -> bool {
    ops.iter().any(|op| {
        if op.tier() == PromotionTier::Tier0 {
            return false;
        }
        op.buff_ids.iter().any(|bid| {
            table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "closure_special"
                    )
                })
            })
        })
    })
}

fn match_closure_shortcut(
    ops: &[TradeOperatorRef<'_>],
    table: &SkillTable,
    order_eff_pre: f64,
    _trade_level: u8,
) -> Option<TradeShortcutMatch> {
    if !has_closure(ops, table) {
        return None;
    }
    // 低语清零：同房有精二巫恋时不得按可露希尔分档短路。
    if has_witch_e2(ops, table) || room_has_docus_mechanic(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let tiers: Vec<_> = cache.closure_entries().collect();
    let station_bonus = Efficiency::from_percent_points(order_eff_pre);
    let best = tiers
        .iter()
        .min_by_key(|entry| (station_bonus.millis() - closure_tier(entry).millis()).abs())?;
    if (station_bonus.millis() - closure_tier(best).millis()).abs() > 250 {
        return None;
    }
    Some(TradeShortcutMatch {
        entry: (*best).clone(),
    })
}

fn closure_tier(entry: &TradeShortcutEntry) -> Efficiency {
    station_bonus_efficiency_anchor(entry)
}

fn station_bonus_efficiency_anchor(entry: &TradeShortcutEntry) -> Efficiency {
    entry
        .r#match
        .as_ref()
        .and_then(|m| m.station_bonus_efficiency_anchor)
        .unwrap_or(Efficiency::ZERO)
}

fn distribution_for_tier(tier: ShortcutTailorTier, level: u8) -> GoldDistribution {
    if level < 3 {
        return GoldDistribution::regular_lv3();
    }
    match tier {
        ShortcutTailorTier::Beta => GoldDistribution::beta_peak_lv3(),
        ShortcutTailorTier::Alpha => GoldDistribution::alpha_peak_lv3(),
        ShortcutTailorTier::Regular | ShortcutTailorTier::Docus => GoldDistribution::regular_lv3(),
    }
}

fn tier_params(gold: u8) -> (f64, f64) {
    match gold {
        2 => (144.0, 1000.0),
        3 => (210.0, 1500.0),
        4 => (276.0, 2000.0),
        _ => (144.0, 1000.0),
    }
}

fn long_invest_bonus_avg(tier: ShortcutTailorTier, dist: &GoldDistribution) -> f64 {
    match tier {
        ShortcutTailorTier::Beta => dist.p4 * 500.0,
        ShortcutTailorTier::Regular if dist.p4 > 0.0 => dist.p4 * 250.0,
        _ => 0.0,
    }
}

fn expected_from_dist(dist: &GoldDistribution, long_bonus: f64) -> (f64, f64) {
    let mut gold = 0.0;
    let mut mpg_weighted = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        let (dur, lmd) = tier_params(g);
        let lmd_adj = lmd + if g == 4 { long_bonus } else { 0.0 };
        gold += p * g as f64;
        mpg_weighted += p * (dur / g as f64);
        let _ = lmd_adj;
    }
    (gold, mpg_weighted)
}

impl TradeShortcutMatch {
    pub fn community_unit_trade_per_day(&self, level: u8, reference: f64) -> Option<f64> {
        self.entry
            .unit_output
            .as_ref()
            .map(|rule| rule.unit_trade_per_day(level, reference))
    }

    pub fn unit_output_from_anchor(
        &self,
        baseline_unit_trade: f64,
    ) -> Option<crate::trade::unit_output::TradeUnitOutput> {
        let ut = self.entry.unit_trade_anchor?;
        let ug = self.entry.unit_gsl_gold_anchor.unwrap_or(0.0);
        let unit_gold = ug / crate::trade::unit_output::GSL_GOLD_UNIT_SCALE;
        let mult = if baseline_unit_trade > 0.0 {
            ut / baseline_unit_trade
        } else {
            1.0
        };
        Some(crate::trade::unit_output::TradeUnitOutput {
            unit_trade_per_day: ut,
            unit_gold_per_day: unit_gold,
            gold_consumption_per_day: unit_gold,
            unit_originium_per_day: 0.0,
            multiplier_vs_lv3_regular: mult,
            drone_unit_trade_per_day: ut * crate::trade::unit_output::DRONE_TRADE_FACTOR,
            drone_unit_gold_per_day: unit_gold * crate::trade::unit_output::DRONE_TRADE_FACTOR,
            drone_unit_originium_per_day: 0.0,
        })
    }

    pub fn build_mechanic_result(&self, trade_level: u8) -> OrderMechanicResult {
        let dist = distribution_for_tier(self.entry.tailor_tier, trade_level);
        let long_avg = long_invest_bonus_avg(self.entry.tailor_tier, &dist);
        let (gold_avg, mpg) = expected_from_dist(&dist, long_avg);

        OrderMechanicResult {
            order_kind: TradeOrderKind::Gold,
            dominant_kind: SpecialOrderKind::NormalGold,
            gold_distribution: dist,
            originium_distribution: None,
            mechanic_equivalent_efficiency: self.entry.mechanic_equivalent_efficiency,
            gold_per_order_avg: gold_avg,
            originium_per_order_avg: 0.0,
            minutes_per_gold: mpg,
            minutes_per_originium_shard: 0.0,
            shortcut_id: Some(self.entry.id.clone()),
        }
    }
}
