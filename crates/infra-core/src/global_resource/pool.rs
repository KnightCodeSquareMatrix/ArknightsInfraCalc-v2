use std::collections::{HashMap, HashSet};
use std::ops::{Index, IndexMut};

use super::key::GlobalResourceKey;
use super::registry::CONVERSIONS;

/// 贸易/制造房间的可变数值快照；未提供的资源从正零开始。
/// 不表达键存在性，不改变全局资源池的 HashMap 接口。
#[derive(Debug, Clone, Default)]
pub struct RoomState([f64; GlobalResourceKey::ALL.len()]);

impl Index<GlobalResourceKey> for RoomState {
    type Output = f64;

    fn index(&self, key: GlobalResourceKey) -> &Self::Output {
        &self.0[key.room_state_index()]
    }
}

impl IndexMut<GlobalResourceKey> for RoomState {
    fn index_mut(&mut self, key: GlobalResourceKey) -> &mut Self::Output {
        &mut self.0[key.room_state_index()]
    }
}

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

    /// 贸易/制造专用固定快照：按键直接复制，保留数值，不做归并或浮点运算。
    pub fn to_fixed_room_state(&self) -> RoomState {
        let mut room = RoomState::default();
        for (&key, &value) in &self.values {
            room[key] = value;
        }
        room
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
    pub fn run_conversions(&mut self, active_buffs: &HashSet<(String, String)>) {
        loop {
            let mut changed = false;
            for conv in CONVERSIONS {
                let provider_active = active_buffs
                    .iter()
                    .any(|(_, buff)| buff == conv.provider_buff_id);
                let converter_active = active_buffs
                    .iter()
                    .any(|(_, buff)| buff == conv.converter_buff_id);
                if !provider_active || !converter_active {
                    continue;
                }
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
