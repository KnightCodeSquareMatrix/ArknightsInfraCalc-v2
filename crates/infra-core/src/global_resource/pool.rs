use std::collections::HashMap;

use super::key::GlobalResourceKey;
use super::registry::CONVERSIONS;

/// 全基建全局资源池：producer 写入、consumer 读取、同房 StateWrite 可在快照上叠加。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalResourcePool {
    values: HashMap<GlobalResourceKey, f64>,
}

impl GlobalResourcePool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_value(key: GlobalResourceKey, amount: f64) -> Self {
        let mut pool = Self::new();
        pool.set(key, amount);
        pool
    }

    pub fn get(&self, key: GlobalResourceKey) -> f64 {
        self.values.get(&key).copied().unwrap_or(0.0)
    }

    pub fn get_u8(&self, key: GlobalResourceKey) -> u8 {
        self.get(key).max(0.0).min(255.0) as u8
    }

    pub fn get_u32(&self, key: GlobalResourceKey) -> u32 {
        self.get(key).max(0.0).min(u32::MAX as f64) as u32
    }

    pub fn set(&mut self, key: GlobalResourceKey, amount: f64) {
        if amount == 0.0 {
            self.values.remove(&key);
        } else {
            self.values.insert(key, amount);
        }
    }

    pub fn add(&mut self, key: GlobalResourceKey, delta: f64) {
        if delta == 0.0 {
            return;
        }
        let next = self.get(key) + delta;
        self.set(key, next);
    }

    pub fn produce(&mut self, key: GlobalResourceKey, amount: f64) {
        self.add(key, amount);
    }

    /// 尝试扣减；不足时返回 false 且不修改。
    pub fn try_consume(&mut self, key: GlobalResourceKey, amount: f64) -> bool {
        if amount <= 0.0 {
            return true;
        }
        let cur = self.get(key);
        if cur + f64::EPSILON < amount {
            return false;
        }
        self.set(key, cur - amount);
        true
    }

    pub fn contains(&self, key: GlobalResourceKey) -> bool {
        self.values.contains_key(&key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (GlobalResourceKey, f64)> + '_ {
        self.values.iter().map(|(&k, &v)| (k, v))
    }

    /// 将另一池非零项叠加到本池（全基建多 producer 汇总）。
    pub fn merge(&mut self, other: &Self) {
        for (key, value) in other.iter() {
            self.add(key, value);
        }
    }

    /// 同房求解用的可变快照（StateWrite / StateConvert 在此 HashMap 上操作）。
    pub fn to_room_state(&self) -> HashMap<GlobalResourceKey, f64> {
        self.values.clone()
    }

    /// 物理发电站数 + 虚拟发电站资源。
    pub fn effective_power_station_count(&self, physical: u8) -> u8 {
        physical.saturating_add(self.get_u8(GlobalResourceKey::VirtualPower))
    }

    /// 从同房 state 回写全局池（保留接口；当前搜索路径多为只读快照）。
    pub fn absorb_room_state(&mut self, room: &HashMap<GlobalResourceKey, f64>) {
        for (key, value) in room {
            self.set(*key, *value);
        }
    }

    /// 按 `CONVERSIONS` 注册表执行全局资源转化（固定点迭代）。
    pub fn run_conversions(&mut self) {
        loop {
            let mut changed = false;
            for conv in CONVERSIONS {
                let from_amount = self.get(conv.from);
                if from_amount + f64::EPSILON < conv.from_per {
                    continue;
                }
                let times = (from_amount / conv.from_per).floor();
                if times < 1.0 {
                    continue;
                }
                let consume = times * conv.from_per;
                let produce = times * conv.to_per;
                self.add(conv.from, -consume);
                self.add(conv.to, produce);
                changed = true;
            }
            if !changed {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_power_stacks_on_physical() {
        let pool = GlobalResourcePool::with_value(GlobalResourceKey::VirtualPower, 2.0);
        assert_eq!(pool.effective_power_station_count(3), 5);
    }

    #[test]
    fn merge_and_consume() {
        let mut base = GlobalResourcePool::with_value(GlobalResourceKey::Matatabi, 4.0);
        let mut extra = GlobalResourcePool::with_value(GlobalResourceKey::Matatabi, 2.0);
        extra.set(GlobalResourceKey::Perception, 10.0);
        base.merge(&extra);
        assert_eq!(base.get(GlobalResourceKey::Matatabi), 6.0);
        assert!(base.try_consume(GlobalResourceKey::Matatabi, 5.0));
        assert_eq!(base.get(GlobalResourceKey::Matatabi), 1.0);
        assert!(!base.try_consume(GlobalResourceKey::Matatabi, 2.0));
    }

    #[test]
    fn conversions_human_fireworks_to_witchcraft() {
        let mut pool = GlobalResourcePool::with_value(GlobalResourceKey::HumanFireworks, 10.0);
        pool.run_conversions();
        assert!((pool.get(GlobalResourceKey::HumanFireworks) - 0.0).abs() < f64::EPSILON);
        assert!((pool.get(GlobalResourceKey::WitchcraftCrystal) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn room_state_roundtrip() {
        let pool = GlobalResourcePool::with_value(GlobalResourceKey::MonsterCuisine, 3.0);
        let room = pool.to_room_state();
        assert_eq!(room.get(&GlobalResourceKey::MonsterCuisine), Some(&3.0));
    }
}
