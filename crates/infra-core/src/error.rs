//! Shared failure boundary for public `infra-core` operations.
//!
//! The enum provides transparent I/O, JSON, and CSV source variants for uncontextualized
//! conversions. Callers may instead wrap failures with reader-facing context in [`Error::Msg`]
//! through [`Error::msg`]. The crate does not currently expose stable machine-readable domain
//! error codes.

use thiserror::Error;

/// Result type returned by fallible `infra-core` operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors surfaced by data loading, parsing, and domain validation.
#[derive(Debug, Error)]
pub enum Error {
    /// Domain, unsupported-state, or contextualized failure with reader-facing context.
    #[error("{0}")]
    Msg(String),
    /// Filesystem or stream failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON decoding or encoding failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// CSV decoding or encoding failure.
    #[error(transparent)]
    Csv(#[from] csv::Error),
    /// A rotation report was constructed, but a later validation/finalization step failed.
    /// The report is retained for diagnostic consumers such as `plan.compute`.
    #[error("{message}")]
    PartialRotation {
        report: Box<crate::schedule::TeamRotationReport>,
        message: String,
    },
    /// A complete ABC candidate failed its Control required/combination mood budget.
    #[error("{message}")]
    ControlRotationMoodRejected { operator: String, message: String },
    /// All bounded full-roster rebuilds for this ABC candidate failed the same gate.
    #[error("{message}")]
    ControlRotationMoodRetriesExhausted { message: String },
    /// A recommended Plan role made its required trade-room fill infeasible.
    #[error("{message}")]
    OptionalTradeRoleInfeasible {
        system_id: String,
        role_id: String,
        operator: String,
        message: String,
    },
    /// The selected gold-flow core cannot be completed as a whole candidate.
    #[error("{message}")]
    GoldFlowCoreInfeasible { message: String },
    /// The selected Plan's required control anchors cannot fit the facility.
    #[error("control_rotation_admission: required anchors exceed blueprint capacity")]
    ControlRotationRequiredAnchorsExceedCapacity { required: usize, capacity: usize },
    /// An optional timed-rotation preclaim could not complete its facility fill.
    #[error("{message}")]
    OptionalRotationPlacementInfeasible { message: String },
    /// A manufacture room with hard anchors could not be completed and scored.
    #[error("manufacture {room_id} produced no anchored hit with {anchor_count} anchors")]
    ManufactureAnchoredCompletionInfeasible {
        room_id: String,
        anchor_count: usize,
    },
}

impl Error {
    /// Construct a domain, unsupported-state, or contextualized error.
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }

    /// Retain a report when a downstream rotation stage fails after construction.
    pub fn with_rotation(
        report: &crate::schedule::TeamRotationReport,
        error: impl std::fmt::Display,
    ) -> Self {
        Self::PartialRotation {
            report: Box::new(report.clone()),
            message: error.to_string(),
        }
    }

    pub fn partial_rotation_report(&self) -> Option<crate::schedule::TeamRotationReport> {
        match self {
            Self::PartialRotation { report, .. } => Some((**report).clone()),
            _ => None,
        }
    }
}
