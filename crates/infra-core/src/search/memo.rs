//! 请求内叶搜索结果 memo：完整输入精确相等才复用成功报告。
//!
//! 仅当前线程的最外层 scope 拥有状态；独立于 perf 开关，也不向 Rayon 工作线程传播。
//! 键和 intern ID 只在产生它们的 scope 内有效。编码错误由调用方退回原搜索，
//! 不应把诊断错误变成搜索失败。报告保存内存 Clone，保留响应序列化会省略的字段。

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use serde::Serialize;

use crate::bake::BakeMode;
use crate::error::Result;
use crate::layout::{LayoutContext, SharedLayout};
use crate::pool::{ManuPool, TradePool};
use crate::skill_table::SkillTable;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use crate::trade::TradeOperator;
use crate::types::{CompiledAtom, RecipeKind};

use super::exact_key::encode;
use super::{
    ManuSearchOptions, ManuSearchReport, SearchTripleFilter, TradeSearchOptions, TradeSearchReport,
};

thread_local! {
    static SEARCH_MEMO: RefCell<Option<MemoState>> = const { RefCell::new(None) };
}

/// 最外层 guard 释放请求状态；内层 guard 只借用既有 scope。
///
/// guard 绑定创建线程，须保活到请求完成（包括所有内层搜索返回）。
#[must_use = "保活 guard 到请求结束，才能复用请求内搜索 memo"]
pub struct SearchMemoScopeGuard {
    owns_scope: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

/// 为当前线程确保一个请求 scope；嵌套调用不会替换或清空既有 memo。
pub fn ensure_search_memo_scope() -> SearchMemoScopeGuard {
    let owns_scope = SEARCH_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.is_some() {
            false
        } else {
            *memo = Some(MemoState::default());
            true
        }
    });
    SearchMemoScopeGuard {
        owns_scope,
        _thread_bound: PhantomData,
    }
}

impl Drop for SearchMemoScopeGuard {
    fn drop(&mut self) {
        if self.owns_scope {
            SEARCH_MEMO.with(|memo| {
                *memo.borrow_mut() = None;
            });
        }
    }
}

#[derive(Default)]
struct MemoState {
    trade: HashMap<Vec<u8>, TradeSearchReport>,
    manufacture: HashMap<Vec<u8>, ManuSearchReport>,
    layouts: ExactIntern<LayoutContext>,
    atoms: ExactIntern<[CompiledAtom]>,
}

/// 保活所有首次见到的 Arc，防止指针复用，也让 Arc::make_mut 分配新实例。
/// bytes 的 HashMap 以完整 Vec 相等判定，不用摘要或原类型 PartialEq 代替。
struct ExactIntern<T: ?Sized> {
    instances: HashMap<(usize, usize), Retained<T>>,
    bytes: HashMap<Vec<u8>, usize>,
}

struct Retained<T: ?Sized> {
    _value: Arc<T>,
    id: usize,
}

impl<T: ?Sized> Default for ExactIntern<T> {
    fn default() -> Self {
        Self {
            instances: HashMap::new(),
            bytes: HashMap::new(),
        }
    }
}

impl<T: ?Sized> ExactIntern<T> {
    fn intern(
        &mut self,
        value: &Arc<T>,
        pointer: (usize, usize),
        encode_value: impl FnOnce(&T) -> Result<Vec<u8>>,
    ) -> Result<usize> {
        if let Some(retained) = self.instances.get(&pointer) {
            return Ok(retained.id);
        }
        let bytes = encode_value(value.as_ref())?;
        let next_id = self.bytes.len();
        let id = *self.bytes.entry(bytes).or_insert(next_id);
        self.instances.insert(
            pointer,
            Retained {
                _value: Arc::clone(value),
                id,
            },
        );
        Ok(id)
    }
}

impl MemoState {
    fn layout_id(&mut self, layout: &SharedLayout) -> Result<usize> {
        let pointer = Arc::as_ptr(layout) as usize;
        self.layouts.intern(layout, (pointer, 0), |layout| {
            // 动态规则会 clone family map 后插入；跨实例 seed/capacity 对求和顺序的
            // 等价性尚未证明，因此即使编码字段相等，也保持不同实例分离。
            #[derive(Serialize)]
            struct LayoutKey<'a> {
                layout: &'a LayoutContext,
                dynamic_instance: Option<usize>,
            }
            let dynamic = !layout.global_inject.trade_tagged().is_empty()
                || !layout.global_inject.manu_count_scaled().is_empty();
            encode(&LayoutKey {
                layout,
                dynamic_instance: dynamic.then_some(pointer),
            })
        })
    }

    fn atoms_id(&mut self, atoms: &Arc<[CompiledAtom]>) -> Result<usize> {
        // 空切片的地址可能相同；胖指针长度也是实例身份的一部分。
        let pointer = (
            Arc::as_ptr(atoms) as *const CompiledAtom as usize,
            atoms.len(),
        );
        self.atoms.intern(atoms, pointer, encode)
    }
}

