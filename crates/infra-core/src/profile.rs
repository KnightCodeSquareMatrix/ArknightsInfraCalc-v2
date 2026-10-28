use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

thread_local! {
    // No atomics, allocation, or request trace borrow at actual solver/check entry.
    static HOT_PATH_BATCH: Cell<Option<HotPathSnapshot>> = const { Cell::new(None) };
}

#[inline]
pub fn record_shortcut_json_load() {
    HOT_PATH_BATCH.with(|batch| {
        if let Some(mut counts) = batch.get() {
            counts.shortcut_json_loads += 1;
            batch.set(Some(counts));
        } else {
            COUNTERS.shortcut_json_loads.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[inline]
pub fn record_exclusive_check() {
    HOT_PATH_BATCH.with(|batch| {
        if let Some(mut counts) = batch.get() {
            counts.exclusive_checks += 1;
            batch.set(Some(counts));
        } else {
            COUNTERS.exclusive_checks.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[inline]
pub fn record_trade_solve() {
    HOT_PATH_BATCH.with(|batch| {
        if let Some(mut counts) = batch.get() {
            counts.trade_solves += 1;
            batch.set(Some(counts));
        } else {
            COUNTERS.trade_solves.fetch_add(1, Ordering::Relaxed);
        }
    });
}

impl HotPathCounters {
    fn merge(&self, counts: HotPathSnapshot) {
        for (counter, value) in [
            (&self.shortcut_json_loads, counts.shortcut_json_loads),
            (&self.exclusive_checks, counts.exclusive_checks),
            (&self.trade_solves, counts.trade_solves),
        ] {
            if value != 0 {
                counter.fetch_add(value, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self) -> HotPathSnapshot {
        HotPathSnapshot {
            shortcut_json_loads: self.shortcut_json_loads.load(Ordering::Relaxed),
            exclusive_checks: self.exclusive_checks.load(Ordering::Relaxed),
            trade_solves: self.trade_solves.load(Ordering::Relaxed),
        }
    }
}

/// A Rayon task (or serial search) owns plain TLS counts until its guard drops.
/// Nested batches restore the outer counts without counting inner entries twice.
/// The guard must drop on the thread where it installed TLS.
pub struct HotPathBatchGuard {
    totals: Arc<HotPathCounters>,
    previous: Option<HotPathSnapshot>,
    _thread_bound: PhantomData<Rc<()>>,
}

pub fn start_hot_path_batch(totals: Arc<HotPathCounters>) -> HotPathBatchGuard {
    let previous = HOT_PATH_BATCH.with(|batch| batch.replace(Some(HotPathSnapshot::default())));
    HotPathBatchGuard {
        totals,
        previous,
        _thread_bound: PhantomData,
    }
}

impl Drop for HotPathBatchGuard {
    fn drop(&mut self) {
        let counts = HOT_PATH_BATCH
            .with(|batch| batch.replace(self.previous.take()))
            .unwrap_or_default();
        self.totals.merge(counts);
        COUNTERS.merge(counts);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OperationMetrics {
    pub calls: u64,
    pub wall_nanos: u64,
    pub workforce_nanos: u64,
    pub layout_nanos: u64,
    pub mechanics_nanos: u64,
    pub cross_facility_nanos: u64,
    pub support_nanos: u64,
    pub support_registry_load_nanos: u64,
    pub room_materialize_nanos: u64,
    pub support_registry_loads: u64,
    pub layout_clones: u64,
    pub blueprint_clones: u64,
    pub assignment_clones: u64,
    pub room_context_clones: u64,
    pub support_registry_parses: u64,
    pub prepare_nanos: u64,
    pub memo_key_prepare_nanos: u64,
    pub bake_lookup_nanos: u64,
    pub evaluate_nanos: u64,
    pub sort_topk_nanos: u64,
    pub generated_combinations: u64,
    pub candidate_attempts: u64,
    pub accepted_hits: u64,
    pub selected_assignments: u64,
    pub bake_hits: u64,
    pub memo_hits: u64,
    pub memo_misses: u64,
    pub live_calls: u64,
    pub actual_trade_solves: u64,
    pub actual_exclusive_checks: u64,
    pub actual_shortcut_json_loads: u64,
}

impl OperationMetrics {
    fn accumulate(&mut self, other: Self) {
        self.calls = self.calls.saturating_add(other.calls);
        self.wall_nanos = self.wall_nanos.saturating_add(other.wall_nanos);
        self.workforce_nanos = self.workforce_nanos.saturating_add(other.workforce_nanos);
        self.layout_nanos = self.layout_nanos.saturating_add(other.layout_nanos);
        self.mechanics_nanos = self.mechanics_nanos.saturating_add(other.mechanics_nanos);
        self.cross_facility_nanos = self
            .cross_facility_nanos
            .saturating_add(other.cross_facility_nanos);
        self.support_nanos = self.support_nanos.saturating_add(other.support_nanos);
        self.support_registry_load_nanos = self
            .support_registry_load_nanos
            .saturating_add(other.support_registry_load_nanos);
        self.room_materialize_nanos = self
            .room_materialize_nanos
            .saturating_add(other.room_materialize_nanos);
        self.support_registry_loads = self
            .support_registry_loads
            .saturating_add(other.support_registry_loads);
        self.layout_clones = self.layout_clones.saturating_add(other.layout_clones);
        self.blueprint_clones = self.blueprint_clones.saturating_add(other.blueprint_clones);
        self.assignment_clones = self
            .assignment_clones
            .saturating_add(other.assignment_clones);
        self.room_context_clones = self
            .room_context_clones
            .saturating_add(other.room_context_clones);
        self.support_registry_parses = self
            .support_registry_parses
            .saturating_add(other.support_registry_parses);
        self.prepare_nanos = self.prepare_nanos.saturating_add(other.prepare_nanos);
        self.memo_key_prepare_nanos = self
            .memo_key_prepare_nanos
            .saturating_add(other.memo_key_prepare_nanos);
        self.bake_lookup_nanos = self
            .bake_lookup_nanos
            .saturating_add(other.bake_lookup_nanos);
        self.evaluate_nanos = self.evaluate_nanos.saturating_add(other.evaluate_nanos);
        self.sort_topk_nanos = self.sort_topk_nanos.saturating_add(other.sort_topk_nanos);
        self.generated_combinations = self
            .generated_combinations
            .saturating_add(other.generated_combinations);
        self.candidate_attempts = self
            .candidate_attempts
            .saturating_add(other.candidate_attempts);
        self.accepted_hits = self.accepted_hits.saturating_add(other.accepted_hits);
        self.selected_assignments = self
            .selected_assignments
            .saturating_add(other.selected_assignments);
        self.bake_hits = self.bake_hits.saturating_add(other.bake_hits);
        self.memo_hits = self.memo_hits.saturating_add(other.memo_hits);
        self.memo_misses = self.memo_misses.saturating_add(other.memo_misses);
        self.live_calls = self.live_calls.saturating_add(other.live_calls);
        self.actual_trade_solves = self
            .actual_trade_solves
            .saturating_add(other.actual_trade_solves);
        self.actual_exclusive_checks = self
            .actual_exclusive_checks
            .saturating_add(other.actual_exclusive_checks);
        self.actual_shortcut_json_loads = self
            .actual_shortcut_json_loads
            .saturating_add(other.actual_shortcut_json_loads);
    }

    fn delta(self, before: Self) -> Self {
        Self {
            calls: self.calls.saturating_sub(before.calls),
            wall_nanos: self.wall_nanos.saturating_sub(before.wall_nanos),
            workforce_nanos: self.workforce_nanos.saturating_sub(before.workforce_nanos),
            layout_nanos: self.layout_nanos.saturating_sub(before.layout_nanos),
            mechanics_nanos: self.mechanics_nanos.saturating_sub(before.mechanics_nanos),
            cross_facility_nanos: self
                .cross_facility_nanos
                .saturating_sub(before.cross_facility_nanos),
            support_nanos: self.support_nanos.saturating_sub(before.support_nanos),
            support_registry_load_nanos: self
                .support_registry_load_nanos
                .saturating_sub(before.support_registry_load_nanos),
            room_materialize_nanos: self
                .room_materialize_nanos
                .saturating_sub(before.room_materialize_nanos),
            support_registry_loads: self
                .support_registry_loads
                .saturating_sub(before.support_registry_loads),
            layout_clones: self.layout_clones.saturating_sub(before.layout_clones),
            blueprint_clones: self
                .blueprint_clones
                .saturating_sub(before.blueprint_clones),
            assignment_clones: self
                .assignment_clones
                .saturating_sub(before.assignment_clones),
            room_context_clones: self
                .room_context_clones
                .saturating_sub(before.room_context_clones),
            support_registry_parses: self
                .support_registry_parses
                .saturating_sub(before.support_registry_parses),
            prepare_nanos: self.prepare_nanos.saturating_sub(before.prepare_nanos),
            memo_key_prepare_nanos: self
                .memo_key_prepare_nanos
                .saturating_sub(before.memo_key_prepare_nanos),
            bake_lookup_nanos: self
                .bake_lookup_nanos
                .saturating_sub(before.bake_lookup_nanos),
            evaluate_nanos: self.evaluate_nanos.saturating_sub(before.evaluate_nanos),
            sort_topk_nanos: self.sort_topk_nanos.saturating_sub(before.sort_topk_nanos),
            generated_combinations: self
                .generated_combinations
                .saturating_sub(before.generated_combinations),
            candidate_attempts: self
                .candidate_attempts
                .saturating_sub(before.candidate_attempts),
            accepted_hits: self.accepted_hits.saturating_sub(before.accepted_hits),
            selected_assignments: self
                .selected_assignments
                .saturating_sub(before.selected_assignments),
            bake_hits: self.bake_hits.saturating_sub(before.bake_hits),
            memo_hits: self.memo_hits.saturating_sub(before.memo_hits),
            memo_misses: self.memo_misses.saturating_sub(before.memo_misses),
            live_calls: self.live_calls.saturating_sub(before.live_calls),
            actual_trade_solves: self
                .actual_trade_solves
                .saturating_sub(before.actual_trade_solves),
            actual_exclusive_checks: self
                .actual_exclusive_checks
                .saturating_sub(before.actual_exclusive_checks),
            actual_shortcut_json_loads: self
                .actual_shortcut_json_loads
                .saturating_sub(before.actual_shortcut_json_loads),
        }
    }
}

/// 固定诊断类别；热路径只传枚举并累计整数，标签映射在输出时执行。
#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub(crate) enum BakeMissKind {
    ForbiddenPairs,
    Override,
    Predicate,
    InvalidMood,
    InvalidShift,
    Layout,
    MoodSensitive,
    PeerReset,
    Generating,
    Unavailable,
    SkillTable,
    DuplicateNames,
    StateAbsent,
    FacilityStateAbsent,
    NonCanonicalOrder,
    TradeAtoms,
    ManufactureAtoms,
    SignatureAbsent,
    Capacity,
    HeapUnavailable,
    TradeIncomplete,
    ManufactureIncomplete,
    NoEligibleTrade,
}

impl BakeMissKind {
    const ALL: [Self; 23] = [
        Self::ForbiddenPairs,
        Self::Override,
        Self::Predicate,
        Self::InvalidMood,
        Self::InvalidShift,
        Self::Layout,
        Self::MoodSensitive,
        Self::PeerReset,
        Self::Generating,
        Self::Unavailable,
        Self::SkillTable,
        Self::DuplicateNames,
        Self::StateAbsent,
        Self::FacilityStateAbsent,
        Self::NonCanonicalOrder,
        Self::TradeAtoms,
        Self::ManufactureAtoms,
        Self::SignatureAbsent,
        Self::Capacity,
        Self::HeapUnavailable,
        Self::TradeIncomplete,
        Self::ManufactureIncomplete,
        Self::NoEligibleTrade,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ForbiddenPairs => "forbidden_pairs",
            Self::Override => "override",
            Self::Predicate => "predicate",
            Self::InvalidMood => "invalid_mood",
            Self::InvalidShift => "invalid_shift",
            Self::Layout => "layout",
            Self::MoodSensitive => "mood_sensitive",
            Self::PeerReset => "peer_reset",
            Self::Generating => "generating",
            Self::Unavailable => "unavailable",
            Self::SkillTable => "skill_table",
            Self::DuplicateNames => "duplicate_names",
            Self::StateAbsent => "state_absent",
            Self::FacilityStateAbsent => "facility_state_absent",
            Self::NonCanonicalOrder => "noncanonical_order",
            Self::TradeAtoms => "trade_atoms",
            Self::ManufactureAtoms => "manufacture_atoms",
            Self::SignatureAbsent => "signature_absent",
            Self::Capacity => "capacity",
            Self::HeapUnavailable => "heap_unavailable",
            Self::TradeIncomplete => "trade_incomplete",
            Self::ManufactureIncomplete => "manufacture_incomplete",
            Self::NoEligibleTrade => "no_eligible_trade",
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub(crate) enum BakeColdPhase {
    Init,
    Generate,
    Validate,
    Verify,
    Index,
    Publish,
}

impl BakeColdPhase {
    const ALL: [Self; 6] = [
        Self::Init,
        Self::Generate,
        Self::Validate,
        Self::Verify,
        Self::Index,
        Self::Publish,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Generate => "generate",
            Self::Validate => "validate",
            Self::Verify => "verify",
            Self::Index => "index",
            Self::Publish => "publish",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BakeColdMetrics {
    pub calls: u64,
    pub errors: u64,
    pub nanos: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BakeMetrics {
    pub trade_misses: [u64; 23],
    pub manufacture_misses: [u64; 23],
    pub trade_errors: u64,
    pub manufacture_errors: u64,
    pub cold: [BakeColdMetrics; 6],
}

impl BakeMetrics {
    fn accumulate(&mut self, other: Self) {
        for (a, b) in self.trade_misses.iter_mut().zip(other.trade_misses) {
            *a = a.saturating_add(b);
        }
        for (a, b) in self
            .manufacture_misses
            .iter_mut()
            .zip(other.manufacture_misses)
        {
            *a = a.saturating_add(b);
        }
        self.trade_errors = self.trade_errors.saturating_add(other.trade_errors);
        self.manufacture_errors = self
            .manufacture_errors
            .saturating_add(other.manufacture_errors);
        for (a, b) in self.cold.iter_mut().zip(other.cold) {
            a.calls = a.calls.saturating_add(b.calls);
            a.errors = a.errors.saturating_add(b.errors);
            a.nanos = a.nanos.saturating_add(b.nanos);
        }
    }
    fn delta(self, before: Self) -> Self {
        let mut result = Self::default();
        for i in 0..23 {
            result.trade_misses[i] = self.trade_misses[i].saturating_sub(before.trade_misses[i]);
            result.manufacture_misses[i] =
                self.manufacture_misses[i].saturating_sub(before.manufacture_misses[i]);
        }
        result.trade_errors = self.trade_errors.saturating_sub(before.trade_errors);
        result.manufacture_errors = self
            .manufacture_errors
            .saturating_sub(before.manufacture_errors);
        for i in 0..6 {
            result.cold[i] = BakeColdMetrics {
                calls: self.cold[i].calls.saturating_sub(before.cold[i].calls),
                errors: self.cold[i].errors.saturating_sub(before.cold[i].errors),
                nanos: self.cold[i].nanos.saturating_sub(before.cold[i].nanos),
            };
        }
        result
    }
}

pub(crate) fn record_bake_query(
    operation: SearchOperation,
    miss: Option<BakeMissKind>,
    error: bool,
) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        let (misses, errors) = match operation {
            SearchOperation::Trade => (
                &mut state.metrics.bake.trade_misses,
                &mut state.metrics.bake.trade_errors,
            ),
            SearchOperation::Manufacture => (
                &mut state.metrics.bake.manufacture_misses,
                &mut state.metrics.bake.manufacture_errors,
            ),
            _ => return,
        };
        if let Some(kind) = miss {
            misses[kind as usize] = misses[kind as usize].saturating_add(1);
        }
        if error && miss.is_none() {
            *errors = errors.saturating_add(1);
        }
    });
}

pub(crate) struct BakeColdGuard {
    phase: BakeColdPhase,
    started: Option<Instant>,
    succeeded: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

pub(crate) fn start_bake_cold(phase: BakeColdPhase) -> BakeColdGuard {
    let active = PERF_TRACE.with(|trace| trace.borrow().is_some());
    BakeColdGuard {
        phase,
        started: active.then(Instant::now),
        succeeded: false,
        _thread_bound: PhantomData,
    }
}

impl BakeColdGuard {
    pub(crate) fn finish<T>(&mut self, result: &crate::error::Result<T>) {
        self.succeeded = result.is_ok();
    }
}

impl Drop for BakeColdGuard {
    fn drop(&mut self) {
        let Some(started) = self.started else {
            return;
        };
        let nanos = duration_nanos(started.elapsed());
        PERF_TRACE.with(|trace| {
            if let Some(state) = trace.borrow_mut().as_mut() {
                let metrics = &mut state.metrics.bake.cold[self.phase as usize];
                metrics.calls = metrics.calls.saturating_add(1);
                metrics.errors = metrics.errors.saturating_add(u64::from(!self.succeeded));
                metrics.nanos = metrics.nanos.saturating_add(nanos);
            }
        });
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SolverPerfSnapshot {
    pub resolve: OperationMetrics,
    pub control: OperationMetrics,
    pub trade: OperationMetrics,
    pub manufacture: OperationMetrics,
    pub power: OperationMetrics,
    pub bake: BakeMetrics,
}

impl SolverPerfSnapshot {
    fn accumulate(&mut self, other: Self) {
        self.resolve.accumulate(other.resolve);
        self.control.accumulate(other.control);
        self.trade.accumulate(other.trade);
        self.manufacture.accumulate(other.manufacture);
        self.power.accumulate(other.power);
        self.bake.accumulate(other.bake);
    }

    pub fn delta(self, before: Self) -> Self {
        Self {
            resolve: self.resolve.delta(before.resolve),
            control: self.control.delta(before.control),
            trade: self.trade.delta(before.trade),
            manufacture: self.manufacture.delta(before.manufacture),
            power: self.power.delta(before.power),
            bake: self.bake.delta(before.bake),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SearchOperation {
    Control,
    Trade,
    Manufacture,
    Power,
}

#[derive(Debug, Clone, Copy)]
pub enum ResolvePhase {
    Workforce,
    Layout,
    Mechanics,
    CrossFacility,
    Support,
    RoomMaterialize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchPhaseDurations {
    pub prepare: Duration,
    pub bake_lookup: Duration,
    pub evaluate: Duration,
    pub sort_topk: Duration,
}

// IDs are stable within diagnostic schema v2; the final slot safely combines unknown labels.
const STAGE_LABELS: [(&str, &str); 61] = [
    ("search", "control"),
    ("search", "trade"),
    ("search", "manufacture"),
    ("search", "power"),
    ("shift", "plan_build"),
    ("shift", "pipeline"),
    ("shift_pipeline", "plan_execute"),
    ("shift_pipeline", "pool_build"),
    ("shift_pipeline", "peak_prepare_control"),
    ("shift_pipeline", "peak_prefixes"),
    ("shift_pipeline", "final_snapshot_refresh"),
    ("shift_pipeline", "meeting_fill"),
    ("shift_pipeline", "stable_sort"),
    ("shift_pipeline", "total"),
    ("shift_pipeline_resolve", "plan_execute"),
    ("shift_pipeline_resolve", "pool_build"),
    ("shift_pipeline_resolve", "peak_prepare_control"),
    ("shift_pipeline_resolve", "peak_prefixes"),
    ("shift_pipeline_resolve", "final_snapshot_refresh"),
    ("shift_pipeline_resolve", "meeting_fill"),
    ("shift_pipeline_resolve", "stable_sort"),
    ("rotation_finalize", "dorm_prepare"),
    ("rotation_finalize", "rescore"),
    ("rotation_finalize", "dorm_assign_and_totals"),
    ("rotation_finalize", "invariants"),
    ("rotation_finalize", "total"),
    ("rotation_resolve", "total"),
    ("rotation", "total"),
    ("rotation_abc", "shift_1"),
    ("rotation_abc", "shift_2"),
    ("rotation_abc", "shift_3"),
    ("rotation_abc", "peak"),
    ("rotation_abc", "shared_pool_split"),
    ("rotation_abc", "gamma"),
    ("rotation_abc", "shifts_and_postprocess"),
    ("rotation_abc", "total"),
    ("plan_cli", "resource_load"),
    ("plan_cli", "compute_plan"),
    ("plan_cli", "prefix_shadow"),
    ("plan_cli", "output_serialize_write"),
    ("plan_compute", "total"),
    ("plan_compute", "current_probe"),
    ("plan_compute", "explicit_invariants"),
    ("plan_compute", "profile_baseline"),
    ("plan_compute", "maa_build"),
    ("plan_compute_resolve", "current_probe"),
    ("serve_request", "request_parse"),
    ("serve_request", "request_total"),
    ("rotation_abc_candidate", "N"),
    ("rotation_abc_candidate", "A"),
    ("rotation_abc_candidate", "B"),
    ("rotation_abc_control_rebuild", "attempt_0"),
    ("rotation_abc_control_rebuild", "attempt_1"),
    ("rotation_abc_control_rebuild", "attempt_2"),
    ("rotation_abc_control_rebuild", "attempt_3"),
    ("rotation_abc", "gamma_h1"),
    ("rotation_abc", "gamma_h2"),
    ("rotation_abc", "abyssal_backup"),
    ("rotation_abc", "fiammetta_backup"),
    ("rotation_abc", "originium_pair"),
    ("rotation_backup", "disjoint"),
];
const OVERFLOW_STAGE_ID: usize = STAGE_LABELS.len();
const STAGE_SLOT_COUNT: usize = STAGE_LABELS.len() + 1;

fn stage_id(scope: &str, stage: &str) -> usize {
    match (scope, stage) {
        ("search", "control") => 0,
        ("search", "trade") => 1,
        ("search", "manufacture") => 2,
        ("search", "power") => 3,
        ("shift", "plan_build") => 4,
        ("shift", "pipeline") => 5,
        ("shift_pipeline", "plan_execute") => 6,
        ("shift_pipeline", "pool_build") => 7,
        ("shift_pipeline", "peak_prepare_control") => 8,
        ("shift_pipeline", "peak_prefixes") => 9,
        ("shift_pipeline", "final_snapshot_refresh") => 10,
        ("shift_pipeline", "meeting_fill") => 11,
        ("shift_pipeline", "stable_sort") => 12,
        ("shift_pipeline", "total") => 13,
        ("shift_pipeline_resolve", "plan_execute") => 14,
        ("shift_pipeline_resolve", "pool_build") => 15,
        ("shift_pipeline_resolve", "peak_prepare_control") => 16,
        ("shift_pipeline_resolve", "peak_prefixes") => 17,
        ("shift_pipeline_resolve", "final_snapshot_refresh") => 18,
        ("shift_pipeline_resolve", "meeting_fill") => 19,
        ("shift_pipeline_resolve", "stable_sort") => 20,
        ("rotation_finalize", "dorm_prepare") => 21,
        ("rotation_finalize", "rescore") => 22,
        ("rotation_finalize", "dorm_assign_and_totals") => 23,
        ("rotation_finalize", "invariants") => 24,
        ("rotation_finalize", "total") => 25,
        ("rotation_resolve", "total") => 26,
        ("rotation", "total") => 27,
        ("rotation_abc", "shift_1") => 28,
        ("rotation_abc", "shift_2") => 29,
        ("rotation_abc", "shift_3") => 30,
        ("rotation_abc", "peak") => 31,
        ("rotation_abc", "shared_pool_split") => 32,
        ("rotation_abc", "gamma") => 33,
        ("rotation_abc", "shifts_and_postprocess") => 34,
        ("rotation_abc", "total") => 35,
        ("plan_cli", "resource_load") => 36,
        ("plan_cli", "compute_plan") => 37,
        ("plan_cli", "prefix_shadow") => 38,
        ("plan_cli", "output_serialize_write") => 39,
        ("plan_compute", "total") => 40,
        ("plan_compute", "current_probe") => 41,
        ("plan_compute", "explicit_invariants") => 42,
        ("plan_compute", "profile_baseline") => 43,
        ("plan_compute", "maa_build") => 44,
        ("plan_compute_resolve", "current_probe") => 45,
        ("serve_request", "request_parse") => 46,
        ("serve_request", "request_total") => 47,
        ("rotation_abc_candidate", "N") => 48,
        ("rotation_abc_candidate", "A") => 49,
        ("rotation_abc_candidate", "B") => 50,
        ("rotation_abc_control_rebuild", "attempt_0") => 51,
        ("rotation_abc_control_rebuild", "attempt_1") => 52,
        ("rotation_abc_control_rebuild", "attempt_2") => 53,
        ("rotation_abc_control_rebuild", "attempt_3") => 54,
        ("rotation_abc", "gamma_h1") => 55,
        ("rotation_abc", "gamma_h2") => 56,
        ("rotation_abc", "abyssal_backup") => 57,
        ("rotation_abc", "fiammetta_backup") => 58,
        ("rotation_abc", "originium_pair") => 59,
        ("rotation_backup", "disjoint") => 60,
        _ => OVERFLOW_STAGE_ID,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct StageTotals {
    calls: u64,
    errors: u64,
    elapsed_nanos: u64,
    metrics: SolverPerfSnapshot,
}

#[derive(Debug, Clone, Copy)]
struct RawStageEvent {
    stage_id: usize,
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed_nanos: u64,
    metrics: SolverPerfSnapshot,
}

#[derive(Debug)]
struct PerfTraceState {
    trace_id: u64,
    layout: String,
    operbox: String,
    started: Instant,
    metrics: SolverPerfSnapshot,
    resolve_depth: u32,
    stages: Box<[StageTotals; STAGE_SLOT_COUNT]>,
    details: Option<Vec<RawStageEvent>>,
}

impl PerfTraceState {
    fn record_stage(&mut self, event: RawStageEvent) {
        let totals = &mut self.stages[event.stage_id];
        totals.calls = totals.calls.saturating_add(1);
        totals.errors = totals
            .errors
            .saturating_add(u64::from(event.status == "error"));
        totals.elapsed_nanos = totals.elapsed_nanos.saturating_add(event.elapsed_nanos);
        totals.metrics.accumulate(event.metrics);
        if let Some(details) = self.details.as_mut() {
            details.push(event);
        }
    }

    fn flush(self) {
        // Stop the request/run clock before any diagnostic allocation, formatting or I/O.
        let elapsed_nanos = duration_nanos(self.started.elapsed());
        let stages: Vec<_> = self
            .stages
            .iter()
            .enumerate()
            .filter(|(_, totals)| totals.calls != 0)
            .map(|(id, totals)| {
                let (scope, stage) = STAGE_LABELS
                    .get(id)
                    .copied()
                    .unwrap_or(("overflow", "unknown"));
                serde_json::json!({
                    "stage_id": id,
                    "scope": scope,
                    "stage": stage,
                    "calls": totals.calls,
                    "errors": totals.errors,
                    "elapsed_ms": nanos_ms(totals.elapsed_nanos),
                    "metrics": snapshot_json(totals.metrics),
                })
            })
            .collect();
        let mut summary = serde_json::json!({
            "schema_version": 2,
            "event": "solver_summary",
            "trace_id": self.trace_id,
            "layout": self.layout,
            "operbox": self.operbox,
            "elapsed_ms": nanos_ms(elapsed_nanos),
            "elapsed_boundary": "trace_start_to_before_perf_format",
            "timing": "inclusive_non_additive",
            "metrics": snapshot_json(self.metrics),
            "stages": stages,
        });
        if let Some(details) = self.details {
            summary["details"] = serde_json::Value::Array(
                details
                    .into_iter()
                    .map(|event| {
                        serde_json::json!({
                            "stage_id": event.stage_id,
                            "scope": event.scope,
                            "stage": event.stage,
                            "status": event.status,
                            "elapsed_nanos": event.elapsed_nanos,
                            "elapsed_ms": nanos_ms(event.elapsed_nanos),
                            "metrics": snapshot_json(event.metrics),
                        })
                    })
                    .collect(),
            );
        }
        // Ignore stderr failure: diagnostics must not turn a completed solve into a failure.
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "[perf] {summary}");
    }
}

thread_local! {
    static PERF_TRACE: RefCell<Option<PerfTraceState>> = const { RefCell::new(None) };
}

static NEXT_TRACE_ID: AtomicU64 = AtomicU64::new(1);
static SOLVER_PERF_ENABLED: AtomicBool = AtomicBool::new(true);
static SOLVER_PERF_DETAIL_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_solver_perf_detail_enabled(enabled: bool) {
    SOLVER_PERF_DETAIL_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_solver_perf_enabled(enabled: bool) {
    SOLVER_PERF_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Restores the previous request-local trace even when the measured operation returns early.
pub struct SolverPerfTraceGuard {
    previous: Option<PerfTraceState>,
    owns_trace: bool,
}

impl Drop for SolverPerfTraceGuard {
    fn drop(&mut self) {
        if self.owns_trace {
            // Release the RefCell borrow before flushing or dropping the completed state.
            let completed = PERF_TRACE
                .with(|trace| std::mem::replace(&mut *trace.borrow_mut(), self.previous.take()));
            if let Some(state) = completed {
                state.flush();
            }
        }
    }
}

pub fn start_solver_perf_trace(layout: &str, operbox: &str) -> SolverPerfTraceGuard {
    if !SOLVER_PERF_ENABLED.load(Ordering::Relaxed) {
        return SolverPerfTraceGuard {
            previous: None,
            owns_trace: false,
        };
    }
    let state = PerfTraceState {
        trace_id: NEXT_TRACE_ID.fetch_add(1, Ordering::Relaxed),
        layout: layout.to_string(),
        operbox: operbox.to_string(),
        started: Instant::now(),
        metrics: SolverPerfSnapshot::default(),
        resolve_depth: 0,
        stages: Box::new([StageTotals::default(); STAGE_SLOT_COUNT]),
        details: SOLVER_PERF_DETAIL_ENABLED
            .load(Ordering::Relaxed)
            .then(Vec::new),
    };
    let previous = PERF_TRACE.with(|trace| trace.borrow_mut().replace(state));
    SolverPerfTraceGuard {
        previous,
        owns_trace: true,
    }
}

pub fn ensure_solver_perf_trace(layout: &str, operbox: &str) -> SolverPerfTraceGuard {
    if PERF_TRACE.with(|trace| trace.borrow().is_some()) {
        update_solver_perf_trace_labels(layout, operbox);
        SolverPerfTraceGuard {
            previous: None,
            owns_trace: false,
        }
    } else {
        start_solver_perf_trace(layout, operbox)
    }
}

pub fn update_solver_perf_trace_labels(layout: &str, operbox: &str) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        state.layout = layout.to_string();
        state.operbox = operbox.to_string();
    });
}

pub fn solver_perf_snapshot() -> SolverPerfSnapshot {
    PERF_TRACE.with(|trace| {
        trace
            .borrow()
            .as_ref()
            .map(|state| state.metrics)
            .unwrap_or_default()
    })
}

pub struct ResolvePerfGuard {
    active: bool,
}

pub fn begin_resolve_perf() -> ResolvePerfGuard {
    let active = PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return false;
        };
        state.resolve_depth = state.resolve_depth.saturating_add(1);
        true
    });
    ResolvePerfGuard { active }
}

impl Drop for ResolvePerfGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        PERF_TRACE.with(|trace| {
            if let Some(state) = trace.borrow_mut().as_mut() {
                state.resolve_depth = state.resolve_depth.saturating_sub(1);
            }
        });
    }
}

pub fn record_resolve(elapsed: Duration) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        state.metrics.resolve.calls += 1;
        state.metrics.resolve.wall_nanos = state
            .metrics
            .resolve
            .wall_nanos
            .saturating_add(duration_nanos(elapsed));
    });
}

pub fn record_resolve_phase(phase: ResolvePhase, elapsed: Duration) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        let target = match phase {
            ResolvePhase::Workforce => &mut state.metrics.resolve.workforce_nanos,
            ResolvePhase::Layout => &mut state.metrics.resolve.layout_nanos,
            ResolvePhase::Mechanics => &mut state.metrics.resolve.mechanics_nanos,
            ResolvePhase::CrossFacility => &mut state.metrics.resolve.cross_facility_nanos,
            ResolvePhase::Support => &mut state.metrics.resolve.support_nanos,
            ResolvePhase::RoomMaterialize => &mut state.metrics.resolve.room_materialize_nanos,
        };
        *target = target.saturating_add(duration_nanos(elapsed));
    });
}

pub fn record_resolve_construction(
    support_registry_loads: u64,
    layout_clones: u64,
    blueprint_clones: u64,
    assignment_clones: u64,
    room_context_clones: u64,
) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        let metrics = &mut state.metrics.resolve;
        metrics.support_registry_loads = metrics
            .support_registry_loads
            .saturating_add(support_registry_loads);
        metrics.layout_clones = metrics.layout_clones.saturating_add(layout_clones);
        metrics.blueprint_clones = metrics.blueprint_clones.saturating_add(blueprint_clones);
        metrics.assignment_clones = metrics.assignment_clones.saturating_add(assignment_clones);
        metrics.room_context_clones = metrics
            .room_context_clones
            .saturating_add(room_context_clones);
    });
}

pub fn record_support_registry_access() {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        if state.resolve_depth == 0 {
            return;
        }
        state.metrics.resolve.support_registry_loads = state
            .metrics
            .resolve
            .support_registry_loads
            .saturating_add(1);
    });
}

pub fn record_support_registry_parse(elapsed: Duration) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        if state.resolve_depth == 0 {
            return;
        }
        state.metrics.resolve.support_registry_parses = state
            .metrics
            .resolve
            .support_registry_parses
            .saturating_add(1);
        state.metrics.resolve.support_registry_load_nanos = state
            .metrics
            .resolve
            .support_registry_load_nanos
            .saturating_add(duration_nanos(elapsed));
    });
}

pub fn record_search(
    operation: SearchOperation,
    elapsed: Duration,
    generated_combinations: u64,
    candidate_attempts: u64,
    accepted_hits: u64,
    selected_assignments: u64,
    live_search: bool,
    baked: bool,
    phases: SearchPhaseDurations,
) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        let metrics = match operation {
            SearchOperation::Control => &mut state.metrics.control,
            SearchOperation::Trade => &mut state.metrics.trade,
            SearchOperation::Manufacture => &mut state.metrics.manufacture,
            SearchOperation::Power => &mut state.metrics.power,
        };
        metrics.calls += 1;
        metrics.wall_nanos = metrics.wall_nanos.saturating_add(duration_nanos(elapsed));
        metrics.prepare_nanos = metrics
            .prepare_nanos
            .saturating_add(duration_nanos(phases.prepare));
        metrics.bake_lookup_nanos = metrics
            .bake_lookup_nanos
            .saturating_add(duration_nanos(phases.bake_lookup));
        metrics.evaluate_nanos = metrics
            .evaluate_nanos
            .saturating_add(duration_nanos(phases.evaluate));
        metrics.sort_topk_nanos = metrics
            .sort_topk_nanos
            .saturating_add(duration_nanos(phases.sort_topk));
        metrics.generated_combinations = metrics
            .generated_combinations
            .saturating_add(generated_combinations);
        metrics.candidate_attempts = metrics
            .candidate_attempts
            .saturating_add(candidate_attempts);
        metrics.accepted_hits = metrics.accepted_hits.saturating_add(accepted_hits);
        metrics.selected_assignments = metrics
            .selected_assignments
            .saturating_add(selected_assignments);
        if live_search {
            metrics.live_calls += 1;
        } else if baked {
            metrics.bake_hits += 1;
        }
    });
}

