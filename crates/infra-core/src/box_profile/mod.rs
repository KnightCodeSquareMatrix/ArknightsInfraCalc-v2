mod build;
mod eval;
mod narrative;
mod probe;

pub use build::{
    baseline_path_or_default, build_box_profile, build_box_profile_from_current_probe,
    build_box_profile_from_metrics, ActionKind, BoxProfile, BoxProfileMetrics, BoxProfileOptions,
    ComboSnapshot, DomainMetric, GapSeverity, OperboxSummary, ProfileAction, RotationSnapshot,
};
pub use eval::{
    default_schedule_export_path, manu_report_from_assignment, reference_shift_assignment,
    run_schedule_eval_probe, trade_report_from_assignment,
};
pub use narrative::render_box_profile_narrative;
pub use probe::{
    run_layout_probe, run_user_rotation_probe, run_user_rotation_probe_with_profile,
    run_user_rotation_probe_with_profile_and_preferences,
    run_user_rotation_probe_with_request_and_preferences, LayoutProbe,
};
