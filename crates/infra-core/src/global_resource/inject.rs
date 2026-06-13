use std::collections::HashMap;

use crate::types::RecipeKind;

/// 灵知·精密计算：每名同贸易房谢拉格干员的订单效率/上限增量规则。
/// 控制域只携带系数；最终值在贸易域按房间内 `cc.g.karlan` 计数结算。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KarlanPrecision {
    pub eff_per_karlan: f64,
    pub limit_per_karlan: i32,
}

/// 中枢 `GlobalInject` 汇总：同 `inject_family` 取最高，不同族相加。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalInjectManifest {
    trade_by_family: HashMap<String, f64>,
    manu_all_by_family: HashMap<String, f64>,
    manu_gold_by_family: HashMap<String, f64>,
    manu_battle_record_by_family: HashMap<String, f64>,
    manu_originium_by_family: HashMap<String, f64>,
    /// 灵知·精密计算规则（单一中枢，不与自身叠加）。
    karlan_precision: Option<KarlanPrecision>,
    /// 中枢八幡海铃 E2「家族认可」已进驻（叙拉古但书链段 producer；不含贸易站计数）。
    haru_e2_in_control: bool,
}

#[derive(Debug, Clone, Copy)]
enum ManuInjectSlot {
    All,
    Gold,
    BattleRecord,
    Originium,
}

impl GlobalInjectManifest {
    pub fn trade_eff_pct(&self) -> f64 {
        self.trade_by_family.values().sum()
    }

    pub fn manu_eff_for(&self, recipe: RecipeKind) -> f64 {
        let all: f64 = self.manu_all_by_family.values().sum();
        all + match recipe {
            RecipeKind::All => 0.0,
            RecipeKind::Gold => self.manu_gold_by_family.values().sum(),
            RecipeKind::BattleRecord => self.manu_battle_record_by_family.values().sum(),
            RecipeKind::Originium => self.manu_originium_by_family.values().sum(),
        }
    }

    pub fn record_trade(&mut self, family: &str, value: f64) {
        let entry = self.trade_by_family.entry(family.to_string()).or_insert(0.0);
        *entry = entry.max(value);
    }

    pub fn record_manu(&mut self, family: &str, recipe: Option<RecipeKind>, value: f64) {
        let slot = match recipe {
            None | Some(RecipeKind::All) => ManuInjectSlot::All,
            Some(RecipeKind::Gold) => ManuInjectSlot::Gold,
            Some(RecipeKind::BattleRecord) => ManuInjectSlot::BattleRecord,
            Some(RecipeKind::Originium) => ManuInjectSlot::Originium,
        };
        let map = match slot {
            ManuInjectSlot::All => &mut self.manu_all_by_family,
            ManuInjectSlot::Gold => &mut self.manu_gold_by_family,
            ManuInjectSlot::BattleRecord => &mut self.manu_battle_record_by_family,
            ManuInjectSlot::Originium => &mut self.manu_originium_by_family,
        };
        let entry = map.entry(family.to_string()).or_insert(0.0);
        *entry = entry.max(value);
    }

    /// 记录灵知·精密计算；单一中枢不叠加，取订单上限增益更大的一条。
    pub fn record_karlan_precision(&mut self, eff_per_karlan: f64, limit_per_karlan: i32) {
        let cand = KarlanPrecision {
            eff_per_karlan,
            limit_per_karlan,
        };
        self.karlan_precision = Some(match self.karlan_precision {
            Some(cur) if cur.limit_per_karlan >= limit_per_karlan => cur,
            _ => cand,
        });
    }

    pub fn karlan_precision(&self) -> Option<KarlanPrecision> {
        self.karlan_precision
    }

    /// 叙拉古但书链段：中枢有八幡海铃 E2 时激活；贸易站三人组不全则仍走 `gsl_docus_solo` fallback。
    pub fn record_haru_e2_in_control(&mut self) {
        self.haru_e2_in_control = true;
    }

    pub fn haru_e2_in_control(&self) -> bool {
        self.haru_e2_in_control
    }
}

/// 默认注入族：全贸易站 flat %（阿米娅/诗怀雅/明椒/阿斯卡纶等）。
pub const INJECT_FAMILY_TRADE_GLOBAL_FLAT: &str = "trade_global_flat";

/// 默认注入族：全制造站 flat %（凯尔希/Mon3tr 等）。
pub const INJECT_FAMILY_MANU_GLOBAL_ALL: &str = "manu_global_all";
