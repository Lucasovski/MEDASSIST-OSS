//! Audit query use-case (read side). Writes happen through the `AuditSink`
//! port in every service; this exposes authorised listing for compliance UIs.

use uuid::Uuid;

use crate::errors::{AppError, Actor};
use crate::ports::AuditEvent;

/// In-memory + repository-backed audit reader is provided by infrastructure;
/// the application defines the access policy: only admins may list events,
/// and only within their own organisation.
pub struct AuditService<S: AuditLister> {
    pub lister: S,
}

#[async_trait::async_trait]
pub trait AuditLister: Send + Sync {
    async fn list(&self, org_id: Uuid, limit: i64, offset: i64) -> Result<(Vec<AuditEvent>, i64), AppError>;
}

impl<S: AuditLister> AuditService<S> {
    pub fn new(lister: S) -> Self {
        Self { lister }
    }

    pub async fn list(&self, actor: &Actor, limit: i64, offset: i64) -> Result<(Vec<AuditEvent>, i64), AppError> {
        if !actor.is_admin() {
            return Err(AppError::Forbidden("audit events are visible to administrators only"));
        }
        self.lister.list(actor.org_id, limit, offset).await
    }
}