/// Captures leaf-search failures before `?` returns from a solver path.
pub struct SearchPerfGuard {
    operation: SearchOperation,
    started: std::time::Instant,
    before: SolverPerfSnapshot,
    generated_combinations: u64,
    candidate_attempts: u64,
    accepted_hits: u64,
    selected_assignments: u64,
    baked: bool,
    phases: SearchPhaseDurations,
    memo_key_prepare: Duration,
    memo_hit: Option<bool>,
    live_started: bool,
    finished: bool,
    hot_path_totals: Option<Arc<HotPathCounters>>,
    hot_path_batch: Option<HotPathBatchGuard>,
}

pub fn start_search_perf(operation: SearchOperation) -> SearchPerfGuard {
    let hot_path_totals = matches!(
        operation,
        SearchOperation::Trade | SearchOperation::Manufacture
    )
    .then(|| Arc::new(HotPathCounters::default()));
    let hot_path_batch = hot_path_totals
        .as_ref()
        .map(|totals| start_hot_path_batch(Arc::clone(totals)));
    SearchPerfGuard {
        operation,
        started: std::time::Instant::now(),
        before: solver_perf_snapshot(),
        generated_combinations: 0,
        candidate_attempts: 0,
        accepted_hits: 0,
        selected_assignments: 0,
        baked: false,
        phases: SearchPhaseDurations::default(),
        memo_key_prepare: Duration::ZERO,
        memo_hit: None,
        live_started: false,
        finished: false,
        hot_path_totals,
        hot_path_batch,
    }
}

