use std::collections::HashSet;
use std::sync::Arc;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::roster::{OperatorProgress, Roster};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::TradeOperator;
use crate::types::{Action, CompiledAtom, Phase, SkillDef};

use crate::layout::tier::OperatorTier;

use super::base::{build_roster_pool, filter_pool, HasName, PoolCore, TierTagged};

/// 建池时按 buff 展开并排序 atom，供 solve 热路径归并。
pub fn compile_operator_atoms(buff_ids: &[String], table: &SkillTable) -> Arc<[CompiledAtom]> {
    let mut atoms = Vec::new();
    let mut seq = 0u16;
    for bid in buff_ids {
        let Some(skill) = table.get(bid) else {
            continue;
        };
        for atom in &skill.atoms {
            atoms.push(CompiledAtom {
                atom: atom.clone(),
                sort_key: (atom.phase.sort_key(), atom.phase_order),
                seq,
            });
            seq = seq.saturating_add(1);
        }
    }
    atoms.sort_by(|a, b| a.sort_key.cmp(&b.sort_key).then(a.seq.cmp(&b.seq)));
    atoms.into()
}

/// 贸易站精0 孑（摊贩）；轮换余量班强制用此带队，无视 operbox 更高练度。
pub const JIE_TRADE_NAME: &str = "孑";

/// 控制中枢灵知·精密计算已激活（贸易房按谢拉格人数注入 ±效率/上限）。
pub fn karlan_precision_active(inject: &crate::global_resource::GlobalInjectManifest) -> bool {
    inject.karlan_precision().is_some()
}

