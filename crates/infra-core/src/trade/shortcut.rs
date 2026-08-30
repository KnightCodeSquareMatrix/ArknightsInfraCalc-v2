use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::global_resource::GlobalInjectManifest;
use crate::skill_table::data_path;
use crate::skill_table::SkillTable;
use crate::trade::input::{TradeOperator, TradeOrderKind};
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
    room_has_docus_mechanic(ops, table) && room_has_witch_side_group(ops, table)
}

/// 佩佩独占站：同房不得进驻提供订单获取效率% 的干员（工具人位应填上限/心情等）。
pub fn pepe_station_trade_eff_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    if !room_has_pepe_exclusive(ops, table) {
        return false;
    }
    ops.iter()
        .any(|o| o.name != "佩佩" && operator_constant_trade_flat_eff(o, table) > 0.0)
}

/// 贸易站同房互斥（公孙长乐）：违约 / 特别订单 / 巫恋低语 / 佩佩+效率人 不得混排。
pub fn trade_station_exclusive_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    crate::profile::record_exclusive_check();
    let docus = room_has_docus_mechanic(ops, table);
    let closure = has_closure(ops, table);
    let witch = has_witch_e2(ops, table);
    let pepe = room_has_pepe_exclusive(ops, table);

    if docus_tailor_exclusive_violation(ops, table) {
        return true;
    }
    if pepe_station_trade_eff_violation(ops, table) {
        return true;
    }
    if pepe && (docus || closure || room_has_witch_side_group(ops, table)) {
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
    ops.len() >= 3 && room_has_docus_mechanic(ops, table) && !room_has_witch_side_group(ops, table)
}

/// 叙拉古但书链段 consumer：但书 + 伺夜 + 贝洛内（无巫恋侧）。
pub fn is_docus_syracusa_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    is_docus_solo_station(ops, table)
        && ops.iter().any(|o| o.name == "伺夜")
        && ops.iter().any(|o| o.name == "贝洛内")
}

/// 链段 producer 已满足（中枢八幡海铃 E2）。
pub fn docus_syracusa_segment_active(inject: &GlobalInjectManifest) -> bool {
    inject.has_active_source_buff("control_tra_limit&spd2[000]")
}

/// 可露希尔特别订单站（不含但书/巫恋低语）。
pub fn is_closure_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
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
    resolve_trade_shortcut_inner(ops, table, order_eff_pre, trade_level, inject, true)
}

/// 搜索热路径：调用方已做互斥预筛时跳过重复校验。
pub(crate) fn resolve_trade_shortcut_prevalidated(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    resolve_trade_shortcut_inner(ops, table, order_eff_pre, trade_level, inject, false)
}

fn resolve_trade_shortcut_inner(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
    check_exclusivity: bool,
) -> Option<TradeShortcutMatch> {
    if check_exclusivity && trade_station_exclusive_violation(ops, table) {
        return None;
    }
    if let Some(m) = crate::trade::segment::match_registered_trade_segment(ops, table, inject) {
        return Some(m);
    }
    if let Some(m) = match_docus_solo_shortcut(ops, table) {
        return Some(m);
    }
    if let Some(m) = match_witch_group_shortcut(ops, table, trade_level) {
        return Some(m);
    }
    if let Some(m) = match_blackkey_closure_shortcut(ops, table) {
        return Some(m);
    }
    // 推王龙门的 135% 贸易效率是戴菲恩中枢 producer 激活后的 vault 满配值，
    // 只能通过 trade_segments 的 producer-gated 路径命中；无戴菲恩 fallback 需另建锚点。
    if let Some(m) = match_penguin_exusiai_lemuen_shortcut(ops, table) {
        return Some(m);
    }
    if let Some(m) = match_penguin_texangel_e2_shortcut(ops, table) {
        return Some(m);
    }
    if let Some(m) = match_penguin_texlap_e0_shortcut(ops, table) {
        return Some(m);
    }
    match_closure_shortcut(ops, table, order_eff_pre, trade_level)
}

