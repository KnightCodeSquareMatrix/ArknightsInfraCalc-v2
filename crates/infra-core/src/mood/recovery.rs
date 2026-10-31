//! 宿舍每小时心情回复速率（每个在住干员一个值）。
//!
//! 组装（来自公孙长乐规范文档）：
//! ```text
//! 每人回复 = 宿舍基线(查表)
//!          + 群回(group：同宿全员，同类取最高)
//!          + 单回(single：按进驻顺序精确锁定一名受益者，同类取最高)
//!          + 自回(self：仅自身)
//! ```
//! - 同类取最高、不同类相加。
//! - 菲亚梅塔·自律：自身固定 `no_external`，不吃基线/群回/单回，回复恒为其 value。
//! - 单回受益者按进驻顺序快照：宿管在前 → 锁其后第一名非宿管室友；宿管在后 → 已进驻者里当前心情最低的非宿管。
//!   本函数给稳态速率，用「消耗最快 = 稳态最需要」近似「心情最低」；ETA 逐时模拟里再用真实心情细化。
//! - 冰酿（pool，总额 0.8 平摊）：按当前受益人数拆成等额群回，留 TODO 精细化，这里按「未满心情人数」平摊。

use std::collections::HashMap;

use super::MoodModel;

/// 宿舍在住干员上下文。`drain_hint` 为该干员工作时的净消耗（用于单回「最需要」近似）；不参与回复计算的可填 0。
pub struct DormOccupant {
    pub name: String,
    pub elite: u8,
    /// 工作消耗速率提示：越大越「需要」被单回优先照顾。
    pub drain_hint: f64,
}

/// 计算一间宿舍每个在住干员的每小时心情回复速率。
/// `occupants` 顺序即进驻顺序（与 assignment 的 `RoomAssignment.operators` 一致）。
pub fn dorm_recovery_rates(
    model: &MoodModel,
    dorm_level: u8,
    occupants: &[DormOccupant],
) -> HashMap<String, f64> {
    let base = model.dorm_base_recovery(dorm_level);
    let n = occupants.len();
    let mut out: HashMap<String, f64> = HashMap::new();

    // 预取每人宿舍技能。
    let skills: Vec<Vec<&super::DormRecoverySkill>> = occupants
        .iter()
        .map(|o| model.dorm_skills(&o.name, o.elite))
        .collect();

    // 群回：同宿全员，同类取最高（含 also_group 附带的群回）。
    let mut group_max = 0.0_f64;
    for s in skills.iter().flat_map(|entries| entries.iter().copied()) {
        if s.is_group() {
            group_max = group_max.max(s.value);
        }
        if let Some(g) = s.also_group {
            group_max = group_max.max(g);
        }
    }

    // 单回：按进驻顺序锁定受益者，同类多个取最高值的那一个宿管，受益者唯一。
    let single_beneficiary = resolve_single_recovery(occupants, &skills);

    // 冰酿 pool：总额 value 平摊给「非宿管且未满心情」——稳态里按非提供者人数平摊近似。
    let pool_total: f64 = skills
        .iter()
        .flat_map(|entries| entries.iter().copied())
        .filter(|s| s.is_pool())
        .map(|s| s.value)
        .fold(0.0, f64::max);

    for (i, occ) in occupants.iter().enumerate() {
        // 菲亚梅塔自律：不吃任何外部来源，恒为自身 value。
        if let Some(s) = skills[i].iter().copied().find(|s| s.no_external) {
            out.insert(occ.name.clone(), s.value);
            continue;
        }

        let mut rate = base + group_max;

        // 自回以及各技能附带的自身回复修正；不同类型可以叠加。
        for s in &skills[i] {
            if s.is_self() {
                rate += s.value;
            }
            if let Some(sd) = s.self_delta {
                rate += sd;
            }
        }

        // 单回：仅锁定的受益者吃。
        if let Some((idx, val)) = single_beneficiary {
            if idx == i {
                rate += val;
            }
        }

        // pool 平摊：非提供 pool 的人均分。
        if pool_total > 0.0 {
            let providers = skills
                .iter()
                .filter(|entries| entries.iter().any(|s| s.is_pool()))
                .count()
                .max(1);
            let receivers = n.saturating_sub(providers).max(1);
            let is_provider = skills[i].iter().any(|s| s.is_pool());
            if !is_provider {
                rate += pool_total / receivers as f64;
            }
        }

        out.insert(occ.name.clone(), rate);
    }

    out
}

/// 按进驻顺序锁定单回受益者，返回 `(occupant_index, recovery_value)`。
///
/// 规则：取值最高的单回宿管为提供者；
/// - 若该宿管**之后**有非宿管室友 → 锁其后第一名非宿管；
/// - 否则从**其之前**已进驻者里挑「最需要」（drain_hint 最大）的非宿管。
fn resolve_single_recovery(
    occupants: &[DormOccupant],
    skills: &[Vec<&super::DormRecoverySkill>],
) -> Option<(usize, f64)> {
    // 选出提供单回的宿管：值最高者（同类取最高）；并列取先进驻者。
    let provider = skills
        .iter()
        .enumerate()
        .flat_map(|(i, entries)| {
            entries
                .iter()
                .copied()
                .filter(|s| s.is_single())
                .map(move |s| (i, s.value))
        })
        .fold(None::<(usize, f64)>, |acc, (i, v)| match acc {
            Some((_, bv)) if bv >= v => acc,
            _ => Some((i, v)),
        })?;
    let (provider_idx, value) = provider;

    let is_manager = |i: usize| skills[i].iter().any(|s| s.is_manager());

    // 宿管之后第一名非宿管。
    if let Some(after) = (provider_idx + 1..occupants.len()).find(|&i| !is_manager(i)) {
        return Some((after, value));
    }
    // 否则之前已进驻的非宿管里挑 drain_hint 最大者。
    (0..provider_idx)
        .filter(|&i| !is_manager(i))
        .max_by(|&a, &b| {
            occupants[a]
                .drain_hint
                .partial_cmp(&occupants[b].drain_hint)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|i| (i, value))
}
