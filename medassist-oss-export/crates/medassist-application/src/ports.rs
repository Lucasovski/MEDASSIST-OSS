//! Ports (traits) implemented by the infrastructure layer. The application
//! depends only on these abstractions — never on SQLx/Axum/provider SDKs.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use medassist_domain::document::ClinicalDocument;
use medassist_domain::encounter::Encounter;
use medassist_domain::job::{AsyncJob, JobKind, JobState};
use medassist_domain::note::Note;
use medassist_domain::patient::Patient;
use medassist_domain::prov::Provenance;
use medassist_domain::transcription::{RecordingConsent, Transcription, TranscriptSegment};

use crate::errors::{AppError, Actor};

#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Default)]
pub struct PatientQuery {
    pub text: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

#[async_trait]
pub trait PatientRepository: Send + Sync {
    async fn insert(&self, p: &Patient) -> Result<(), AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Patient>, AppError>;
    async fn search(&self, actor: &Actor, q: &PatientQuery) -> Result<Page<Patient>, AppError>;
    async fn update(&self, p: &Patient) -> Result<(), AppError>;
    async fn soft_delete(&self, actor: &Actor, id: Uuid, now: DateTime<Utc>) -> Result<bool, AppError>;
    /// Candidates for duplicate review (advisory only — never auto-merged).
    async fn duplicate_candidates(&self, actor: &Actor, name: String, birth: Option<chrono::NaiveDate>, external_keys: Vec<String>) -> Result<Vec<(Patient, f64)>, AppError>;
}

#[async_trait]
pub trait EncounterRepository: Send + Sync {
    async fn insert(&self, e: &Encounter) -> Result<(), AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Encounter>, AppError>;
    async fn list_for_patient(&self, actor: &Actor, patient_id: Uuid, limit: i64, offset: i64) -> Result<Page<Encounter>, AppError>;
    async fn update(&self, e: &Encounter) -> Result<(), AppError>;
}

#[async_trait]
pub trait NoteRepository: Send + Sync {
    async fn insert(&self, n: &Note) -> Result<(), AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Note>, AppError>;
    async fn list_for_encounter(&self, actor: &Actor, encounter_id: Uuid) -> Result<Vec<Note>, AppError>;
    /// Persist an edit *and* its snapshot atomically; version guard enforced
    /// in SQL (`WHERE version = expected`) so races fail with a conflict.
    async fn update_with_version_guard(&self, n: &Note, expected_version: u32, edited_by: Uuid) -> Result<(), AppError>;
    async fn append_version_snapshot(&self, n: &Note, edited_by: Uuid) -> Result<(), AppError>;
}

#[async_trait]
pub trait DocumentRepository: Send + Sync {
    async fn insert(&self, d: &ClinicalDocument) -> Result<(), AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<ClinicalDocument>, AppError>;
    async fn set_state(&self, actor: &Actor, id: Uuid, state: medassist_domain::document::ProcessingState, error: Option<String>) -> Result<(), AppError>;
    async fn list_for_patient(&self, actor: &Actor, patient_id: Uuid, limit: i64, offset: i64) -> Result<Page<ClinicalDocument>, AppError>;
}

/// Blob storage port. Implementations must keep originals immutable.
#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put(&self, org_id: Uuid, doc_id: Uuid, bytes: &[u8]) -> Result<String, AppError>;
    async fn get(&self, key: &str) -> Result<Vec<u8>, AppError>;
    async fn delete(&self, key: &str) -> Result<(), AppError>;
}

/// Blanket forwarding of the audit-sink port through shared references so
/// services can hold `Arc<dyn AuditSink>` without monomorphising per adapter.
#[async_trait]
impl<T: ?Sized + AuditSink + Sync> AuditSink for std::sync::Arc<T> {
    async fn record(&self, event: AuditEvent) {
        (**self).record(event).await
    }
}

