//! Concrete application state: wires infrastructure adapters to application
//! services exactly once, at startup. Handlers only ever see this typed state.

use std::sync::Arc;

use medassist_application::{
    ai::LanguageModelProvider,
    audit::{AuditLister, AuditService},
    auth::AuthService,
    documents::DocumentService,
    encounters::EncounterService,
    notes::NoteService,
    patients::PatientService,
    ports::{AuditSink, BlobStore},
    templates::TemplateService,
    transcription::TranscriptionService,
};
use medassist_infrastructure::{
    ai_double::{DoubleEngine, DoubleLlm},
    ai_ollama::OllamaLlm,
    blob_fs::FsBlobStore,
    pg_repos::{
        PgAudit, PgCodes, PgConsents, PgDocuments, PgEncounters, PgJobs, PgNotes, PgPatients,
        PgSessions, PgTemplates, PgTranscriptions, PgUsers,
    },
};

/// Which documentation engine is active (resolved from env at startup).
pub enum EngineChoice {
    Ollama(Arc<OllamaLlm>),
    /// Clearly-labelled test double — NOT a real AI engine.
    Double(Arc<DoubleEngine>),
}

pub struct Inner {
    pub patients: PatientService<PgPatients, Arc<dyn AuditSink>>,
    pub encounters: EncounterService<PgEncounters, PgPatients, Arc<dyn AuditSink>>,
    pub notes: NoteService<PgNotes, PgEncounters, Arc<dyn AuditSink>>,
    pub documents: DocumentService<PgDocuments, Arc<dyn BlobStore>, PgPatients, Arc<dyn AuditSink>>,
    pub transcriptions: TranscriptionService<PgTranscriptions, PgConsents, Arc<dyn AuditSink>>,
    pub templates: TemplateService<PgTemplates>,
    pub audit: AuditService<AuditReader>,
    pub auth: AuthService<PgUsers, PgSessions, PgAudit>,
    pub jobs: PgJobs,
    pub codes: PgCodes,
    pub blobs: Arc<dyn BlobStore>,
    pub sink: Arc<dyn AuditSink>,
    pub engine: EngineChoice,
    pub llm_id: String,
    pub llm_external: bool,
}

/// Reader adapter implementing the application's `AuditLister` port on top of
/// the Postgres audit table.
pub struct AuditReader(pub PgAudit);

#[async_trait::async_trait]
impl AuditLister for AuditReader {
    async fn list(
        &self,
        org_id: uuid::Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<medassist_application::ports::AuditEvent>, i64), medassist_application::AppError>
    {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT time, actor_user_id, action, resource_type, resource_id, outcome, metadata,
                    COUNT(*) OVER() AS total
             FROM audit_events WHERE org_id=$1
             ORDER BY time DESC LIMIT $2 OFFSET $3",
        )
        .bind(org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.0.pool())
        .await
        .map_err(medassist_application::AppError::storage)?;

        let mut out = Vec::with_capacity(rows.len());
        let mut total = 0i64;
        for r in rows {
            use chrono::{DateTime, Utc};
            total = r.get::<i64, _>("total");
            let outcome = match r.get::<String, _>("outcome").as_str() {
                "success" => medassist_application::ports::AuditOutcome::Success,
                "failure" => medassist_application::ports::AuditOutcome::Failure,
                _ => medassist_application::ports::AuditOutcome::Denied,
            };
            out.push(medassist_application::ports::AuditEvent {
                time: r.get::<DateTime<Utc>, _>("time"),
                actor_user_id: r.get::<Option<uuid::Uuid>, _>("actor_user_id"),
                org_id: Some(org_id),
                action: r.get::<String, _>("action"),
                resource_type: r.get::<String, _>("resource_type"),
                resource_id: r.get::<Option<uuid::Uuid>, _>("resource_id"),
                outcome,
                metadata: r.get::<serde_json::Value, _>("metadata"),
            });
        }
        Ok((out, total))
    }

}

pub type AppState = Arc<Inner>;

/// Build the wired application state. The DB pool must already be migrated.
pub fn build_state(
    pool: sqlx::PgPool,
    blob_root: &std::path::Path,
    ollama_base: Option<String>,
    ollama_model: Option<String>,
) -> AppState {
    let audit = PgAudit::new(pool.clone());
    let patients_repo = PgPatients(pool.clone());
    let encounters_repo = PgEncounters(pool.clone());
    let docs_repo = PgDocuments(pool.clone());
    let blobs = Arc::new(FsBlobStore::new(blob_root));
    let sink: Arc<dyn AuditSink> = Arc::new(audit.clone());

    let (engine, llm_id, llm_external) = match (&ollama_base, &ollama_model) {
        (Some(base), Some(model)) => {
            let llm = Arc::new(OllamaLlm::new(base.clone(), model.clone()));
            (
                EngineChoice::Ollama(llm.clone()),
                llm.id(),
                medassist_application::ai::LanguageModelProvider::is_external(&*llm),
            )
        }
        _ => {
            tracing::warn!(
                "MEDASSIST_OLLAMA_BASE_URL/MODEL not set: using TEST DOUBLE engine. \
                 It is NOT a real AI model and is intended for development/testing only."
            );
            let d = Arc::new(DoubleEngine(DoubleLlm::new()));
            (
                EngineChoice::Double(d.clone()),
                medassist_application::ai::ClinicalDocumentationEngine::id(&*d),
                false,
            )
        }
    };

    Arc::new(Inner {
        patients: PatientService::new(patients_repo.clone(), sink.clone()),
        encounters: EncounterService::new(encounters_repo.clone(), patients_repo.clone(), sink.clone()),
        notes: NoteService::new(PgNotes(pool.clone()), encounters_repo, sink.clone()),
        documents: DocumentService::new(docs_repo, blobs.clone() as Arc<dyn BlobStore>, patients_repo, sink.clone()),
        transcriptions: TranscriptionService::new(
            PgTranscriptions(pool.clone()),
            PgConsents(pool.clone()),
            sink.clone(),
        ),
        templates: TemplateService::new(PgTemplates(pool.clone())),
        audit: AuditService::new(AuditReader(PgAudit::new(pool.clone()))),
        auth: AuthService::new(
            PgUsers(pool.clone()),
            PgSessions(pool.clone()),
            PgAudit::new(pool.clone()),
        ),
        jobs: PgJobs(pool.clone()),
        codes: PgCodes(pool),
        blobs,
        sink,
        engine,
        llm_id,
        llm_external,
    })
}