impl SearchPerfGuard {
    /// 叶入口只记录一次数字；None 表示查询不具备缓存资格或键编码失败。
    pub fn set_memo_lookup(&mut self, key_prepare: Duration, hit: Option<bool>) {
        self.memo_key_prepare = key_prepare;
        self.memo_hit = hit;
    }

    /// Share numeric counts explicitly with Rayon tasks; request TLS is not inherited.
    pub fn hot_path_batch_counters(&self) -> Arc<HotPathCounters> {
        Arc::clone(
            self.hot_path_totals
                .as_ref()
                .expect("trade/manufacture search batch"),
        )
    }

    pub fn mark_live_started(&mut self) {
        self.live_started = true;
    }

    pub fn set_progress(
        &mut self,
        generated_combinations: u64,
        candidate_attempts: u64,
        accepted_hits: u64,
        phases: SearchPhaseDurations,
    ) {
        self.generated_combinations = generated_combinations;
        self.candidate_attempts = candidate_attempts;
        self.accepted_hits = accepted_hits;
        self.phases = phases;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        generated_combinations: u64,
        candidate_attempts: u64,
        accepted_hits: u64,
        selected_assignments: u64,
        baked: bool,
        phases: SearchPhaseDurations,
    ) {
        self.generated_combinations = generated_combinations;
        self.candidate_attempts = candidate_attempts;
        self.accepted_hits = accepted_hits;
        self.selected_assignments = selected_assignments;
        self.baked = baked;
        self.phases = phases;
        self.record();
        emit_solver_stage_metrics(
            "search",
            search_operation_name(self.operation),
            "ok",
            self.started.elapsed(),
            self.before,
            solver_perf_snapshot(),
        );
        self.finished = true;
    }

