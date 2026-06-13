use std::sync::atomic::{AtomicU64, Ordering};

/// 轻量运行时计数器，供 `infra-cli profile` 对比优化前后热路径行为。
#[derive(Debug, Default)]
pub struct HotPathCounters {
    pub shortcut_json_loads: AtomicU64,
    pub exclusive_checks: AtomicU64,
    pub trade_solves: AtomicU64,
}

static COUNTERS: HotPathCounters = HotPathCounters {
    shortcut_json_loads: AtomicU64::new(0),
    exclusive_checks: AtomicU64::new(0),
    trade_solves: AtomicU64::new(0),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotPathSnapshot {
    pub shortcut_json_loads: u64,
    pub exclusive_checks: u64,
    pub trade_solves: u64,
}

pub fn reset_hot_path_counters() {
    COUNTERS.shortcut_json_loads.store(0, Ordering::Relaxed);
    COUNTERS.exclusive_checks.store(0, Ordering::Relaxed);
    COUNTERS.trade_solves.store(0, Ordering::Relaxed);
}

pub fn hot_path_snapshot() -> HotPathSnapshot {
    HotPathSnapshot {
        shortcut_json_loads: COUNTERS.shortcut_json_loads.load(Ordering::Relaxed),
        exclusive_checks: COUNTERS.exclusive_checks.load(Ordering::Relaxed),
        trade_solves: COUNTERS.trade_solves.load(Ordering::Relaxed),
    }
}

pub fn record_shortcut_json_load() {
    COUNTERS.shortcut_json_loads.fetch_add(1, Ordering::Relaxed);
}

pub fn record_exclusive_check() {
    COUNTERS.exclusive_checks.fetch_add(1, Ordering::Relaxed);
}

pub fn record_trade_solve() {
    COUNTERS.trade_solves.fetch_add(1, Ordering::Relaxed);
}
