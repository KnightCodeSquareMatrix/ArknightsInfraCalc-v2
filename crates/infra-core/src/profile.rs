use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    pub bake_lookup_nanos: u64,
    pub evaluate_nanos: u64,
    pub sort_topk_nanos: u64,
    pub generated_combinations: u64,
    pub candidate_attempts: u64,
    pub accepted_hits: u64,
    pub selected_assignments: u64,
    pub bake_hits: u64,
    pub live_calls: u64,
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
        self.live_calls = self.live_calls.saturating_add(other.live_calls);
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
            live_calls: self.live_calls.saturating_sub(before.live_calls),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SolverPerfSnapshot {
    pub resolve: OperationMetrics,
    pub control: OperationMetrics,
    pub trade: OperationMetrics,
    pub manufacture: OperationMetrics,
    pub power: OperationMetrics,
}

impl SolverPerfSnapshot {
    fn accumulate(&mut self, other: Self) {
        self.resolve.accumulate(other.resolve);
        self.control.accumulate(other.control);
        self.trade.accumulate(other.trade);
        self.manufacture.accumulate(other.manufacture);
        self.power.accumulate(other.power);
    }

    pub fn delta(self, before: Self) -> Self {
        Self {
            resolve: self.resolve.delta(before.resolve),
            control: self.control.delta(before.control),
            trade: self.trade.delta(before.trade),
            manufacture: self.manufacture.delta(before.manufacture),
            power: self.power.delta(before.power),
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
const STAGE_LABELS: [(&str, &str); 48] = [
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
    live_started: bool,
    finished: bool,
}

pub fn start_search_perf(operation: SearchOperation) -> SearchPerfGuard {
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
        live_started: false,
        finished: false,
    }
}

impl SearchPerfGuard {
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

    fn record(&self) {
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
    serde_json::Value::Object(operations)
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
    serde_json::json!({
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
        "bake_lookup_ms": nanos_ms(metrics.bake_lookup_nanos),
        "evaluate_ms": nanos_ms(metrics.evaluate_nanos),
        "sort_topk_ms": nanos_ms(metrics.sort_topk_nanos),
        "generated_combinations": metrics.generated_combinations,
        "candidate_attempts": metrics.candidate_attempts,
        "accepted_hits": metrics.accepted_hits,
        "selected_assignments": metrics.selected_assignments,
        "bake_hits": metrics.bake_hits,
        "live_calls": metrics.live_calls,
    })
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn nanos_ms(nanos: u64) -> f64 {
    (nanos as f64 / 1_000_000.0 * 1000.0).round() / 1000.0
}