    fn record(&mut self) {
        // Rayon collection has dropped every task guard. Flush serial entries before
        // sampling totals, including preparation/Bake failures and full materialization.
        drop(self.hot_path_batch.take());
        PERF_TRACE.with(|trace| {
            if let Some(state) = trace.borrow_mut().as_mut() {
                let metrics = match self.operation {
                    SearchOperation::Trade => &mut state.metrics.trade,
                    SearchOperation::Manufacture => &mut state.metrics.manufacture,
                    SearchOperation::Control => &mut state.metrics.control,
                    SearchOperation::Power => &mut state.metrics.power,
                };
                metrics.memo_key_prepare_nanos = metrics
                    .memo_key_prepare_nanos
                    .saturating_add(duration_nanos(self.memo_key_prepare));
                match self.memo_hit {
                    Some(true) => metrics.memo_hits = metrics.memo_hits.saturating_add(1),
                    Some(false) => metrics.memo_misses = metrics.memo_misses.saturating_add(1),
                    None => {}
                }
            }
        });
        if let Some(totals) = self.hot_path_totals.as_ref() {
            let counts = totals.snapshot();
            PERF_TRACE.with(|trace| {
                if let Some(state) = trace.borrow_mut().as_mut() {
                    let metrics = match self.operation {
                        SearchOperation::Trade => &mut state.metrics.trade,
                        SearchOperation::Manufacture => &mut state.metrics.manufacture,
                        SearchOperation::Control => &mut state.metrics.control,
                        SearchOperation::Power => &mut state.metrics.power,
                    };
                    metrics.actual_trade_solves = metrics
                        .actual_trade_solves
                        .saturating_add(counts.trade_solves);
                    metrics.actual_exclusive_checks = metrics
                        .actual_exclusive_checks
                        .saturating_add(counts.exclusive_checks);
                    metrics.actual_shortcut_json_loads = metrics
                        .actual_shortcut_json_loads
                        .saturating_add(counts.shortcut_json_loads);
                }
            });
        }
        record_search(
            self.operation,
            self.started.elapsed(),
            self.generated_combinations,
            self.candidate_attempts,
            self.accepted_hits,
            self.selected_assignments,
            self.live_started && !self.baked,
            self.baked,
            self.phases,
        );
    }
}