/// 公孙感知链贸站：黑键 E2（乐感）+ 可露希尔 closure，第三人任意高效散件；不与但书/巫恋同房。
pub fn is_blackkey_closure_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.len() >= 3
        && room_has_blackkey_perception(ops, table)
        && has_closure(ops, table)
        && !room_has_witch_side_group(ops, table)
        && !room_has_docus_mechanic(ops, table)
}

pub fn match_blackkey_closure_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_blackkey_closure_station(ops, table) {
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
    ops.len() >= 3
        && ops.iter().any(|o| o.name == "德克萨斯")
        && ops.iter().any(|o| o.name == "拉普兰德")
        && !penguin_bond_excluded(ops, table)
}

pub fn match_penguin_texlap_e0_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_penguin_texlap_e0_station(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_penguin_texlap_e0")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 德克萨斯 E2「默契」+ 能天使；不含蕾缪安 E2 相伴链。
pub fn is_penguin_texangel_e2_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.len() >= 3
        && room_has_named_buff(ops, "德克萨斯", TEX_MOQI_BUFF)
        && ops.iter().any(|o| o.name == "能天使")
        && !ops.iter().any(|o| o.name == "蕾缪安" && o.elite >= 2)
        && !penguin_bond_excluded(ops, table)
}

pub fn match_penguin_texangel_e2_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_penguin_texangel_e2_station(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_penguin_texangel_e2")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 能天使 + 蕾缪安 E2「相伴」；第三人任意。
pub fn is_penguin_exusiai_lemuen_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.len() >= 3
        && ops.iter().any(|o| o.name == "能天使")
        && ops.iter().any(|o| o.name == "蕾缪安" && o.elite >= 2)
        && !penguin_bond_excluded(ops, table)
}

pub fn match_penguin_exusiai_lemuen_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_penguin_exusiai_lemuen_station(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_penguin_exusiai_lemuen")?.clone();
    Some(TradeShortcutMatch { entry })
}

/// 推王组：推进之王 + 摩根 + 维娜·维多利亚（需中枢戴菲恩 E2 producer）。
pub fn is_vina_lungmen_station(ops: &[TradeOperator], _table: &SkillTable) -> bool {
    ops.len() >= 3
        && ops.iter().any(|o| o.name == "推进之王")
        && ops.iter().any(|o| o.name == "摩根")
        && ops.iter().any(|o| o.name == "维娜·维多利亚")
}

pub fn match_vina_lungmen_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    if !is_vina_lungmen_station(ops, table) {
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
    if !is_docus_solo_station(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let docus_e2 = docus_operator_is_e2(ops, table);
    let shortcut_id = if docus_e2 {
        "gsl_docus_solo"
    } else {
        "gsl_docus_solo_e0"
    };
    let entry = cache.get_by_id(shortcut_id)?.clone();
    Some(TradeShortcutMatch { entry })
}

pub(crate) fn docus_operator_is_e2(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter()
        .find(|operator| has_docus_buff(operator, table))
        .is_some_and(|operator| operator.elite >= 2)
}

/// 叙拉古但书链段：委托 `trade/segment` 注册表（保留 API 供测试对照）。
pub fn match_docus_syracusa_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    crate::trade::segment::match_registered_trade_segment(ops, table, inject)
        .filter(|m| m.entry.id == "gsl_docus_syracusa")
}

pub fn match_witch_group_shortcut(
    ops: &[TradeOperator],
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

fn classify_witch_room(ops: &[TradeOperator], table: &SkillTable) -> Option<WitchRoomKind> {
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
    let long_tier_up = long.is_some_and(|o| o.elite >= 2);
    let long_tier_0 = long.is_some_and(|o| o.elite < 2);

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

fn room_has_docus_mechanic(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| has_docus_buff(o, table))
}

fn room_has_named_buff(ops: &[TradeOperator], name: &str, buff: &str) -> bool {
    ops.iter()
        .any(|o| o.name == name && o.buff_ids.iter().any(|b| b == buff))
}

fn penguin_bond_excluded(ops: &[TradeOperator], table: &SkillTable) -> bool {
    room_has_docus_mechanic(ops, table)
        || room_has_witch_side_group(ops, table)
        || is_blackkey_closure_station(ops, table)
        || has_closure(ops, table)
}

fn room_has_blackkey_perception(ops: &[TradeOperator], _table: &SkillTable) -> bool {
    ops.iter()
        .any(|op| op.elite >= 2 && op.buff_ids.iter().any(|b| b == BLACKKEY_PERCEPTION_BUFF))
}

fn room_has_pepe_exclusive(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| has_pepe_exclusive_buff(o, table))
}

