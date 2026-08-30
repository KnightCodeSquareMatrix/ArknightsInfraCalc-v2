use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

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

#[derive(Debug, Clone, Copy, Default)]
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

#[derive(Debug, Clone)]
struct PerfTraceState {
    trace_id: u64,
    layout: String,
    operbox: String,
    metrics: SolverPerfSnapshot,
    resolve_depth: u32,
}

thread_local! {
    static PERF_TRACE: RefCell<Option<PerfTraceState>> = const { RefCell::new(None) };
}

static NEXT_TRACE_ID: AtomicU64 = AtomicU64::new(1);

/// Restores the previous request-local trace even when the measured operation returns early.
pub struct SolverPerfTraceGuard {
    previous: Option<PerfTraceState>,
    owns_trace: bool,
}

impl Drop for SolverPerfTraceGuard {
    fn drop(&mut self) {
        if self.owns_trace {
            PERF_TRACE.with(|trace| {
                *trace.borrow_mut() = self.previous.take();
            });
        }
    }
}

pub fn start_solver_perf_trace(layout: &str, operbox: &str) -> SolverPerfTraceGuard {
    let state = PerfTraceState {
        trace_id: NEXT_TRACE_ID.fetch_add(1, Ordering::Relaxed),
        layout: layout.to_string(),
        operbox: operbox.to_string(),
        metrics: SolverPerfSnapshot::default(),
        resolve_depth: 0,
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

pub fn record_support_registry_load(elapsed: Duration) {
    PERF_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(state) = trace.as_mut() else {
            return;
        };
        if state.resolve_depth == 0 {
            return;
        }
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
    _before: SolverPerfSnapshot,
    _after: SolverPerfSnapshot,
) {
    PERF_TRACE.with(|trace| {
        let trace = trace.borrow();
        let Some(state) = trace.as_ref() else {
            return;
        };
        let event = serde_json::json!({
            "schema_version": 1,
            "event": "solver_stage",
            "trace_id": state.trace_id,
            "layout": state.layout,
            "operbox": state.operbox,
            "scope": scope,
            "stage": stage,
            "status": status,
            "elapsed_ms": duration_ms(elapsed),
        });
        eprintln!("[perf] {event}");
    });
}

pub fn emit_solver_stage_metrics(
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed: Duration,
    before: SolverPerfSnapshot,
    after: SolverPerfSnapshot,
) {
    PERF_TRACE.with(|trace| {
        let trace = trace.borrow();
        let Some(state) = trace.as_ref() else {
            return;
        };
        let metrics = after.delta(before);
        let event = serde_json::json!({
            "schema_version": 1,
            "event": "solver_stage",
            "trace_id": state.trace_id,
            "layout": state.layout,
            "operbox": state.operbox,
            "scope": scope,
            "stage": stage,
            "status": status,
            "elapsed_ms": duration_ms(elapsed),
            "metrics": {
                "resolve": operation_json(metrics.resolve),
                "control": operation_json(metrics.control),
                "trade": operation_json(metrics.trade),
                "manufacture": operation_json(metrics.manufacture),
                "power": operation_json(metrics.power),
            }
        });
        eprintln!("[perf] {event}");
    });
}

pub fn emit_resolve_stage(
    scope: &'static str,
    stage: &'static str,
    status: &'static str,
    elapsed: Duration,
    before: SolverPerfSnapshot,
    after: SolverPerfSnapshot,
) {
    let metrics = after.delta(before).resolve;
    if metrics.calls == 0 {
        return;
    }
    PERF_TRACE.with(|trace| {
        let trace = trace.borrow();
        let Some(state) = trace.as_ref() else {
            return;
        };
        let event = serde_json::json!({
            "schema_version": 1,
            "event": "solver_stage",
            "trace_id": state.trace_id,
            "layout": state.layout,
            "operbox": state.operbox,
            "scope": scope,
            "stage": stage,
            "status": status,
            "elapsed_ms": duration_ms(elapsed),
            "metrics": {"resolve": operation_json(metrics)}
        });
        eprintln!("[perf] {event}");
    });
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

fn duration_ms(duration: Duration) -> f64 {
    nanos_ms(duration_nanos(duration))
}

fn nanos_ms(nanos: u64) -> f64 {
    (nanos as f64 / 1_000_000.0 * 1000.0).round() / 1000.0
}