impl Drop for SearchPerfGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.record();
        emit_solver_stage_metrics(
            "search",
            search_operation_name(self.operation),
            "error",
            self.started.elapsed(),
            self.before,
            solver_perf_snapshot(),
        );
    }
}

/// Main-thread inclusive stage timing and operation delta. Mark successful exit
/// explicitly; unwinding and early error returns still record the entered stage.
pub struct SolverStageGuard {
    scope: &'static str,
    stage: &'static str,
    started: Instant,
    before: SolverPerfSnapshot,
    status: &'static str,
}

pub fn start_solver_stage(scope: &'static str, stage: &'static str) -> SolverStageGuard {
    SolverStageGuard {
        scope,
        stage,
        started: Instant::now(),
        before: solver_perf_snapshot(),
        status: "error",
    }
}

impl SolverStageGuard {
    pub fn finish(&mut self) {
        self.status = "ok";
    }

    pub fn record_result<T, E>(&mut self, result: &Result<T, E>) {
        if result.is_ok() {
            self.finish();
        }
    }
}

impl Drop for SolverStageGuard {
    fn drop(&mut self) {
        emit_solver_stage_metrics(
            self.scope,
            self.stage,
            self.status,
            self.started.elapsed(),
            self.before,
            solver_perf_snapshot(),
        );
    }
}

