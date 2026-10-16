//! Uniform JSON error envelope. Never leaks internal causes to clients.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use medassist_application::AppError;
use serde_json::json;

/// Wrapper so handlers can return `Result<T, ApiError>` directly.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(AppError::Unprocessable(msg.into()))
    }
    pub fn unauthorized() -> Self {
        Self(AppError::Unauthorized)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self.0 {
            AppError::Domain(de) => (domain_status(de), self.0.code(), de.to_string()),
            AppError::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated", "invalid credentials".into()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthenticated", "authentication required".into()),
            AppError::Forbidden(m) => (StatusCode::FORBIDDEN, "forbidden", m.to_string()),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found", "resource not found".into()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m.clone()),
            AppError::Unprocessable(m) => (StatusCode::UNPROCESSABLE_ENTITY, "unprocessable", m.clone()),
            AppError::ExternalService(_) => (
                StatusCode::BAD_GATEWAY,
                "external_service_error",
                "external AI service error".into(),
            ),
            AppError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "timeout", "operation timed out".into()),
            AppError::Cancelled => (StatusCode::BAD_REQUEST, "cancelled", "operation cancelled".into()),
            AppError::Internal(_) => {
                // Log the fact, never the cause.
                tracing::error!("internal server error (cause hidden)");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "internal error".into())
            }
        };
        // Message sanitisation: domain/app messages are written by us and are
        // safe (no PHI, no driver details). Internal errors are masked above.
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}

fn domain_status(e: &medassist_domain::DomainError) -> StatusCode {
    use medassist_domain::DomainError::*;
    match e {
        Validation { .. } | InvalidDateRange { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        NotFound(_) => StatusCode::NOT_FOUND,
        VersionConflict { .. } | IllegalTransition { .. } | DuplicateCandidate(_) => StatusCode::CONFLICT,
        ImmutableField | ProvenanceRequired(_) => StatusCode::UNPROCESSABLE_ENTITY,
    }
}
