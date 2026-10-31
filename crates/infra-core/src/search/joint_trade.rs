use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JointTradeRow {
    pub logical_mask: Vec<u64>,
}

/// Returns the lexicographically first pairwise-disjoint index tuple.
///
/// Each list must contain the complete row universe for one room under the
/// same response signature and must already be sorted by that room's final
/// comparator. The traversal changes search order only; it does not prune.
pub(crate) fn first_mask_disjoint_tuple(
    sorted_lists: &[Vec<JointTradeRow>],
    prefix_mask: &[u64],
) -> Option<Vec<usize>> {
    if sorted_lists.is_empty() {
        return Some(Vec::new());
    }
    if sorted_lists.iter().any(Vec::is_empty) {
        return None;
    }

    let initial = vec![0; sorted_lists.len()];
    let mut heap = BinaryHeap::from([Reverse(initial.clone())]);
    let mut visited = HashSet::from([initial]);

    while let Some(Reverse(indices)) = heap.pop() {
        if tuple_masks_are_disjoint(sorted_lists, &indices, prefix_mask) {
            return Some(indices);
        }
        for dimension in 0..indices.len() {
            let mut next = indices.clone();
            next[dimension] += 1;
            if next[dimension] < sorted_lists[dimension].len() && visited.insert(next.clone()) {
                heap.push(Reverse(next));
            }
        }
    }
    None
}

fn tuple_masks_are_disjoint(
    lists: &[Vec<JointTradeRow>],
    indices: &[usize],
    prefix_mask: &[u64],
) -> bool {
    let width = prefix_mask.len().max(
        lists
            .iter()
            .flat_map(|list| list.iter())
            .map(|row| row.logical_mask.len())
            .max()
            .unwrap_or(0),
    );
    let mut occupied = vec![0; width];
    occupied[..prefix_mask.len()].copy_from_slice(prefix_mask);
    for (list, index) in lists.iter().zip(indices) {
        let row = &list[*index];
        for (word, bits) in row.logical_mask.iter().copied().enumerate() {
            if occupied[word] & bits != 0 {
                return false;
            }
            occupied[word] |= bits;
        }
    }
    true
}