pub fn emit_solver_stage(
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed: Duration,
    before: SolverPerfSnapshot,
    after: SolverPerfSnapshot,
) {
    emit_solver_stage_metrics(scope, stage, status, elapsed, before, after);
}

pub fn emit_solver_stage_metrics(
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed: Duration,
    before: SolverPerfSnapshot,
    after: SolverPerfSnapshot,
) {
    record_stage_event(scope, stage, status, elapsed, after.delta(before));
}

pub fn emit_resolve_stage(
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed: Duration,
    before: SolverPerfSnapshot,
    after: SolverPerfSnapshot,
) {
    let resolve = after.delta(before).resolve;
    if resolve.calls == 0 {
        return;
    }
    record_stage_event(
        scope,
        stage,
        status,
        elapsed,
        SolverPerfSnapshot {
            resolve,
            ..SolverPerfSnapshot::default()
        },
    );
}

fn record_stage_event(
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed: Duration,
    metrics: SolverPerfSnapshot,
) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        state.record_stage(RawStageEvent {
            stage_id: stage_id(scope, stage),
            scope,
            stage,
            status,
            elapsed_nanos: duration_nanos(elapsed),
            metrics,
        });
    });
}

fn snapshot_json(metrics: SolverPerfSnapshot) -> serde_json::Value {
    let mut operations = serde_json::Map::new();
    for (name, operation) in [
        ("resolve", metrics.resolve),
        ("control", metrics.control),
        ("trade", metrics.trade),
        ("manufacture", metrics.manufacture),
        ("power", metrics.power),
    ] {
        if operation != OperationMetrics::default() {
            operations.insert(name.to_string(), operation_json(operation));
        }
    }
    if metrics.bake != BakeMetrics::default() {
        operations.insert("bake".to_string(), bake_metrics_json(metrics.bake));
    }
    serde_json::Value::Object(operations)
}

