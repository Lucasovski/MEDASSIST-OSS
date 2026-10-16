use medassist_domain::DomainError;
use thiserror::Error;

/// Application-level error taxonomy. The API layer maps these to HTTP codes;
/// nothing here knows about HTTP.
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("unauthenticated")]
    Unauthenticated,

    #[error("not authorised to access this resource")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(&'static str),

    #[error("not found: {0}")]
    NotFound(&'static str),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("unprocessable: {0}")]
    Unprocessable(String),

    #[error("external service error: {0}")]
    ExternalService(String),

    #[error("timeout")]
    Timeout,

    #[error("cancelled")]
    Cancelled,

    #[error("internal error")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl AppError {
    pub fn internal(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        AppError::Internal(Box::new(e))
    }

    /// Wrap an opaque storage/infrastructure error without leaking its type.
    pub fn storage(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        tracing::error!(error = %e, "storage error");
        AppError::Internal(Box::new(StorageError))
    }

    /// Stable machine-readable code for API error envelopes.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Domain(DomainError::Validation { .. }) => "validation_error",
            AppError::Domain(DomainError::InvalidDateRange { .. }) => "invalid_date_range",
            AppError::Domain(DomainError::VersionConflict { .. }) => "version_conflict",
            AppError::Domain(DomainError::IllegalTransition { .. }) => "illegal_transition",
            AppError::Domain(DomainError::ImmutableField) => "immutable_field",
            AppError::Domain(DomainError::ProvenanceRequired(_)) => "provenance_required",
            AppError::Domain(DomainError::NotFound(_)) => "not_found",
            AppError::Domain(DomainError::DuplicateCandidate(_)) => "duplicate_candidate",
            AppError::Unauthenticated => "unauthenticated",
            AppError::Unauthorized => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::Unprocessable(_) => "unprocessable",
            AppError::ExternalService(_) => "external_service_error",
            AppError::Timeout => "timeout",
            AppError::Cancelled => "cancelled",
            AppError::Internal(_) => "internal_error",
        }
    }
}

/// Opaque marker so infrastructure failures never leak driver details to the
/// API surface (the real cause is logged, not returned).
#[derive(Debug)]
pub struct StorageError;
impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "storage failure")
    }
}
impl std::error::Error for StorageError {}

/// Machine-facing context propagated from transport into use-cases.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: uuid::Uuid,
    pub org_id: uuid::Uuid,
    pub roles: Vec<String>,
}

impl Actor {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }
}
