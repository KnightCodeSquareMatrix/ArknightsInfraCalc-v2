use std::collections::HashSet;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::roster::Roster;
use crate::skill_table::SkillTable;

use super::trade::{n_choose_k_u64, PoolSkip, PoolStats};

/// PoolEntry 必须提供名称供 `entry()` 查找。
pub trait HasName {
    fn pool_name(&self) -> &str;
}

/// 泛型池核心：消除了 4 个 `*Pool` 结构体 + `stats()` + `entry()` 的复制粘贴。
///
/// 每个设施只需：
///   1. 定义 `*PoolEntry + HasName`
///   2. 定义设施特定的 `try_entry()`
///   3. 用 `build_roster_pool()` 替代 `build_*_pool()`
///   4. 用 `type FooPool = PoolCore<FooPoolEntry>` 替代手动 struct
///   5. 用 `filter_pool()` 替代 `filter_*_pool()`
#[derive(Debug, Clone)]
pub struct PoolCore<T> {
    pub entries: Vec<T>,
    pub skipped: Vec<(String, u8, PoolSkip)>,
}

impl<T: HasName> PoolCore<T> {
    /// `combinations_k`：C(n, k)；贸易/制造用 3，power 用 1。
    pub fn stats(&self, combinations_k: usize) -> PoolStats {
        let n = self.entries.len();
        PoolStats {
            ready: n,
            skipped: self.skipped.len(),
            combinations_3: n_choose_k_u64(n, combinations_k),
        }
    }

    pub fn entry(&self, name: &str) -> Option<&T> {
        self.entries.iter().find(|e| e.pool_name() == name)
    }
}

/// 从 roster 建池的通用迭代骨架：遍历名册 → 逐个 try_entry → 排序。
///
/// `sort_key`：排序键提取闭包（降序，同键按 name 升序）
/// `try_entry`：设施特定的 try_entry
pub fn build_roster_pool<T, F, S>(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    sort_key: S,
    try_entry: F,
) -> Result<PoolCore<T>>
where
    F: Fn(&str, crate::roster::OperatorProgress, &OperatorInstances, &SkillTable)
        -> std::result::Result<T, PoolSkip>,
    S: Fn(&T) -> f64,
    T: HasName,
{
    let mut entries = Vec::new();
    let mut skipped = Vec::new();

    for name in roster.names() {
        let Some(progress) = roster.progress(name) else {
            continue;
        };
        match try_entry(name, progress, instances, table) {
            Ok(entry) => entries.push(entry),
            Err(skip) => skipped.push((name.clone(), progress.elite, skip)),
        }
    }

    entries.sort_by(|a, b| {
        sort_key(b)
            .partial_cmp(&sort_key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.pool_name().cmp(b.pool_name()))
    });

    Ok(PoolCore { entries, skipped })
}

/// 通用过滤：排除已分配的干员（sub-pool）。
pub fn filter_pool<T: HasName + Clone>(pool: &PoolCore<T>, exclude: &HashSet<String>) -> PoolCore<T> {
    PoolCore {
        entries: pool
            .entries
            .iter()
            .filter(|e| !exclude.contains(e.pool_name()))
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}