pub fn jie_e0_trade_operator(
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Option<TradeOperator> {
    let buff_ids = instances.resolve_trade_buff_ids(JIE_TRADE_NAME, PromotionTier::Tier0);
    if buff_ids.is_empty() {
        return None;
    }
    let mut op = TradeOperator::new(JIE_TRADE_NAME, 0, buff_ids.clone());
    op.compiled_atoms = compile_operator_atoms(&buff_ids, table);
    Some(op)
}

/// 精1+ 孑（市井之道）；默认不进通用池，仅在灵知精密计算等明确场景中注入。
pub fn jie_market_trade_operator(
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Option<TradeOperator> {
    const JIE_MARKET_BUFF: &str = "trade_ord_limit_count[000]";
    let buff_ids = instances.resolve_trade_buff_ids(JIE_TRADE_NAME, PromotionTier::TierUp);
    if !buff_ids.iter().any(|b| b == JIE_MARKET_BUFF) {
        return None;
    }
    // 市井之道与摊贩经济是精0/精1替换关系，但它们的 buff stem 不同，通用
    // stepwise 合并会把两者都带上。灵知线注入的精1+ 孑必须只使用市井之道。
    let buff_ids = vec![JIE_MARKET_BUFF.to_string()];
    // BaseAssignment only stores `elite`, and resolve currently maps unknown rarity
    // tier_up by elite>=2. Keep the injected 市井孑 stable across search -> assign -> resolve.
    let mut op = TradeOperator::new(JIE_TRADE_NAME, 2, buff_ids);
    op.compiled_atoms = compile_operator_atoms(&op.buff_ids, table);
    Some(op)
}

pub fn jie_market_trade_pool_entry(
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Option<TradePoolEntry> {
    let op = jie_market_trade_operator(instances, table)?;
    Some(TradePoolEntry {
        name: op.name,
        elite: op.elite,
        progress: OperatorProgress::new(op.elite, 1, 4),
        buff_ids: op.buff_ids,
        tags: op.tags,
        compiled_atoms: op.compiled_atoms,
        flat_eff_hint: 0.0,
        is_mechanic: true,
        tier: OperatorTier::CrossStation,
    })
}

pub fn add_jie_market_to_trade_pool(
    pool: &mut TradePool,
    instances: &OperatorInstances,
    table: &SkillTable,
) {
    if pool.entry(JIE_TRADE_NAME).is_some() {
        return;
    }
    let Some(entry) = jie_market_trade_pool_entry(instances, table) else {
        return;
    };
    pool.entries.push(entry);
    pool.entries.sort_by(|a, b| {
        b.flat_eff_hint
            .partial_cmp(&a.flat_eff_hint)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
}

#[derive(Debug, Clone)]
pub struct TradePoolEntry {
    pub name: String,
    pub elite: u8,
    pub progress: OperatorProgress,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    pub compiled_atoms: Arc<[CompiledAtom]>,
    /// Sum of `AddFlatEff` in `constant` phase — sort hint only, not final score.
    pub flat_eff_hint: f64,
    pub is_mechanic: bool,
    pub tier: OperatorTier,
}

impl HasName for TradePoolEntry {
    fn pool_name(&self) -> &str {
        &self.name
    }
}

impl TierTagged for TradePoolEntry {
    fn tier(&self) -> OperatorTier {
        self.tier
    }

    fn set_tier(&mut self, tier: OperatorTier) {
        self.tier = tier;
    }
}

impl TradePoolEntry {
    pub fn to_trade_operator(&self) -> TradeOperator {
        TradeOperator {
            name: self.name.clone(),
            elite: self.elite,
            buff_ids: self.buff_ids.clone(),
            tags: self.tags.clone(),
            compiled_atoms: self.compiled_atoms.clone(),
        }
    }
}

/// 向后兼容别名：`TradePool` = `PoolCore<TradePoolEntry>`
pub type TradePool = PoolCore<TradePoolEntry>;

/// 从池索引组装三人组（保留进驻顺序）；孑 E0 override 由调用方注入。
pub fn build_trade_combo_operators(
    pool: &TradePool,
    combo: [usize; 3],
    must_name: Option<&str>,
    override_op: Option<&TradeOperator>,
) -> [TradeOperator; 3] {
    std::array::from_fn(|slot| {
        let entry = &pool.entries[combo[slot]];
        if must_name.is_some_and(|n| entry.name == n) {
            override_op
                .cloned()
                .unwrap_or_else(|| entry.to_trade_operator())
        } else {
            entry.to_trade_operator()
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolSkip {
    NoTradeBinding,
    UnmodeledBuff(String),
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub ready: usize,
    pub skipped: usize,
    pub combinations_3: u64,
}

pub fn build_trade_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<TradePool> {
    build_roster_pool(roster, instances, table, |e| e.flat_eff_hint, try_entry)
}

/// Sub-pool excluding operators already assigned in the same shift.
pub fn filter_trade_pool(pool: &TradePool, exclude: &HashSet<String>) -> TradePool {
    filter_pool(pool, exclude)
}

fn try_entry(
    name: &str,
    progress: crate::roster::OperatorProgress,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> std::result::Result<TradePoolEntry, PoolSkip> {
    let tier = PromotionTier::from_progress(progress);
    let inst = instances.get(name, tier);
    if inst.is_none_or(|i| !i.facilities.contains_key("trade")) {
        return Err(PoolSkip::NoTradeBinding);
    }

    let buff_ids = instances.resolve_trade_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return Err(PoolSkip::NoTradeBinding);
    }

    // 精1+ 孑（市井）不进通用池；仅恢复班 `jie_e0_trade_operator` 或灵知线固定注入。
    if name == JIE_TRADE_NAME && progress.elite > 0 {
        return Err(PoolSkip::NoTradeBinding);
    }

    let mut flat_eff_hint = 0.0;
    let mut is_mechanic = false;
    for bid in &buff_ids {
        let Some(skill) = table.get(bid) else {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        };
        let (flat, mech) = trade_skill_hints(skill);
        flat_eff_hint += flat;
        is_mechanic |= mech;
    }

    let tags = inst.map(|i| i.tags.clone()).unwrap_or_default();

    Ok(TradePoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids: buff_ids.clone(),
        tags,
        compiled_atoms: compile_operator_atoms(&buff_ids, table),
        flat_eff_hint,
        is_mechanic,
        tier: OperatorTier::Standalone,
    })
}

fn trade_skill_hints(skill: &SkillDef) -> (f64, bool) {
    let mut flat = 0.0;
    let mut mech = false;
    for atom in &skill.atoms {
        if atom.phase == Phase::Constant {
            // 有条件的 flat（如蕾缪安「相伴」+25% 需能天使同房）不计入 solo hint。
            if atom.condition.is_some() {
                continue;
            }
            if let Action::AddFlatEff { value, .. } = atom.action {
                flat += value;
            }
        }
        // 贸易站三人组 hint：peer_share 按 2 名队友估算（如吉星勤俭经营·β）。
        if atom.phase == Phase::PeerShare {
            if let Action::AddFlatEffFromSelector { multiplier, .. } = atom.action {
                flat += multiplier * 2.0;
            }
        }
        if atom.phase == Phase::OrderMechanic {
            mech = true;
        }
    }
    if is_gold_flow_skill(&skill.id) {
        mech = true;
    }
    (flat, mech)
}

fn is_gold_flow_skill(id: &str) -> bool {
    id.contains("line_gold") || id.contains("spd&gold") || id.contains("line_durin")
}

pub fn n_choose_k_u64(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut c = 1u64;
    for i in 0..k {
        c = c.saturating_mul((n - i) as u64) / (i + 1) as u64;
    }
    c
}

/// Stream all index combinations of size `k` from `n` items.
pub fn combinations_indices(n: usize, k: usize) -> impl Iterator<Item = Vec<usize>> {
    let mut state = (false, vec![0usize; k]);
    std::iter::from_fn(move || {
        let (started, combo) = &mut state;
        if k == 0 {
            return if !*started {
                *started = true;
                Some(vec![])
            } else {
                None
            };
        }
        if k > n {
            return None;
        }
        if !*started {
            for (i, slot) in combo.iter_mut().enumerate() {
                *slot = i;
            }
            *started = true;
            return Some(combo.clone());
        }
        let mut i = k;
        while i > 0 {
            i -= 1;
            if combo[i] != i + n - k {
                combo[i] += 1;
                for j in i + 1..k {
                    combo[j] = combo[j - 1] + 1;
                }
                return Some(combo.clone());
            }
        }
        None
    })
}

/// `C(n,3)` 零堆分配枚举（`k=3` 热路径专用）。
pub fn combinations_triples(n: usize) -> CombinationsTripleIter {
    CombinationsTripleIter {
        n,
        combo: [0, 1, 2],
        started: false,
        done: n < 3,
    }
}

#[derive(Debug, Clone)]
pub struct CombinationsTripleIter {
    n: usize,
    combo: [usize; 3],
    started: bool,
    done: bool,
}

impl Iterator for CombinationsTripleIter {
    type Item = [usize; 3];

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if !self.started {
            self.started = true;
            return Some(self.combo);
        }
        let k = 3usize;
        let mut i = k;
        while i > 0 {
            i -= 1;
            if self.combo[i] != i + self.n - k {
                self.combo[i] += 1;
                for j in i + 1..k {
                    self.combo[j] = self.combo[j - 1] + 1;
                }
                return Some(self.combo);
            }
        }
        self.done = true;
        None
    }
}

/// 固定锚点干员 + 从其余池成员中选 2 人（孑带队等）。
pub fn combinations_triples_with_anchor(
    n: usize,
    anchor: usize,
) -> impl Iterator<Item = [usize; 3]> {
    let mut i = 0usize;
    let mut j = 1usize;
    std::iter::from_fn(move || {
        while i < n {
            while j < n {
                if i != anchor && j != anchor && i < j {
                    let out = [anchor, i, j];
                    j += 1;
                    return Some(out);
                }
                j += 1;
            }
            i += 1;
            j = i + 1;
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn fixture_pool() -> TradePool {
        let roster =
            Roster::load_csv_for_facility(&crate::roster::default_roster_path().unwrap(), "trade")
                .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        build_trade_pool(&roster, &instances, &table).unwrap()
    }

    #[test]
    fn docus_and_pilots_ready_in_pool() {
        let pool = fixture_pool();
        assert!(pool.entry("但书").is_some());
        assert!(pool.entry("德克萨斯").is_some());
        assert!(pool.entry("能天使").is_some());
    }

    #[test]
    fn exusiai_e2_expands_stepwise_buffs() {
        let pool = fixture_pool();
        let ex = pool.entry("能天使").expect("能天使");
        assert_eq!(
            ex.buff_ids,
            vec!["trade_ord_spd[020]".to_string()],
            "精2 仅物流专家，不得叠精0 企鹅物流·α"
        );
    }

    #[test]
    fn witch_and_tailor_operators_ready_in_pool() {
        let pool = fixture_pool();
        let wl = pool.entry("巫恋").expect("巫恋");
        assert!(wl.buff_ids.contains(&"trade_ord_vodfox[000]".to_string()));
        assert!(wl.buff_ids.contains(&"trade_ord_wt&cost[000]".to_string()));
        assert!(pool.entry("龙舌兰").is_some());
        assert!(pool.entry("折光").is_some());
        assert!(pool.entry("琳琅诗怀雅").is_some());
        assert!(pool.entry("柏喙").is_some());
    }

    #[test]
    fn gongsun_roster_fully_ready() {
        let roster = Roster::load_csv_for_facility(
            &crate::skill_table::data_path("roster_gongsun.csv").unwrap(),
            "trade",
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        assert_eq!(pool.skipped.len(), 0, "{:?}", pool.skipped);
    }

    #[test]
    fn heidi_e2_flat_eff_hint_single_skill() {
        use crate::roster::OperatorProgress;

        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut roster = Roster::default();
        roster.insert("海蒂", OperatorProgress::elite_only(2));
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let heidi = pool.entry("海蒂").expect("海蒂");
        assert_eq!(heidi.buff_ids, vec!["trade_ord_spd[021]".to_string()]);
        assert!(
            (heidi.flat_eff_hint - 35.0).abs() < f64::EPSILON,
            "名流欢会 flat_eff_hint 应仅 35%，got {}",
            heidi.flat_eff_hint
        );
    }

    #[test]
    fn jixing_e2_flat_eff_hint_includes_peer_share() {
        use crate::roster::OperatorProgress;

        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut roster = Roster::default();
        roster.insert("吉星", OperatorProgress::elite_only(2));
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let jixing = pool.entry("吉星").expect("吉星");
        assert!(
            (jixing.flat_eff_hint - 40.0).abs() < f64::EPSILON,
            "勤俭经营·β 三人站 hint 应≈2×20%=40%，got {}",
            jixing.flat_eff_hint
        );
    }

    #[test]
    fn lemuen_e2_flat_eff_hint_excludes_exusiai_bonus() {
        use crate::roster::OperatorProgress;

        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut roster = Roster::default();
        roster.insert("蕾缪安", OperatorProgress::elite_only(2));
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let lemuen = pool.entry("蕾缪安").expect("蕾缪安");
        assert_eq!(
            lemuen.buff_ids,
            vec!["trade_ord_spd&multiPar[100]".to_string()]
        );
        assert!(
            (lemuen.flat_eff_hint - 20.0).abs() < f64::EPSILON,
            "相伴无能天使时 flat_eff_hint 应仅 20%，got {}",
            lemuen.flat_eff_hint
        );
    }

    #[test]
    fn elite_jie_excluded_from_general_trade_pool() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut roster = Roster::default();
        roster.insert("孑", crate::roster::OperatorProgress::elite_only(2));
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        assert!(pool.entry("孑").is_none(), "精1+ 孑应从通用贸易池排除");
    }

    #[test]
    fn jie_market_injection_uses_market_skill_only() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let op = jie_market_trade_operator(&instances, &table).expect("市井孑");

        assert_eq!(
            op.buff_ids,
            vec!["trade_ord_limit_count[000]".to_string()],
            "灵知线注入的是精1 市井之道，不能同时叠精0 摊贩经济"
        );
        assert_eq!(
            op.elite, 2,
            "assignment 只有 elite 字段；市井孑需在 resolve 时保持 tier_up"
        );
    }
}