#[async_trait]
impl<T: ?Sized + BlobStore + Sync> BlobStore for std::sync::Arc<T> {
    async fn put(&self, org_id: Uuid, doc_id: Uuid, bytes: &[u8]) -> Result<String, AppError> {
        (**self).put(org_id, doc_id, bytes).await
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>, AppError> {
        (**self).get(key).await
    }
    async fn delete(&self, key: &str) -> Result<(), AppError> {
        (**self).delete(key).await
    }
}

#[async_trait]
pub trait ConsentRepository: Send + Sync {
    async fn insert(&self, c: &RecordingConsent) -> Result<(), AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<RecordingConsent>, AppError>;
}

#[async_trait]
pub trait TranscriptionRepository: Send + Sync {
    async fn insert(&self, t: &Transcription) -> Result<(), AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Transcription>, AppError>;
    async fn complete(&self, actor: &Actor, id: Uuid, segments: Vec<TranscriptSegment>, engine: &str, engine_version: Option<String>, now: DateTime<Utc>) -> Result<(), AppError>;
    async fn fail(&self, actor: &Actor, id: Uuid, error: String, now: DateTime<Utc>) -> Result<(), AppError>;
    async fn mark_reviewed(&self, actor: &Actor, id: Uuid, now: DateTime<Utc>) -> Result<(), AppError>;
}

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn insert(&self, j: &AsyncJob) -> Result<(), AppError>;
    async fn claim_next(&self, kind: JobKind) -> Result<Option<AsyncJob>, AppError>;
    async fn transition(&self, id: Uuid, from: JobState, to: JobState, error: Option<String>, result_ref: Option<String>) -> Result<bool, AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<AsyncJob>, AppError>;
}

/// Audit trail port. Implementations store metadata only — never full
/// clinical payloads (see docs/SECURITY).
#[async_trait]
pub trait AuditSink: Send + Sync {
    async fn record(&self, event: AuditEvent);
}

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub time: DateTime<Utc>,
    pub actor_user_id: Option<Uuid>,
    pub org_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub outcome: AuditOutcome,
    /// Non-clinical detail (ids, versions, flags). Never free-text PHI.
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

/// User credentials port used by authentication.
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_email(&self, email: &str) -> Result<Option<UserRecord>, AppError>;
    async fn insert(&self, u: &NewUserRecord) -> Result<Uuid, AppError>;
    async fn update_password_hash(&self, user_id: Uuid, hash: String) -> Result<(), AppError>;
    async fn register_login_success(&self, user_id: Uuid, now: DateTime<Utc>) -> Result<(), AppError>;
    async fn register_login_failure(&self, user_id: Uuid) -> Result<(), AppError>;
    async fn lock_until(&self, user_id: Uuid, until: DateTime<Utc>) -> Result<(), AppError>;
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub roles: Vec<String>,
    pub failed_logins: i32,
    pub locked_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewUserRecord {
    pub org_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub token_hash: String,
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub roles: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert(&self, s: &SessionRecord) -> Result<(), AppError>;
    async fn find_by_token_hash(&self, hash: &str) -> Result<Option<SessionRecord>, AppError>;
    async fn revoke(&self, hash: &str, now: DateTime<Utc>) -> Result<(), AppError>;
    async fn revoke_all_for_user(&self, user_id: Uuid, now: DateTime<Utc>) -> Result<(), AppError>;
}

/// Template registry for note templates (versioned JSON definitions).
#[derive(Debug, Clone)]
#[derive(serde::Serialize)]
pub struct NoteTemplate {
    pub id: Uuid,
    pub org_id: Option<Uuid>, // None => built-in
    pub name: String,
    pub version: u32,
    pub sections: Vec<String>,
    pub required_sections: Vec<String>,
}

#[async_trait]
pub trait TemplateRepository: Send + Sync {
    async fn list(&self, actor: &Actor) -> Result<Vec<NoteTemplate>, AppError>;
}

/// Clinical coding suggestion port (e.g. CID-10 lookup). Suggestions are
/// always unconfirmed until reviewed by a professional.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSuggestion {
    pub system: String,
    pub version: String,
    pub code: String,
    pub display: String,
    pub provenance: Provenance,
    pub confirmed: bool,
}

#[async_trait]
pub trait CodingRepository: Send + Sync {
    async fn search_codes(&self, system: &str, text: &str, limit: i64) -> Result<Vec<CodeSuggestion>, AppError>;
    async fn upsert_code(&self, system: &str, version: &str, code: &str, display: &str) -> Result<(), AppError>;
}
