//! 完整状态组合的 checked colex 索引；不改变解算输入顺序。

use crate::error::{Error, Result};

pub(super) const INVALID_ROW: usize = usize::MAX;

pub(super) fn choose(n: usize, k: usize) -> Result<usize> {
    if k > 3 || k == 0 {
        return Err(Error::msg("Bake rank supports capacities 1..=3"));
    }
    if k > n {
        return Ok(0);
    }
    let mut value = 1u128;
    for i in 1..=k {
        value = value
            .checked_mul((n - k + i) as u128)
            .ok_or_else(|| Error::msg("Bake rank array size overflow"))?
            / i as u128;
    }
    usize::try_from(value).map_err(|_| Error::msg("Bake rank array size exceeds usize"))
}

pub(super) fn colex(ids: &[usize]) -> Result<usize> {
    if ids.is_empty() || ids.len() > 3 {
        return Err(Error::msg("Bake rank supports capacities 1..=3"));
    }
    let mut sorted = [0; 3];
    sorted[..ids.len()].copy_from_slice(ids);
    sorted[..ids.len()].sort_unstable();
    if sorted[..ids.len()]
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(Error::msg("Bake rank contains duplicate state IDs"));
    }
    let mut rank = 0usize;
    for (i, id) in sorted[..ids.len()].iter().enumerate() {
        rank = rank
            .checked_add(choose(*id, i + 1)?)
            .ok_or_else(|| Error::msg("Bake combo rank overflow"))?;
    }
    Ok(rank)
}

pub(super) fn empty_rows(state_count: usize, capacity: usize) -> Result<Vec<usize>> {
    let len = choose(state_count, capacity)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(len).map_err(|error| {
        Error::msg(format!(
            "cannot allocate Bake rank array ({len} slots): {error}"
        ))
    })?;
    rows.resize(len, INVALID_ROW);
    Ok(rows)
}

/// 与原 combinations_indices 相同的字典序；每个组合使用固定三槽。
pub(super) fn combinations(n: usize, k: usize) -> impl Iterator<Item = [usize; 3]> {
    let mut combo = [0, 1, 2];
    let mut started = false;
    let mut done = k == 0 || k > 3 || k > n;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        if !started {
            started = true;
            return Some(combo);
        }
        for i in (0..k).rev() {
            if combo[i] < n - k + i {
                combo[i] += 1;
                for j in i + 1..k {
                    combo[j] = combo[j - 1] + 1;
                }
                return Some(combo);
            }
        }
        done = true;
        None
    })
}
