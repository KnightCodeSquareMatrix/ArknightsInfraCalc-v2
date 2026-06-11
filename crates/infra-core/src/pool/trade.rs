use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::roster::Roster;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::TradeOperator;
use crate::types::{Action, Phase, SkillDef};

#[derive(Debug, Clone)]
pub struct TradePoolEntry {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    /// Sum of `AddFlatEff` in `constant` phase — sort hint only, not final score.
    pub flat_eff_hint: f64,
    pub is_mechanic: bool,
}

impl TradePoolEntry {
    pub fn to_trade_operator(&self) -> TradeOperator {
        TradeOperator {
            name: self.name.clone(),
            elite: self.elite,
            buff_ids: self.buff_ids.clone(),
            tags: self.tags.clone(),
        }
    }
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

#[derive(Debug, Clone)]
pub struct TradePool {
    pub entries: Vec<TradePoolEntry>,
    pub skipped: Vec<(String, u8, PoolSkip)>,
}

impl TradePool {
    pub fn stats(&self) -> PoolStats {
        let n = self.entries.len();
        PoolStats {
            ready: n,
            skipped: self.skipped.len(),
            combinations_3: n_choose_k_u64(n, 3),
        }
    }

    pub fn entry(&self, name: &str) -> Option<&TradePoolEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

pub fn build_trade_pool(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<TradePool> {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();

    for name in roster.names() {
        let Some(elite) = roster.elite(name) else {
            continue;
        };
        match try_entry(name, elite, instances, table) {
            Ok(entry) => entries.push(entry),
            Err(skip) => skipped.push((name.clone(), elite, skip)),
        }
    }

    entries.sort_by(|a, b| {
        b.flat_eff_hint
            .partial_cmp(&a.flat_eff_hint)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(TradePool { entries, skipped })
}

fn try_entry(
    name: &str,
    elite: u8,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> std::result::Result<TradePoolEntry, PoolSkip> {
    let tier = PromotionTier::from_elite(elite);
    let inst = instances.get(name, tier);
    if inst.is_none_or(|i| !i.facilities.contains_key("trade")) {
        return Err(PoolSkip::NoTradeBinding);
    }

    let buff_ids = instances.resolve_trade_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return Err(PoolSkip::NoTradeBinding);
    }

    let mut flat_eff_hint = 0.0;
    let mut is_mechanic = false;
    for bid in &buff_ids {
        let Some(skill) = table.get(bid) else {
            return Err(PoolSkip::UnmodeledBuff(bid.clone()));
        };
        let (flat, mech) = skill_hints(skill);
        flat_eff_hint += flat;
        is_mechanic |= mech;
    }

    let tags = inst
        .map(|i| i.tags.clone())
        .unwrap_or_default();

    Ok(TradePoolEntry {
        name: name.to_string(),
        elite,
        buff_ids,
        tags,
        flat_eff_hint,
        is_mechanic,
    })
}

fn skill_hints(skill: &SkillDef) -> (f64, bool) {
    let mut flat = 0.0;
    let mut mech = false;
    for atom in &skill.atoms {
        if atom.phase == Phase::Constant {
            if let Action::AddFlatEff { value } = atom.action {
                flat += value;
            }
        }
        if atom.phase == Phase::OrderMechanic {
            mech = true;
        }
        if matches!(atom.action, Action::ReplaceOrder { .. }) {
            mech = true;
        }
    }
    if is_gold_flow_skill(&skill.id) {
        mech = true;
    }
    (flat, mech)
}

fn is_gold_flow_skill(id: &str) -> bool {
    id.contains("line_gold")
        || id.contains("spd&gold")
        || id.contains("line_durin")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn fixture_pool() -> TradePool {
        let roster = Roster::load_csv_for_facility(
            &crate::roster::default_roster_path().unwrap(),
            "trade",
        )
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
        assert!(ex.buff_ids.contains(&"trade_ord_spd[010]".to_string()));
        assert!(ex.buff_ids.contains(&"trade_ord_spd[020]".to_string()));
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
        assert!(pool.entry("鸿雪").is_some());
        assert!(pool.entry("绮良").is_some());
        assert!(pool.entry("铎铃").is_some());
    }

    #[test]
    fn n_choose_k_matches_small_cases() {
        assert_eq!(n_choose_k_u64(4, 3), 4);
        assert_eq!(n_choose_k_u64(10, 3), 120);
    }
}
