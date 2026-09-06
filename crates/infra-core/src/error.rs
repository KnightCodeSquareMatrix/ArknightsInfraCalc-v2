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