fn has_pepe_exclusive_buff(op: &TradeOperator, table: &SkillTable) -> bool {
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
fn operator_constant_trade_flat_eff(op: &TradeOperator, table: &SkillTable) -> f64 {
    let mut flat = 0.0;
    for bid in &op.buff_ids {
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

fn has_long_invest_buff(op: &TradeOperator, table: &SkillTable) -> bool {
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

fn find_op<'a>(ops: &'a [TradeOperator], name: &str) -> Option<&'a TradeOperator> {
    ops.iter().find(|o| o.name == name)
}

fn has_witch_e2(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| {
        o.name == "巫恋"
            && o.elite >= 2
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

fn has_docus_buff(op: &TradeOperator, table: &SkillTable) -> bool {
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

fn has_tailor_beta(op: &TradeOperator, table: &SkillTable) -> bool {
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

fn has_tailor_alpha(op: &TradeOperator, table: &SkillTable) -> bool {
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

fn is_mechanic_filler(op: &TradeOperator, table: &SkillTable) -> bool {
    if op.name == "巫恋" || op.name == "龙舌兰" {
        return false;
    }
    !has_tailor_beta(op, table) && !has_tailor_alpha(op, table) && !has_docus_buff(op, table)
}

fn has_blank_third(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| is_mechanic_filler(o, table))
}

fn has_closure(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|op| {
        if op.elite < 2 {
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
    ops: &[TradeOperator],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::OperatorInstances;
    use crate::skill_table::default_skill_table_path;

    fn table() -> SkillTable {
        SkillTable::load(&default_skill_table_path().unwrap()).unwrap()
    }

    fn mk_op(name: &str, elite: u8, buff_ids: Vec<&str>) -> TradeOperator {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    }

    #[test]
    fn gsl_penguin_exusiai_lemuen_shortcut() {
        let table = table();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let exu = instances.resolve_trade_buff_ids("能天使", crate::tier::PromotionTier::TierUp);
        let lemuen = instances.resolve_trade_buff_ids("蕾缪安", crate::tier::PromotionTier::TierUp);
        let ops = vec![
            mk_op("能天使", 2, exu.iter().map(String::as_str).collect()),
            mk_op("蕾缪安", 2, lemuen.iter().map(String::as_str).collect()),
            mk_op("芬", 0, vec!["trade_ord_spd[000]"]),
        ];
        let m = match_penguin_exusiai_lemuen_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_penguin_exusiai_lemuen");
        let resolved =
            resolve_trade_shortcut(&ops, &table, 70.0, 3, &GlobalInjectManifest::default())
                .expect("resolve");
        assert_eq!(resolved.entry.id, "gsl_penguin_exusiai_lemuen");
    }

    #[test]
    fn gsl_penguin_texlap_e0_shortcut() {
        let table = table();
        let ops = vec![
            mk_op("德克萨斯", 0, vec!["trade_ord_spd&cost_P[000]"]),
            mk_op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]),
            mk_op("芬", 0, vec!["trade_ord_spd[000]"]),
        ];
        let m = match_penguin_texlap_e0_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_penguin_texlap_e0");
    }

    #[test]
    fn gsl_penguin_texlap_allows_texas_e2_enemy_route() {
        let table = table();
        let ops = vec![
            mk_op("德克萨斯", 2, vec!["trade_ord_limit&cost_P[010]"]),
            mk_op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]),
            mk_op("芬", 0, vec!["trade_ord_spd[000]"]),
        ];
        let m = match_penguin_texlap_e0_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_penguin_texlap_e0");
    }

    #[test]
    fn gsl_vina_lungmen_shortcut_requires_daifeen_producer_for_segment() {
        let table = table();
        let ops = vec![
            mk_op("推进之王", 2, vec!["trade_ord_spd[010]"]),
            mk_op("摩根", 2, vec!["trade_ord_spd[010]"]),
            mk_op("维娜·维多利亚", 2, vec!["trade_ord_spd[010]"]),
        ];
        let without_producer =
            resolve_trade_shortcut(&ops, &table, 80.0, 3, &GlobalInjectManifest::default());
        assert!(
            without_producer.is_none(),
            "推王龙门 135% 满配锚点必须由戴菲恩中枢 producer 激活"
        );
        let mut inject = GlobalInjectManifest::default();
        inject.record_active_source_buff("control_tra_limit&spd[010]");
        let with_segment = resolve_trade_shortcut(&ops, &table, 80.0, 3, &inject).expect("segment");
        assert_eq!(with_segment.entry.id, "gsl_vina_lungmen");
        assert_eq!(with_segment.entry.id, "gsl_vina_lungmen");
    }

    #[test]
    fn gsl_witch_long_beta_shortcut() {
        let table = table();
        let ops = vec![
            mk_op(
                "巫恋",
                2,
                vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
            ),
            mk_op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
            mk_op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
        ];
        let m = match_witch_group_shortcut(&ops, &table, 3).expect("match");
        assert_eq!(m.entry.id, "gsl_witch_long_beta");
        assert_eq!(
            m.entry.mechanic_equivalent_efficiency,
            Efficiency::from_decimal(0.460)
        );
    }

    #[test]
    fn gsl_witch_long_alpha_uses_tequila_elite_mapping() {
        let table = table();
        let witch = mk_op(
            "巫恋",
            2,
            vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
        );
        let tailor = mk_op("柏喙", 0, vec!["trade_ord_wt&cost[000]"]);
        let e0 = match_witch_group_shortcut(
            &[
                witch.clone(),
                mk_op("龙舌兰", 0, vec!["trade_ord_long[000]"]),
                tailor.clone(),
            ],
            &table,
            3,
        )
        .expect("E0 Tequila alpha");
        assert_eq!(e0.entry.id, "gsl_witch_long0_alpha");
        assert_eq!(
            e0.community_unit_trade_per_day(3, 10_265.0),
            Some(11_331.04)
        );
        assert_eq!(e0.entry.unit_gsl_gold_anchor, Some(957.76));

        let e0_beta = match_witch_group_shortcut(
            &[
                witch.clone(),
                mk_op("龙舌兰", 0, vec!["trade_ord_long[000]"]),
                mk_op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
            ],
            &table,
            3,
        )
        .expect("E0 Tequila beta");
        assert_eq!(e0_beta.entry.id, "gsl_witch_long0_beta");
        assert_eq!(
            e0_beta.community_unit_trade_per_day(3, 10_265.0),
            Some(11_575.34)
        );
        assert_eq!(e0_beta.entry.unit_gsl_gold_anchor, Some(1_164.384));

        assert!(match_witch_group_shortcut(
            &[
                witch.clone(),
                mk_op("龙舌兰", 0, vec!["trade_ord_long[000]"]),
                tailor.clone(),
            ],
            &table,
            2,
        )
        .is_none());

        let e2 = match_witch_group_shortcut(
            &[
                witch,
                mk_op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
                tailor,
            ],
            &table,
            3,
        )
        .expect("E2 Tequila alpha");
        assert_eq!(e2.entry.id, "gsl_witch_long_alpha");
        assert_eq!(
            e2.community_unit_trade_per_day(3, 10_265.0),
            Some(12_030.46)
        );
        assert_eq!(e2.entry.unit_gsl_gold_anchor, Some(1_675.127));
    }

    #[test]
    fn docus_and_tailor_group_are_mutually_exclusive() {
        let table = table();
        let witch_long_docus = vec![
            mk_op(
                "巫恋",
                2,
                vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
            ),
            mk_op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
        ];
        assert!(docus_tailor_exclusive_violation(&witch_long_docus, &table));
        assert!(match_witch_group_shortcut(&witch_long_docus, &table, 3).is_none());

        let docus_solo = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op(
                "能天使",
                2,
                vec!["trade_ord_spd[010]", "trade_ord_spd[020]"],
            ),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        assert!(is_docus_solo_station(&docus_solo, &table));
        assert!(!docus_tailor_exclusive_violation(&docus_solo, &table));
    }

    #[test]
    fn gsl_witch_beta_blank_shortcut() {
        let table = table();
        let ops = vec![
            mk_op(
                "巫恋",
                2,
                vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
            ),
            mk_op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
            mk_op("古米", 0, vec!["trade_ord_spd&cost[000]"]),
        ];
        let m = match_witch_group_shortcut(&ops, &table, 3).expect("match");
        assert_eq!(m.entry.id, "gsl_witch_beta_blank");
        assert_eq!(m.entry.mechanic_equivalent_efficiency, Efficiency::ZERO);
    }

    #[test]
    fn gsl_closure_tier90_still_works() {
        let table = table();
        let ops = vec![
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("能天使", 2, vec!["trade_ord_spd[020]"]),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        let m = resolve_trade_shortcut(&ops, &table, 114.0, 3, &GlobalInjectManifest::default())
            .expect("match");
        assert_eq!(m.entry.id, "gsl_closure_tier90");
    }

    #[test]
    fn gsl_blackkey_closure_shortcut() {
        let table = table();
        let ops = vec![
            mk_op(
                "黑键",
                2,
                vec!["trade_ord_spd_bd_n1[000]", "trade_ord_spd_bd[010]"],
            ),
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("吉星", 2, vec!["trade_ord_spd&share[002]"]),
        ];
        let m = match_blackkey_closure_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_blackkey_closure");
        assert_eq!(
            m.entry.mechanic_equivalent_efficiency,
            Efficiency::from_decimal(0.384)
        );
        let resolved =
            resolve_trade_shortcut(&ops, &table, 82.0, 3, &GlobalInjectManifest::default())
                .expect("resolve");
        assert_eq!(resolved.entry.id, "gsl_blackkey_closure");
    }

    #[test]
    fn blackkey_closure_beats_generic_closure_tier() {
        let table = table();
        let ops = vec![
            mk_op(
                "黑键",
                2,
                vec!["trade_ord_spd_bd_n1[000]", "trade_ord_spd_bd[010]"],
            ),
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("吉星", 2, vec!["trade_ord_spd&share[002]"]),
        ];
        assert!(is_blackkey_closure_station(&ops, &table));
        let m = resolve_trade_shortcut(&ops, &table, 82.0, 3, &GlobalInjectManifest::default())
            .expect("match");
        assert_ne!(m.entry.id, "gsl_closure_tier90");
        assert_eq!(m.entry.id, "gsl_blackkey_closure");
    }

    #[test]
    fn closure_and_witch_e2_are_mutually_exclusive() {
        let table = table();
        let mix = vec![
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op(
                "巫恋",
                2,
                vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
            ),
            mk_op("银灰", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
        ];
        assert!(trade_station_exclusive_violation(&mix, &table));
        assert!(
            resolve_trade_shortcut(&mix, &table, 93.0, 3, &GlobalInjectManifest::default())
                .is_none()
        );
        assert!(match_closure_shortcut(&mix, &table, 134.0, 3).is_none());
        assert!(match_witch_group_shortcut(&mix, &table, 3).is_none());
    }

    #[test]
    fn witch_blank_without_long_uses_no_closure_shortcut() {
        let table = table();
        let ops = vec![
            mk_op(
                "巫恋",
                2,
                vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
            ),
            mk_op("银灰", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
            mk_op(
                "能天使",
                2,
                vec!["trade_ord_spd[010]", "trade_ord_spd[020]"],
            ),
        ];
        assert!(!trade_station_exclusive_violation(&ops, &table));
        assert!(
            resolve_trade_shortcut(&ops, &table, 93.0, 3, &GlobalInjectManifest::default())
                .is_none()
        );
    }

    #[test]
    fn gsl_docus_syracusa_requires_haru_and_trio() {
        use crate::instances::{default_instances_path, OperatorInstances};
        use crate::pool::build_trade_pool;
        use crate::roster::Roster;

        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("但书", 2), ("伺夜", 2), ("贝洛内", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let mk = |names: &[&str]| -> Vec<TradeOperator> {
            names
                .iter()
                .map(|n| pool.entry(n).unwrap().to_trade_operator())
                .collect()
        };
        let trio = mk(&["但书", "伺夜", "贝洛内"]);

        let mut with_haru = GlobalInjectManifest::default();
        with_haru.record_active_source_buff("control_tra_limit&spd2[000]");
        let m = resolve_trade_shortcut(&trio, &table, 80.0, 3, &with_haru).expect("match");
        assert_eq!(m.entry.id, "gsl_docus_syracusa");
        assert_eq!(
            m.entry.mechanic_equivalent_efficiency,
            Efficiency::from_decimal(0.550)
        );

        let without_haru = GlobalInjectManifest::default();
        let m2 = resolve_trade_shortcut(&trio, &table, 80.0, 3, &without_haru).expect("match");
        assert_eq!(m2.entry.id, "gsl_docus_solo");
        assert_eq!(
            m2.entry.mechanic_equivalent_efficiency,
            Efficiency::from_decimal(0.550)
        );
    }

    #[test]
    fn gsl_docus_syracusa_falls_back_when_trio_incomplete() {
        let table = table();
        let ops = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op("维娜·维多利亚", 2, vec!["trade_ord_spd&par[001]"]),
            mk_op("泰拉大陆调查团", 2, vec!["trade_ord_spd&limit&bd[000]"]),
        ];
        let mut inject = GlobalInjectManifest::default();
        inject.record_active_source_buff("control_tra_limit&spd2[000]");
        let pre = 55.0;
        let m = resolve_trade_shortcut(&ops, &table, pre, 3, &inject).expect("match");
        assert_eq!(m.entry.id, "gsl_docus_solo");
        assert_eq!(
            m.entry.mechanic_equivalent_efficiency,
            Efficiency::from_decimal(0.550)
        );
    }

    #[test]
    fn gsl_docus_solo_uses_pre_eff_times_155() {
        let table = table();
        let ops = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op(
                "能天使",
                2,
                vec!["trade_ord_spd[010]", "trade_ord_spd[020]"],
            ),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        let pre = 83.0;
        let m = resolve_trade_shortcut(&ops, &table, pre, 3, &GlobalInjectManifest::default())
            .expect("match");
        assert_eq!(m.entry.id, "gsl_docus_solo");
        assert_eq!(
            m.entry.mechanic_equivalent_efficiency,
            Efficiency::from_decimal(0.550)
        );
    }

    #[test]
    fn docus_and_closure_are_mutually_exclusive() {
        let table = table();
        let mix = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op(
                "能天使",
                2,
                vec!["trade_ord_spd[010]", "trade_ord_spd[020]"],
            ),
        ];
        assert!(trade_station_exclusive_violation(&mix, &table));
    }

    #[test]
    fn pepe_and_closure_are_mutually_exclusive() {
        let table = table();
        let mix = vec![
            mk_op(
                "佩佩",
                2,
                vec!["trade_ord_limit&trade&lv[000]", "trade_ord_pepe[000]"],
            ),
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("黑键", 2, vec!["trade_ord_spd_bd[010]"]),
        ];
        assert!(trade_station_exclusive_violation(&mix, &table));
    }

    #[test]
    fn community_unit_output_rules_are_sourced_and_complete() {
        let entries = load_trade_shortcuts(&default_shortcuts_path().unwrap()).unwrap();
        for entry in &entries {
            if entry_requires_community_unit_output(entry) {
                assert!(entry.unit_output.is_some(), "{}", entry.id);
                assert!(entry.unit_output_source.is_some(), "{}", entry.id);
                assert!(entry.unit_output_precision.is_some(), "{}", entry.id);
            }
        }
    }

    #[test]
    fn missing_special_unit_output_is_rejected() {
        let entry: TradeShortcutEntry = serde_json::from_str(
            r#"{
                "id":"broken_closure",
                "label":"broken",
                "mechanic_equivalent_efficiency":0.0,
                "match":{"kind":"closure"}
            }"#,
        )
        .unwrap();
        let err = validate_trade_shortcuts(&[entry]).unwrap_err();
        assert!(err.contains("no community unit_output"), "{err}");
    }
}