fn bake_metrics_json(metrics: BakeMetrics) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for (label, misses, errors) in [
        ("trade", metrics.trade_misses, metrics.trade_errors),
        (
            "manufacture",
            metrics.manufacture_misses,
            metrics.manufacture_errors,
        ),
    ] {
        let mut values = serde_json::Map::new();
        for kind in BakeMissKind::ALL {
            if misses[kind as usize] != 0 {
                values.insert(kind.label().to_string(), misses[kind as usize].into());
            }
        }
        if errors != 0 {
            values.insert("query_errors".to_string(), errors.into());
        }
        if !values.is_empty() {
            result.insert(label.to_string(), serde_json::Value::Object(values));
        }
    }
    let mut phases = serde_json::Map::new();
    for phase in BakeColdPhase::ALL {
        let value = metrics.cold[phase as usize];
        if value.calls != 0 {
            let mut json = serde_json::json!({"calls": value.calls, "errors": value.errors, "elapsed_ms": nanos_ms(value.nanos)});
            retain_nonzero_metrics(&mut json);
            phases.insert(phase.label().to_string(), json);
        }
    }
    if !phases.is_empty() {
        result.insert("cold".to_string(), serde_json::Value::Object(phases));
    }
    serde_json::Value::Object(result)
}

fn retain_nonzero_metrics(value: &mut serde_json::Value) {
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, value| value.as_f64() != Some(0.0));
    }
}