#[derive(Serialize)]
struct ProgressKey {
    elite: u8,
    level: u32,
    rarity: u8,
}

impl From<crate::roster::OperatorProgress> for ProgressKey {
    fn from(progress: crate::roster::OperatorProgress) -> Self {
        Self {
            elite: progress.elite,
            level: progress.level,
            rarity: progress.rarity,
        }
    }
}

#[derive(Serialize)]
enum BakeKey {
    Auto,
    Disabled,
    Required,
}

impl From<BakeMode> for BakeKey {
    fn from(mode: BakeMode) -> Self {
        match mode {
            BakeMode::Auto => Self::Auto,
            BakeMode::Disabled => Self::Disabled,
            BakeMode::Required => Self::Required,
        }
    }
}

#[derive(Serialize)]
struct TradeEntryKey<'a> {
    name: &'a str,
    elite: u8,
    progress: ProgressKey,
    buff_ids: &'a [String],
    tags: &'a [String],
    compiled_atoms: usize,
    flat_eff_hint: f64,
    is_mechanic: bool,
    tier: crate::layout::tier::OperatorTier,
}

#[derive(Serialize)]
struct TradeOperatorKey<'a> {
    name: &'a str,
    elite: u8,
    buff_ids: &'a [String],
    tags: &'a [String],
    compiled_atoms: usize,
}

impl<'a> TradeOperatorKey<'a> {
    fn new(operator: &'a TradeOperator, memo: &mut MemoState) -> Result<Self> {
        Ok(Self {
            name: &operator.name,
            elite: operator.elite,
            buff_ids: &operator.buff_ids,
            tags: &operator.tags,
            compiled_atoms: memo.atoms_id(&operator.compiled_atoms)?,
        })
    }
}

#[derive(Serialize)]
struct TradeOptionsKey {
    trade_level: u8,
    operator_capacity: usize,
    mood: f64,
    top_k: usize,
    gold_production_lines: u32,
    layout: usize,
    shift_hours: f64,
    order_mode: TradeSearchOrderMode,
    bake_mode: BakeKey,
    full_pool: bool,
}

#[derive(Serialize)]
struct TradeFilterKey<'a> {
    must_include_name: Option<&'a str>,
    must_include_names: &'a [String],
    must_operator_override: Option<TradeOperatorKey<'a>>,
    forbidden_pairs: &'a [(String, String)],
}

#[derive(Serialize)]
struct TradeKey<'a> {
    table_identity: u64,
    pool: Vec<TradeEntryKey<'a>>,
    options: TradeOptionsKey,
    order_kind: TradeOrderKind,
    filter: TradeFilterKey<'a>,
}

