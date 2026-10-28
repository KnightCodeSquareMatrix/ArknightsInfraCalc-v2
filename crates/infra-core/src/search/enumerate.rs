use std::sync::Arc;

use rayon::prelude::*;

use crate::profile::{start_hot_path_batch, HotPathCounters};

/// 与 `combinations_indices` 同序；只收集包含全部必选下标的组合。
/// 一次分配平铺结果，`k` 个下标一组，房间人数限制为 1..=3。
pub(super) fn flat_combinations(n: usize, k: usize, must_indices: &[usize]) -> Vec<usize> {
    if k == 0 || k > n || k > 3 {
        return Vec::new();
    }
    let required = must_indices
        .iter()
        .enumerate()
        .filter(|(i, index)| !must_indices[..*i].contains(index))
        .count();
    if required > k || must_indices.iter().any(|index| *index >= n) {
        return Vec::new();
    }
    let count = crate::pool::n_choose_k_u64(n - required, k - required);
    let capacity = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(k))
        .expect("flat combination capacity overflow");
    let mut out = Vec::with_capacity(capacity);
    let mut combo = [0usize; 3];
    for (i, slot) in combo.iter_mut().enumerate().take(k) {
        *slot = i;
    }
    loop {
        if must_indices.iter().all(|index| combo[..k].contains(index)) {
            out.extend_from_slice(&combo[..k]);
        }
        let mut i = k;
        let mut advanced = false;
        while i > 0 {
            i -= 1;
            if combo[i] != i + n - k {
                combo[i] += 1;
                for j in i + 1..k {
                    combo[j] = combo[j - 1] + 1;
                }
                advanced = true;
                break;
            }
        }
        if !advanced {
            break;
        }
    }
    out
}

/// 域内闭包求值与过滤，保留平铺组合的 indexed 顺序及任务计数 batch。
pub(super) fn evaluate_flat_combinations<T, Eval, Accept>(
    flat: &[usize],
    k: usize,
    batch_counters: &Arc<HotPathCounters>,
    evaluate: Eval,
    accept: Accept,
) -> Vec<T>
where
    T: Send,
    Eval: Fn(usize, &[usize]) -> Option<T> + Sync + Send,
    Accept: Fn(&T) -> bool + Sync + Send,
{
    if k == 0 || k > 3 {
        return Vec::new();
    }
    flat.par_chunks(k)
        .enumerate()
        .map_init(
            || start_hot_path_batch(Arc::clone(batch_counters)),
            |_, (index, combo)| evaluate(index, combo),
        )
        .flatten()
        .filter(accept)
        .collect()
}