fn search_operation_name(operation: SearchOperation) -> &'static str {
    match operation {
        SearchOperation::Control => "control",
        SearchOperation::Trade => "trade",
        SearchOperation::Manufacture => "manufacture",
        SearchOperation::Power => "power",
    }
}

fn operation_json(metrics: OperationMetrics) -> serde_json::Value {
    let mut result = serde_json::json!({
        "calls": metrics.calls,
        "wall_ms": nanos_ms(metrics.wall_nanos),
        "workforce_ms": nanos_ms(metrics.workforce_nanos),
        "layout_ms": nanos_ms(metrics.layout_nanos),
        "mechanics_ms": nanos_ms(metrics.mechanics_nanos),
        "cross_facility_ms": nanos_ms(metrics.cross_facility_nanos),
        "support_ms": nanos_ms(metrics.support_nanos),
        "support_registry_load_ms": nanos_ms(metrics.support_registry_load_nanos),
        "room_materialize_ms": nanos_ms(metrics.room_materialize_nanos),
        "support_registry_loads": metrics.support_registry_loads,
        "layout_clones": metrics.layout_clones,
        "blueprint_clones": metrics.blueprint_clones,
        "assignment_clones": metrics.assignment_clones,
        "room_context_clones": metrics.room_context_clones,
        "support_registry_parses": metrics.support_registry_parses,
        "prepare_ms": nanos_ms(metrics.prepare_nanos),
        "memo_key_prepare_ms": nanos_ms(metrics.memo_key_prepare_nanos),
        "bake_lookup_ms": nanos_ms(metrics.bake_lookup_nanos),
        "evaluate_ms": nanos_ms(metrics.evaluate_nanos),
        "sort_topk_ms": nanos_ms(metrics.sort_topk_nanos),
        "generated_combinations": metrics.generated_combinations,
        "candidate_attempts": metrics.candidate_attempts,
        "accepted_hits": metrics.accepted_hits,
        "selected_assignments": metrics.selected_assignments,
        "bake_hits": metrics.bake_hits,
        "memo_hits": metrics.memo_hits,
        "memo_misses": metrics.memo_misses,
        "live_calls": metrics.live_calls,
        "actual_trade_solves": metrics.actual_trade_solves,
        "actual_exclusive_checks": metrics.actual_exclusive_checks,
        "actual_shortcut_json_loads": metrics.actual_shortcut_json_loads,
    });
    retain_nonzero_metrics(&mut result);
    result
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn nanos_ms(nanos: u64) -> f64 {
    (nanos as f64 / 1_000_000.0 * 1000.0).round() / 1000.0
}
