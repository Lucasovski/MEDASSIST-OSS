use thiserror::Error;

/// Domain-level errors. These describe violated clinical/data invariants and
/// are independent of any transport or storage layer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("validation failed: {field}: {reason}")]
    Validation { field: String, reason: String },

    #[error("invalid date range: start {start} must not be after end {end}")]
    InvalidDateRange { start: String, end: String },

    #[error("note conflict: expected version {expected}, found {found}")]
    VersionConflict { expected: u32, found: u32 },

    #[error("illegal state transition for note: {from} -> {to}")]
    IllegalTransition { from: String, to: String },

    #[error("immutable field: finalised notes cannot be edited; create an amendment instead")]
    ImmutableField,

    #[error("provenance required: content of kind {0} must reference its source")]
    ProvenanceRequired(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("duplicate patient candidate requires manual review: {0}")]
    DuplicateCandidate(String),
}

impl DomainError {
    pub fn validation(field: impl Into<String>, reason: impl Into<String>) -> Self {
        DomainError::Validation {
            field: field.into(),
            reason: reason.into(),
        }
    }
}