/// 完整贸易叶键。Required 和任意 predicate 查询不缓存；无 scope 时不编码。
pub(crate) fn trade_key(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    filter: &SearchTripleFilter,
) -> Result<Option<Vec<u8>>> {
    if options.bake_mode == BakeMode::Required || filter.hit_filter.is_some() {
        return Ok(None);
    }
    SEARCH_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        let Some(memo) = memo.as_mut() else {
            return Ok(None);
        };
        let pool = pool
            .entries
            .iter()
            .map(|entry| {
                Ok(TradeEntryKey {
                    name: &entry.name,
                    elite: entry.elite,
                    progress: entry.progress.into(),
                    buff_ids: &entry.buff_ids,
                    tags: &entry.tags,
                    compiled_atoms: memo.atoms_id(&entry.compiled_atoms)?,
                    flat_eff_hint: entry.flat_eff_hint,
                    is_mechanic: entry.is_mechanic,
                    tier: entry.tier,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let must_operator_override = filter
            .must_operator_override
            .as_ref()
            .map(|operator| TradeOperatorKey::new(operator, memo))
            .transpose()?;
        // pool.skipped 只用于建池诊断，不参与任何叶搜索或成功 report 字段。
        encode(&TradeKey {
            table_identity: table.identity(),
            pool,
            options: TradeOptionsKey {
                trade_level: options.trade_level,
                operator_capacity: options.operator_capacity,
                mood: options.mood,
                top_k: options.top_k,
                gold_production_lines: options.gold_production_lines,
                layout: memo.layout_id(&options.layout)?,
                shift_hours: options.shift_hours,
                order_mode: options.order_mode,
                bake_mode: options.bake_mode.into(),
                full_pool: options.full_pool,
            },
            order_kind,
            filter: TradeFilterKey {
                must_include_name: filter.must_include_name.as_deref(),
                must_include_names: &filter.must_include_names,
                must_operator_override,
                forbidden_pairs: &filter.forbidden_pairs,
            },
        })
        .map(Some)
    })
}

#[derive(Serialize)]
struct ManuEntryKey<'a> {
    name: &'a str,
    elite: u8,
    progress: ProgressKey,
    buff_ids: &'a [String],
    tags: &'a [String],
    compiled_atoms: Option<usize>,
    flat_eff_hint: f64,
    has_l2_delegate: bool,
    tier: crate::layout::tier::OperatorTier,
}

#[derive(Serialize)]
struct ManuOptionsKey<'a> {
    level: u8,
    operator_capacity: usize,
    recipe_mode: crate::manufacture::ManuSearchRecipeMode,
    mood: f64,
    top_k: usize,
    layout: usize,
    shift_hours: Option<f64>,
    must_include_name: Option<&'a str>,
    bake_mode: BakeKey,
    full_pool: bool,
}

#[derive(Serialize)]
struct ManuKey<'a> {
    table_identity: u64,
    pool: Vec<ManuEntryKey<'a>>,
    options: ManuOptionsKey<'a>,
    recipe: RecipeKind,
}

/// 完整制造叶键；recipe 是已拆开的单配方，不缓存 Lines 包装器结果。
pub(crate) fn manu_key(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
    recipe: RecipeKind,
) -> Result<Option<Vec<u8>>> {
    if options.bake_mode == BakeMode::Required {
        return Ok(None);
    }
    SEARCH_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        let Some(memo) = memo.as_mut() else {
            return Ok(None);
        };
        let pool = pool
            .entries
            .iter()
            .map(|entry| {
                Ok(ManuEntryKey {
                    name: &entry.name,
                    elite: entry.elite,
                    progress: entry.progress.into(),
                    buff_ids: &entry.buff_ids,
                    tags: &entry.tags,
                    compiled_atoms: entry
                        .compiled_atoms
                        .as_ref()
                        .map(|atoms| memo.atoms_id(atoms))
                        .transpose()?,
                    flat_eff_hint: entry.flat_eff_hint,
                    has_l2_delegate: entry.has_l2_delegate,
                    tier: entry.tier,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // 与贸易相同：skipped 不参与叶搜索或成功报告。
        encode(&ManuKey {
            table_identity: table.identity(),
            pool,
            options: ManuOptionsKey {
                level: options.level,
                operator_capacity: options.operator_capacity,
                recipe_mode: options.recipe_mode,
                mood: options.mood,
                top_k: options.top_k,
                layout: memo.layout_id(&options.layout)?,
                shift_hours: options.shift_hours,
                must_include_name: options.must_include_name.as_deref(),
                bake_mode: options.bake_mode.into(),
                full_pool: options.full_pool,
            },
            recipe,
        })
        .map(Some)
    })
}

pub(crate) fn trade_get(key: &[u8]) -> Option<TradeSearchReport> {
    SEARCH_MEMO.with(|memo| memo.borrow().as_ref()?.trade.get(key).cloned())
}

/// 只接收成功报告；elapsed 命中口径由调用方更新。
pub(crate) fn trade_insert(key: Vec<u8>, report: &TradeSearchReport) {
    SEARCH_MEMO.with(|memo| {
        if let Some(memo) = memo.borrow_mut().as_mut() {
            memo.trade.insert(key, report.clone());
        }
    });
}

pub(crate) fn manu_get(key: &[u8]) -> Option<ManuSearchReport> {
    SEARCH_MEMO.with(|memo| memo.borrow().as_ref()?.manufacture.get(key).cloned())
}

/// 只接收成功报告；elapsed 命中口径由调用方更新。
pub(crate) fn manu_insert(key: Vec<u8>, report: &ManuSearchReport) {
    SEARCH_MEMO.with(|memo| {
        if let Some(memo) = memo.borrow_mut().as_mut() {
            memo.manufacture.insert(key, report.clone());
        }
    });
}
