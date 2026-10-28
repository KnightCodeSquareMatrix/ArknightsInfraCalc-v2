//! Bake 全表的运行时身份与真实准入变体；尚未接入 catalog 生成或查询。

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::pool::{
    build_manufacture_pool, build_trade_pool, HasName, ManuPool, ManuPoolEntry, PoolCore,
    TradePool, TradePoolEntry,
};
use crate::roster::{OperatorProgress, Roster};
use crate::skill_table::SkillTable;

/// 身份保留实际 buff 与标签输入顺序，不按姓名合并不同运行时状态。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct OperatorState {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
}

impl OperatorState {
    pub fn from_trade_entry(entry: &TradePoolEntry) -> Self {
        Self {
            name: entry.name.clone(),
            elite: entry.elite,
            buff_ids: entry.buff_ids.clone(),
            tags: entry.tags.clone(),
        }
    }

    pub fn from_manu_entry(entry: &ManuPoolEntry) -> Self {
        Self {
            name: entry.name.clone(),
            elite: entry.elite,
            buff_ids: entry.buff_ids.clone(),
            tags: entry.tags.clone(),
        }
    }
}

/// 同名可有多个运行时变体；组合消费者必须检查姓名互异。
#[derive(Debug, Clone)]
pub(super) struct VariantPools {
    pub trade: TradePool,
    pub manufacture: ManuPool,
}

// 覆盖 tier_0 的 E0/E1、低星等级解锁，以及 tier_up 的 E1/E2。
const REPRESENTATIVE_PROGRESS: [OperatorProgress; 5] = [
    OperatorProgress {
        elite: 0,
        level: 1,
        rarity: 0,
    },
    OperatorProgress {
        elite: 1,
        level: 1,
        rarity: 0,
    },
    OperatorProgress {
        elite: 0,
        level: 30,
        rarity: 1,
    },
    OperatorProgress {
        elite: 1,
        level: 1,
        rarity: 3,
    },
    OperatorProgress {
        elite: 2,
        level: 1,
        rarity: 0,
    },
];

pub(super) fn build_variant_pools(
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<VariantPools> {
    let mut trade_names = BTreeSet::new();
    let mut manufacture_names = BTreeSet::new();
    for (key, instance) in instances.iter() {
        // 使用与 OperatorInstances::get 相同的真实键，不信任 payload 的姓名。
        let Some((name, tier)) = key.rsplit_once('@') else {
            continue;
        };
        if !matches!(tier, "tier_0" | "tier_up") {
            continue;
        }
        if instance.facilities.contains_key("trade") {
            trade_names.insert(name);
        }
        if instance.facilities.contains_key("manufacture") {
            manufacture_names.insert(name);
        }
    }
    let trade = build_variants(
        trade_names,
        instances,
        table,
        build_trade_pool,
        OperatorState::from_trade_entry,
        |entry| entry.flat_eff_hint,
    )?;
    let manufacture = build_variants(
        manufacture_names,
        instances,
        table,
        build_manufacture_pool,
        OperatorState::from_manu_entry,
        |entry| entry.flat_eff_hint,
    )?;
    Ok(VariantPools { trade, manufacture })
}

fn build_variants<T: HasName>(
    names: BTreeSet<&str>,
    instances: &OperatorInstances,
    table: &SkillTable,
    builder: fn(&Roster, &OperatorInstances, &SkillTable) -> Result<PoolCore<T>>,
    identity: fn(&T) -> OperatorState,
    flat_hint: impl Fn(&T) -> f64,
) -> Result<PoolCore<T>> {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        for progress in REPRESENTATIVE_PROGRESS {
            let mut roster = Roster::default();
            roster.insert(name, progress);
            let pool = builder(&roster, instances, table)?;
            for entry in pool.entries {
                let state = identity(&entry);
                if seen.insert(state.clone()) {
                    entries.push((state, entry));
                }
            }
            skipped.extend(pool.skipped);
        }
    }
    entries.sort_by(|(a_state, a), (b_state, b)| {
        flat_hint(b)
            .partial_cmp(&flat_hint(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.pool_name().cmp(b.pool_name()))
            .then_with(|| a_state.elite.cmp(&b_state.elite))
            .then_with(|| a_state.buff_ids.cmp(&b_state.buff_ids))
            .then_with(|| a_state.tags.cmp(&b_state.tags))
    });
    Ok(PoolCore {
        entries: entries.into_iter().map(|(_, entry)| entry).collect(),
        skipped,
    })
}

/// 防止全表同名变体在同一组合占据多个位置。
pub(super) fn distinct_names<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();
    names.into_iter().all(|name| seen.insert(name))
}
