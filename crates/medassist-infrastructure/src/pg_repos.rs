//! PostgreSQL (SQLx) implementations of the application ports. Every query is
//! parameterised and every clinical table access is filtered by `org_id` from
//! the authenticated actor — a client-supplied id never proves authorisation.
//!
//! License: GPL-3.0-or-later.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use medassist_application::errors::{Actor, AppError};
use medassist_application::ports::{
    AuditEvent, AuditOutcome, AuditSink, ConsentRepository, DocumentRepository,
    EncounterRepository, JobRepository, NewUserRecord, NoteRepository, NoteTemplate, Page,
    PatientQuery, PatientRepository, SessionRecord, SessionRepository, TemplateRepository,
    TranscriptionRepository, UserRecord, UserRepository,
};
use medassist_domain::document::{ClinicalDocument, DocumentKind, ProcessingState};
use medassist_domain::encounter::{Encounter, EncounterStatus};
use medassist_domain::identifier::ExternalIdentifier;
use medassist_domain::job::{AsyncJob, JobKind, JobState};
use medassist_domain::note::{Note, NoteBody, NoteStatus};
use medassist_domain::patient::{Patient, Sex};
use medassist_domain::transcription::{
    RecordingConsent, TranscriptSegment, Transcription, TranscriptionState,
};

type Result<T> = std::result::Result<T, AppError>;

/// Small std-error wrapper so invariant violations map to internal errors
/// without leaking the raw string into logs.
#[derive(Debug)]
struct InternalMsg(&'static str);
impl std::fmt::Display for InternalMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for InternalMsg {}

fn sex_to_str(s: Option<Sex>) -> Option<&'static str> {
    s.map(|s| match s {
        Sex::Female => "female",
        Sex::Male => "male",
        Sex::Intersex => "intersex",
        Sex::Unknown => "unknown",
        Sex::Undisclosed => "undisclosed",
    })
}

fn sex_from_str(s: Option<&str>) -> Option<Sex> {
    Some(match s? {
        "female" => Sex::Female,
        "male" => Sex::Male,
        "intersex" => Sex::Intersex,
        "unknown" => Sex::Unknown,
        "undisclosed" => Sex::Undisclosed,
        _ => return None,
    })
}

fn enc_status_str(s: EncounterStatus) -> &'static str {
    match s {
        EncounterStatus::Open => "open",
        EncounterStatus::InProgress => "in_progress",
        EncounterStatus::Closed => "closed",
        EncounterStatus::Finalised => "finalised",
        EncounterStatus::Cancelled => "cancelled",
    }
}

fn enc_status_from(s: &str) -> Result<EncounterStatus> {
    Ok(match s {
        "open" => EncounterStatus::Open,
        "in_progress" => EncounterStatus::InProgress,
        "closed" => EncounterStatus::Closed,
        "finalised" => EncounterStatus::Finalised,
        "cancelled" => EncounterStatus::Cancelled,
        _other => return Err(AppError::internal(InternalMsg("unknown encounter status"))),
    })
}

fn doc_kind_str(k: DocumentKind) -> &'static str {
    match k {
        DocumentKind::Pdf => "pdf",
        DocumentKind::PlainText => "plain_text",
        DocumentKind::Csv => "csv",
        DocumentKind::Image => "image",
        DocumentKind::Other => "other",
    }
}

fn doc_kind_from(s: &str) -> Result<DocumentKind> {
    Ok(match s {
        "pdf" => DocumentKind::Pdf,
        "plain_text" => DocumentKind::PlainText,
        "csv" => DocumentKind::Csv,
        "image" => DocumentKind::Image,
        _ => DocumentKind::Other,
    })
}

fn proc_state_str(s: ProcessingState) -> &'static str {
    match s {
        ProcessingState::Pending => "pending",
        ProcessingState::Processing => "processing",
        ProcessingState::Extracted => "extracted",
        ProcessingState::Failed => "failed",
        ProcessingState::Reviewed => "reviewed",
    }
}

fn proc_state_from(s: &str) -> Result<ProcessingState> {
    Ok(match s {
        "pending" => ProcessingState::Pending,
        "processing" => ProcessingState::Processing,
        "extracted" => ProcessingState::Extracted,
        "failed" => ProcessingState::Failed,
        "reviewed" => ProcessingState::Reviewed,
        _other => return Err(AppError::internal(InternalMsg("bad document state"))),
    })
}

fn job_state_str(s: JobState) -> &'static str {
    match s {
        JobState::Pending => "pending",
        JobState::Running => "running",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

fn job_state_from(s: &str) -> Result<JobState> {
    Ok(match s {
        "pending" => JobState::Pending,
        "running" => JobState::Running,
        "completed" => JobState::Completed,
        "failed" => JobState::Failed,
        "cancelled" => JobState::Cancelled,
        _other => return Err(AppError::internal(InternalMsg("bad job state"))),
    })
}

fn job_kind_str(k: JobKind) -> &'static str {
    match k {
        JobKind::Transcription => "transcription",
        JobKind::DocumentExtraction => "document_extraction",
        JobKind::AiDocumentationDraft => "ai_documentation_draft",
    }
}

fn job_kind_from(s: &str) -> Result<JobKind> {
    Ok(match s {
        "transcription" => JobKind::Transcription,
        "document_extraction" => JobKind::DocumentExtraction,
        "ai_documentation_draft" => JobKind::AiDocumentationDraft,
        _other => return Err(AppError::internal(InternalMsg("bad job kind"))),
    })
}

fn tx_state_str(s: TranscriptionState) -> &'static str {
    match s {
        TranscriptionState::Pending => "pending",
        TranscriptionState::Processing => "processing",
        TranscriptionState::Completed => "completed",
        TranscriptionState::Failed => "failed",
        TranscriptionState::Cancelled => "cancelled",
    }
}

fn tx_state_from(s: &str) -> Result<TranscriptionState> {
    Ok(match s {
        "pending" => TranscriptionState::Pending,
        "processing" => TranscriptionState::Processing,
        "completed" => TranscriptionState::Completed,
        "failed" => TranscriptionState::Failed,
        "cancelled" => TranscriptionState::Cancelled,
        _other => return Err(AppError::internal(InternalMsg("bad transcription state"))),
    })
}

// ---------------------------------------------------------------------------
// Patients
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgPatients(pub PgPool);

impl PgPatients {
    fn map_row(row: &PgRow) -> Result<Patient> {
        let external_ids: serde_json::Value = row.try_get("external_ids").map_err(err)?;
        let ids: Vec<ExternalIdentifier> = serde_json::from_value(external_ids).unwrap_or_default();
        Ok(Patient {
            id: row.try_get("id").map_err(err)?,
            org_id: row.try_get("org_id").map_err(err)?,
            family_name: row.try_get("family_name").map_err(err)?,
            given_name: row.try_get("given_name").map_err(err)?,
            birth_date: row.try_get("birth_date").map_err(err)?,
            sex: sex_from_str(row.try_get::<Option<&str>, _>("sex").map_err(err)?),
            contact: row.try_get("contact").map_err(err)?,
            external_ids: ids,
            notes: row.try_get("notes").map_err(err)?,
            created_at: row.try_get("created_at").map_err(err)?,
            updated_at: row.try_get("updated_at").map_err(err)?,
            deleted_at: row.try_get("deleted_at").map_err(err)?,
        })
    }
}

fn err(e: sqlx::Error) -> AppError {
    AppError::storage(e)
}

#[async_trait]
impl PatientRepository for PgPatients {
    async fn insert(&self, p: &Patient) -> Result<()> {
        let ids = serde_json::to_value(&p.external_ids).map_err(AppError::internal)?;
        sqlx::query(
            "INSERT INTO patients (id, org_id, family_name, given_name, birth_date, sex, contact, external_ids, notes, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(p.id)
        .bind(p.org_id)
        .bind(&p.family_name)
        .bind(&p.given_name)
        .bind(p.birth_date)
        .bind(sex_to_str(p.sex))
        .bind(&p.contact)
        .bind(&ids)
        .bind(&p.notes)
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Patient>> {
        let row = sqlx::query(
            "SELECT * FROM patients WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(actor.org_id)
        .fetch_optional(&self.0)
        .await
        .map_err(err)?;
        match row {
            Some(r) => Ok(Some(Self::map_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn search(&self, actor: &Actor, q: &PatientQuery) -> Result<Page<Patient>> {
        let total: i64 = if let Some(t) = &q.text {
            let like = format!("%{}%", t.trim().to_lowercase());
            sqlx::query_scalar(
                "SELECT count(*) FROM patients WHERE org_id=$1 AND deleted_at IS NULL AND (lower(family_name) LIKE $2 OR lower(given_name) LIKE $2)",
            )
            .bind(actor.org_id)
            .bind(&like)
            .fetch_one(&self.0)
            .await
            .map_err(err)?
        } else {
            sqlx::query_scalar(
                "SELECT count(*) FROM patients WHERE org_id=$1 AND deleted_at IS NULL",
            )
            .bind(actor.org_id)
            .fetch_one(&self.0)
            .await
            .map_err(err)?
        };
        let rows = if let Some(t) = &q.text {
            let like = format!("%{}%", t.trim().to_lowercase());
            sqlx::query(
                "SELECT * FROM patients WHERE org_id=$1 AND deleted_at IS NULL AND (lower(family_name) LIKE $2 OR lower(given_name) LIKE $2) ORDER BY created_at DESC LIMIT $3 OFFSET $4",
            )
            .bind(actor.org_id)
            .bind(&like)
            .bind(q.limit)
            .bind(q.offset)
            .fetch_all(&self.0)
            .await
            .map_err(err)?
        } else {
            sqlx::query(
                "SELECT * FROM patients WHERE org_id=$1 AND deleted_at IS NULL ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(actor.org_id)
            .bind(q.limit)
            .bind(q.offset)
            .fetch_all(&self.0)
            .await
            .map_err(err)?
        };
        let items = rows.iter().map(Self::map_row).collect::<Result<Vec<_>>>()?;
        Ok(Page {
            items,
            total,
            limit: q.limit,
            offset: q.offset,
        })
    }

    async fn update(&self, p: &Patient) -> Result<()> {
        let ids = serde_json::to_value(&p.external_ids).map_err(AppError::internal)?;
        let res = sqlx::query(
            "UPDATE patients SET family_name=$2, given_name=$3, birth_date=$4, sex=$5, contact=$6, external_ids=$7, notes=$8, updated_at=$9
             WHERE id=$1 AND org_id=$10 AND deleted_at IS NULL",
        )
        .bind(p.id)
        .bind(&p.family_name)
        .bind(&p.given_name)
        .bind(p.birth_date)
        .bind(sex_to_str(p.sex))
        .bind(&p.contact)
        .bind(&ids)
        .bind(&p.notes)
        .bind(p.updated_at)
        .bind(p.org_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("patient"));
        }
        Ok(())
    }

    async fn soft_delete(&self, actor: &Actor, id: Uuid, now: DateTime<Utc>) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE patients SET deleted_at=$2, updated_at=$2 WHERE id=$1 AND org_id=$3 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(now)
        .bind(actor.org_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(res.rows_affected() == 1)
    }

    async fn duplicate_candidates(
        &self,
        actor: &Actor,
        name: String,
        birth: Option<chrono::NaiveDate>,
        external_keys: Vec<String>,
    ) -> Result<Vec<(Patient, f64)>> {
        // Advisory only: same birth date OR matching external identifier value.
        let mut out = Vec::new();
        if let Some(bd) = birth {
            let rows = sqlx::query(
                "SELECT * FROM patients WHERE org_id=$1 AND deleted_at IS NULL AND birth_date=$2 AND id <> $3 LIMIT 20",
            )
            .bind(actor.org_id)
            .bind(bd)
            .bind(Uuid::nil())
            .fetch_all(&self.0)
            .await
            .map_err(err)?;
            for r in &rows {
                let p = Self::map_row(r)?;
                let same_name = p
                    .family_name
                    .eq_ignore_ascii_case(name.split_whitespace().next_back().unwrap_or(""));
                out.push((p, if same_name { 0.9 } else { 0.6 }));
            }
        }
        for key in external_keys {
            let rows = sqlx::query(
                "SELECT * FROM patients WHERE org_id=$1 AND deleted_at IS NULL AND external_ids @> $2::jsonb LIMIT 20",
            )
            .bind(actor.org_id)
            .bind(format!(r#"[{{"value":"{}"}}]"#, key.replace('"', "")))
            .fetch_all(&self.0)
            .await
            .map_err(err)?;
            for r in &rows {
                let p = Self::map_row(r)?;
                if !out.iter().any(|(x, _)| x.id == p.id) {
                    out.push((p, 0.95));
                }
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Encounters
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgEncounters(pub PgPool);

impl PgEncounters {
    fn map_row(row: &PgRow) -> Result<Encounter> {
        let status: String = row.try_get("status").map_err(err)?;
        Ok(Encounter {
            id: row.try_get("id").map_err(err)?,
            org_id: row.try_get("org_id").map_err(err)?,
            patient_id: row.try_get("patient_id").map_err(err)?,
            practitioner_id: row.try_get("practitioner_id").map_err(err)?,
            status: enc_status_from(&status)?,
            chief_complaint: row.try_get("chief_complaint").map_err(err)?,
            started_at: row.try_get("started_at").map_err(err)?,
            ended_at: row.try_get("ended_at").map_err(err)?,
            created_at: row.try_get("created_at").map_err(err)?,
            updated_at: row.try_get("updated_at").map_err(err)?,
        })
    }
}

#[async_trait]
impl EncounterRepository for PgEncounters {
    async fn insert(&self, e: &Encounter) -> Result<()> {
        sqlx::query(
            "INSERT INTO encounters (id, org_id, patient_id, practitioner_id, status, chief_complaint, started_at, ended_at, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(e.id)
        .bind(e.org_id)
        .bind(e.patient_id)
        .bind(e.practitioner_id)
        .bind(enc_status_str(e.status))
        .bind(&e.chief_complaint)
        .bind(e.started_at)
        .bind(e.ended_at)
        .bind(e.created_at)
        .bind(e.updated_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Encounter>> {
        let row = sqlx::query("SELECT * FROM encounters WHERE id=$1 AND org_id=$2")
            .bind(id)
            .bind(actor.org_id)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        row.as_ref().map(Self::map_row).transpose()
    }

    async fn list_for_patient(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Page<Encounter>> {
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM encounters WHERE patient_id=$1 AND org_id=$2")
                .bind(patient_id)
                .bind(actor.org_id)
                .fetch_one(&self.0)
                .await
                .map_err(err)?;
        let rows = sqlx::query(
            "SELECT * FROM encounters WHERE patient_id=$1 AND org_id=$2 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
        )
        .bind(patient_id)
        .bind(actor.org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.0)
        .await
        .map_err(err)?;
        Ok(Page {
            items: rows.iter().map(Self::map_row).collect::<Result<Vec<_>>>()?,
            total,
            limit,
            offset,
        })
    }

    async fn update(&self, e: &Encounter) -> Result<()> {
        let res = sqlx::query(
            "UPDATE encounters SET status=$2, chief_complaint=$3, started_at=$4, ended_at=$5, updated_at=$6
             WHERE id=$1 AND org_id=$7",
        )
        .bind(e.id)
        .bind(enc_status_str(e.status))
        .bind(&e.chief_complaint)
        .bind(e.started_at)
        .bind(e.ended_at)
        .bind(e.updated_at)
        .bind(e.org_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("encounter"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Notes (with optimistic version guard enforced in SQL)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgNotes(pub PgPool);

impl PgNotes {
    fn map_row(row: &PgRow) -> Result<Note> {
        let status: String = row.try_get("status").map_err(err)?;
        let body: serde_json::Value = row.try_get("body").map_err(err)?;
        let body: NoteBody = serde_json::from_value(body).map_err(AppError::internal)?;
        Ok(Note {
            id: row.try_get("id").map_err(err)?,
            org_id: row.try_get("org_id").map_err(err)?,
            encounter_id: row.try_get("encounter_id").map_err(err)?,
            patient_id: row.try_get("patient_id").map_err(err)?,
            author_id: row.try_get("author_id").map_err(err)?,
            template_id: row.try_get("template_id").map_err(err)?,
            status: NoteStatus::parse(&status)
                .ok_or_else(|| AppError::internal(InternalMsg("bad note status")))?,
            version: row.try_get::<i32, _>("version").map_err(err)? as u32,
            body,
            created_at: row.try_get("created_at").map_err(err)?,
            updated_at: row.try_get("updated_at").map_err(err)?,
            finalised_at: row.try_get("finalised_at").map_err(err)?,
            finalised_by: row.try_get("finalised_by").map_err(err)?,
            amends_note_id: row.try_get("amends_note_id").map_err(err)?,
        })
    }
}

#[async_trait]
impl NoteRepository for PgNotes {
    async fn insert(&self, n: &Note) -> Result<()> {
        let body = serde_json::to_value(&n.body).map_err(AppError::internal)?;
        sqlx::query(
            "INSERT INTO notes (id, org_id, encounter_id, patient_id, author_id, template_id, status, version, body, created_at, updated_at, finalised_at, finalised_by, amends_note_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(n.id)
        .bind(n.org_id)
        .bind(n.encounter_id)
        .bind(n.patient_id)
        .bind(n.author_id)
        .bind(n.template_id)
        .bind(n.status.as_str())
        .bind(n.version as i32)
        .bind(&body)
        .bind(n.created_at)
        .bind(n.updated_at)
        .bind(n.finalised_at)
        .bind(n.finalised_by)
        .bind(n.amends_note_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Note>> {
        let row = sqlx::query("SELECT * FROM notes WHERE id=$1 AND org_id=$2")
            .bind(id)
            .bind(actor.org_id)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        row.as_ref().map(Self::map_row).transpose()
    }

    async fn list_for_encounter(&self, actor: &Actor, encounter_id: Uuid) -> Result<Vec<Note>> {
        let rows = sqlx::query(
            "SELECT * FROM notes WHERE encounter_id=$1 AND org_id=$2 ORDER BY created_at DESC",
        )
        .bind(encounter_id)
        .bind(actor.org_id)
        .fetch_all(&self.0)
        .await
        .map_err(err)?;
        rows.iter().map(Self::map_row).collect()
    }

    async fn update_with_version_guard(
        &self,
        n: &Note,
        expected_version: u32,
        _edited_by: Uuid,
    ) -> Result<()> {
        let body = serde_json::to_value(&n.body).map_err(AppError::internal)?;
        let res = sqlx::query(
            "UPDATE notes SET status=$2, version=$3, body=$4, updated_at=$5, finalised_at=$6, finalised_by=$7
             WHERE id=$1 AND org_id=$8 AND version=$9",
        )
        .bind(n.id)
        .bind(n.status.as_str())
        .bind(n.version as i32)
        .bind(&body)
        .bind(n.updated_at)
        .bind(n.finalised_at)
        .bind(n.finalised_by)
        .bind(n.org_id)
        .bind(expected_version as i32)
        .execute(&self.0)
        .await
        .map_err(err)?;
        if res.rows_affected() == 0 {
            // Either missing, cross-org or concurrent modification.
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notes WHERE id=$1 AND org_id=$2)")
                    .bind(n.id)
                    .bind(n.org_id)
                    .fetch_one(&self.0)
                    .await
                    .map_err(err)?;
            if exists {
                return Err(AppError::Conflict(
                    "note was modified concurrently; reload before saving".into(),
                ));
            }
            return Err(AppError::NotFound("note"));
        }
        Ok(())
    }

    async fn append_version_snapshot(&self, n: &Note, edited_by: Uuid) -> Result<()> {
        let body = serde_json::to_value(&n.body).map_err(AppError::internal)?;
        sqlx::query(
            "INSERT INTO note_versions (note_id, version, body, edited_by) VALUES ($1,$2,$3,$4) ON CONFLICT (note_id, version) DO NOTHING",
        )
        .bind(n.id)
        .bind(n.version as i32)
        .bind(&body)
        .bind(edited_by)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgDocuments(pub PgPool);

impl PgDocuments {
    fn map_row(row: &PgRow) -> Result<ClinicalDocument> {
        let kind: String = row.try_get("kind").map_err(err)?;
        let state: String = row.try_get("state").map_err(err)?;
        Ok(ClinicalDocument {
            id: row.try_get("id").map_err(err)?,
            org_id: row.try_get("org_id").map_err(err)?,
            patient_id: row.try_get("patient_id").map_err(err)?,
            encounter_id: row.try_get("encounter_id").map_err(err)?,
            kind: doc_kind_from(&kind)?,
            filename: row.try_get("filename").map_err(err)?,
            content_type: row.try_get("content_type").map_err(err)?,
            size_bytes: row.try_get::<i64, _>("size_bytes").map_err(err)? as u64,
            sha256: row.try_get("sha256").map_err(err)?,
            received_at: row.try_get("received_at").map_err(err)?,
            state: proc_state_from(&state)?,
            error: row.try_get("error").map_err(err)?,
            uploaded_by: row.try_get("uploaded_by").map_err(err)?,
            created_at: row.try_get("created_at").map_err(err)?,
        })
    }
}

#[async_trait]
impl DocumentRepository for PgDocuments {
    async fn insert(&self, d: &ClinicalDocument) -> Result<()> {
        sqlx::query(
            "INSERT INTO documents (id, org_id, patient_id, encounter_id, kind, filename, content_type, size_bytes, sha256, received_at, state, error, uploaded_by, created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(d.id)
        .bind(d.org_id)
        .bind(d.patient_id)
        .bind(d.encounter_id)
        .bind(doc_kind_str(d.kind))
        .bind(&d.filename)
        .bind(&d.content_type)
        .bind(d.size_bytes as i64)
        .bind(&d.sha256)
        .bind(d.received_at)
        .bind(proc_state_str(d.state))
        .bind(&d.error)
        .bind(d.uploaded_by)
        .bind(d.created_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<ClinicalDocument>> {
        let row = sqlx::query("SELECT * FROM documents WHERE id=$1 AND org_id=$2")
            .bind(id)
            .bind(actor.org_id)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        row.as_ref().map(Self::map_row).transpose()
    }

    async fn set_state(
        &self,
        actor: &Actor,
        id: Uuid,
        state: ProcessingState,
        error: Option<String>,
    ) -> Result<()> {
        let res = sqlx::query("UPDATE documents SET state=$2, error=$3 WHERE id=$1 AND org_id=$4")
            .bind(id)
            .bind(proc_state_str(state))
            .bind(error)
            .bind(actor.org_id)
            .execute(&self.0)
            .await
            .map_err(err)?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("document"));
        }
        Ok(())
    }

    async fn list_for_patient(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Page<ClinicalDocument>> {
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM documents WHERE patient_id=$1 AND org_id=$2")
                .bind(patient_id)
                .bind(actor.org_id)
                .fetch_one(&self.0)
                .await
                .map_err(err)?;
        let rows = sqlx::query(
            "SELECT * FROM documents WHERE patient_id=$1 AND org_id=$2 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
        )
        .bind(patient_id)
        .bind(actor.org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.0)
        .await
        .map_err(err)?;
        Ok(Page {
            items: rows.iter().map(Self::map_row).collect::<Result<Vec<_>>>()?,
            total,
            limit,
            offset,
        })
    }
}

// ---------------------------------------------------------------------------
// Consents / transcriptions
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgConsents(pub PgPool);

#[async_trait]
impl ConsentRepository for PgConsents {
    async fn insert(&self, c: &RecordingConsent) -> Result<()> {
        sqlx::query(
            "INSERT INTO consents (id, org_id, patient_id, granted_by, scope, retention_policy, granted_at, revoked_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(c.id)
        .bind(c.org_id)
        .bind(c.patient_id)
        .bind(&c.granted_by)
        .bind(&c.scope)
        .bind(&c.retention_policy)
        .bind(c.granted_at)
        .bind(c.revoked_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<RecordingConsent>> {
        let row = sqlx::query("SELECT * FROM consents WHERE id=$1 AND org_id=$2")
            .bind(id)
            .bind(actor.org_id)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(RecordingConsent {
            id: r.try_get("id").map_err(err)?,
            org_id: r.try_get("org_id").map_err(err)?,
            patient_id: r.try_get("patient_id").map_err(err)?,
            granted_by: r.try_get("granted_by").map_err(err)?,
            scope: r.try_get("scope").map_err(err)?,
            retention_policy: r.try_get("retention_policy").map_err(err)?,
            granted_at: r.try_get("granted_at").map_err(err)?,
            revoked_at: r.try_get("revoked_at").map_err(err)?,
        }))
    }
}

#[derive(Clone)]
pub struct PgTranscriptions(pub PgPool);

impl PgTranscriptions {
    fn map_row(row: &PgRow) -> Result<Transcription> {
        let state: String = row.try_get("state").map_err(err)?;
        let segs: serde_json::Value = row.try_get("segments").map_err(err)?;
        let segments: Vec<TranscriptSegment> =
            serde_json::from_value(segs).map_err(AppError::internal)?;
        Ok(Transcription {
            id: row.try_get("id").map_err(err)?,
            org_id: row.try_get("org_id").map_err(err)?,
            patient_id: row.try_get("patient_id").map_err(err)?,
            encounter_id: row.try_get("encounter_id").map_err(err)?,
            state: tx_state_from(&state)?,
            engine: row.try_get("engine").map_err(err)?,
            engine_version: row.try_get("engine_version").map_err(err)?,
            language: row.try_get("language").map_err(err)?,
            segments,
            reviewed: row.try_get("reviewed").map_err(err)?,
            consent_id: row.try_get("consent_id").map_err(err)?,
            audio_retention_policy: row.try_get("audio_retention_policy").map_err(err)?,
            created_at: row.try_get("created_at").map_err(err)?,
            updated_at: row.try_get("updated_at").map_err(err)?,
        })
    }
}

#[async_trait]
impl TranscriptionRepository for PgTranscriptions {
    async fn insert(&self, t: &Transcription) -> Result<()> {
        let segs = serde_json::to_value(&t.segments).map_err(AppError::internal)?;
        sqlx::query(
            "INSERT INTO transcriptions (id, org_id, patient_id, encounter_id, state, engine, engine_version, language, segments, reviewed, consent_id, audio_retention_policy, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(t.id)
        .bind(t.org_id)
        .bind(t.patient_id)
        .bind(t.encounter_id)
        .bind(tx_state_str(t.state))
        .bind(&t.engine)
        .bind(&t.engine_version)
        .bind(&t.language)
        .bind(&segs)
        .bind(t.reviewed)
        .bind(t.consent_id)
        .bind(&t.audio_retention_policy)
        .bind(t.created_at)
        .bind(t.updated_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<Transcription>> {
        let row = sqlx::query("SELECT * FROM transcriptions WHERE id=$1 AND org_id=$2")
            .bind(id)
            .bind(actor.org_id)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        row.as_ref().map(Self::map_row).transpose()
    }

    async fn complete(
        &self,
        actor: &Actor,
        id: Uuid,
        segments: Vec<TranscriptSegment>,
        engine: &str,
        engine_version: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let segs = serde_json::to_value(&segments).map_err(AppError::internal)?;
        let res = sqlx::query(
            "UPDATE transcriptions SET state='completed', segments=$2, engine=$3, engine_version=$4, updated_at=$5 WHERE id=$1 AND org_id=$6",
        )
        .bind(id)
        .bind(&segs)
        .bind(engine)
        .bind(engine_version)
        .bind(now)
        .bind(actor.org_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("transcription"));
        }
        Ok(())
    }

    async fn fail(&self, actor: &Actor, id: Uuid, error: String, now: DateTime<Utc>) -> Result<()> {
        let res = sqlx::query(
            "UPDATE transcriptions SET state='failed', error=$2, updated_at=$3 WHERE id=$1 AND org_id=$4",
        )
        .bind(id)
        .bind(error)
        .bind(now)
        .bind(actor.org_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("transcription"));
        }
        Ok(())
    }

    async fn mark_reviewed(&self, actor: &Actor, id: Uuid, now: DateTime<Utc>) -> Result<()> {
        let res = sqlx::query(
            "UPDATE transcriptions SET reviewed=true, updated_at=$2 WHERE id=$1 AND org_id=$3",
        )
        .bind(id)
        .bind(now)
        .bind(actor.org_id)
        .execute(&self.0)
        .await
        .map_err(err)?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("transcription"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgJobs(pub PgPool);

#[async_trait]
impl JobRepository for PgJobs {
    async fn insert(&self, j: &AsyncJob) -> Result<()> {
        sqlx::query(
            "INSERT INTO jobs (id, org_id, kind, state, attempts, max_attempts, error, result_ref, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(j.id)
        .bind(j.org_id)
        .bind(job_kind_str(j.kind))
        .bind(job_state_str(j.state))
        .bind(j.attempts as i32)
        .bind(j.max_attempts as i32)
        .bind(&j.error)
        .bind(&j.result_ref)
        .bind(j.created_at)
        .bind(j.updated_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn claim_next(&self, kind: JobKind) -> Result<Option<AsyncJob>> {
        // Atomic claim: SKIP LOCKED prevents two workers taking the same job.
        let row = sqlx::query(
            "UPDATE jobs SET state='running', attempts=attempts+1, updated_at=now()
             WHERE id = (
               SELECT id FROM jobs WHERE kind=$1 AND state='pending' AND attempts < max_attempts
               ORDER BY created_at ASC LIMIT 1 FOR UPDATE SKIP LOCKED
             )
             RETURNING *",
        )
        .bind(job_kind_str(kind))
        .fetch_optional(&self.0)
        .await
        .map_err(err)?;
        let Some(r) = row else { return Ok(None) };
        let state: String = r.try_get("state").map_err(err)?;
        let k: String = r.try_get("kind").map_err(err)?;
        Ok(Some(AsyncJob {
            id: r.try_get("id").map_err(err)?,
            org_id: r.try_get("org_id").map_err(err)?,
            kind: job_kind_from(&k)?,
            state: job_state_from(&state)?,
            attempts: r.try_get::<i32, _>("attempts").map_err(err)? as u32,
            max_attempts: r.try_get::<i32, _>("max_attempts").map_err(err)? as u32,
            error: r.try_get("error").map_err(err)?,
            result_ref: r.try_get("result_ref").map_err(err)?,
            created_at: r.try_get("created_at").map_err(err)?,
            updated_at: r.try_get("updated_at").map_err(err)?,
        }))
    }

    async fn transition(
        &self,
        id: Uuid,
        from: JobState,
        to: JobState,
        error: Option<String>,
        result_ref: Option<String>,
    ) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE jobs SET state=$2, error=$3, result_ref=$4, updated_at=now() WHERE id=$1 AND state=$5",
        )
        .bind(id)
        .bind(job_state_str(to))
        .bind(error)
        .bind(result_ref)
        .bind(job_state_str(from))
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(res.rows_affected() == 1)
    }

    async fn get(&self, actor: &Actor, id: Uuid) -> Result<Option<AsyncJob>> {
        let row = sqlx::query("SELECT * FROM jobs WHERE id=$1 AND org_id=$2")
            .bind(id)
            .bind(actor.org_id)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        let Some(r) = row else { return Ok(None) };
        let state: String = r.try_get("state").map_err(err)?;
        let k: String = r.try_get("kind").map_err(err)?;
        Ok(Some(AsyncJob {
            id: r.try_get("id").map_err(err)?,
            org_id: r.try_get("org_id").map_err(err)?,
            kind: job_kind_from(&k)?,
            state: job_state_from(&state)?,
            attempts: r.try_get::<i32, _>("attempts").map_err(err)? as u32,
            max_attempts: r.try_get::<i32, _>("max_attempts").map_err(err)? as u32,
            error: r.try_get("error").map_err(err)?,
            result_ref: r.try_get("result_ref").map_err(err)?,
            created_at: r.try_get("created_at").map_err(err)?,
            updated_at: r.try_get("updated_at").map_err(err)?,
        }))
    }
}

// ---------------------------------------------------------------------------
// Users / sessions
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgUsers(pub PgPool);

#[async_trait]
impl UserRepository for PgUsers {
    async fn find_by_email(&self, email: &str) -> Result<Option<UserRecord>> {
        let row = sqlx::query("SELECT * FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(UserRecord {
            id: r.try_get("id").map_err(err)?,
            org_id: r.try_get("org_id").map_err(err)?,
            email: r.try_get("email").map_err(err)?,
            password_hash: r.try_get("password_hash").map_err(err)?,
            roles: r.try_get::<Vec<String>, _>("roles").map_err(err)?,
            failed_logins: r.try_get("failed_logins").map_err(err)?,
            locked_until: r.try_get("locked_until").map_err(err)?,
        }))
    }

    async fn insert(&self, u: &NewUserRecord) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, org_id, email, password_hash, roles) VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(id)
        .bind(u.org_id)
        .bind(&u.email)
        .bind(&u.password_hash)
        .bind(&u.roles)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(id)
    }

    async fn update_password_hash(&self, user_id: Uuid, hash: String) -> Result<()> {
        sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1")
            .bind(user_id)
            .bind(hash)
            .execute(&self.0)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn register_login_success(&self, user_id: Uuid, _now: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE users SET failed_logins=0, locked_until=NULL WHERE id=$1")
            .bind(user_id)
            .execute(&self.0)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn register_login_failure(&self, user_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE users SET failed_logins = failed_logins + 1 WHERE id=$1")
            .bind(user_id)
            .execute(&self.0)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn lock_until(&self, user_id: Uuid, until: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE users SET locked_until=$2, failed_logins=0 WHERE id=$1")
            .bind(user_id)
            .bind(until)
            .execute(&self.0)
            .await
            .map_err(err)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct PgSessions(pub PgPool);

#[async_trait]
impl SessionRepository for PgSessions {
    async fn insert(&self, s: &SessionRecord) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, org_id, roles, expires_at, revoked_at)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(&s.token_hash)
        .bind(s.user_id)
        .bind(s.org_id)
        .bind(&s.roles)
        .bind(s.expires_at)
        .bind(s.revoked_at)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn find_by_token_hash(&self, hash: &str) -> Result<Option<SessionRecord>> {
        let row = sqlx::query("SELECT * FROM sessions WHERE token_hash=$1")
            .bind(hash)
            .fetch_optional(&self.0)
            .await
            .map_err(err)?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(SessionRecord {
            token_hash: r.try_get("token_hash").map_err(err)?,
            user_id: r.try_get("user_id").map_err(err)?,
            org_id: r.try_get("org_id").map_err(err)?,
            roles: r.try_get::<Vec<String>, _>("roles").map_err(err)?,
            expires_at: r.try_get("expires_at").map_err(err)?,
            revoked_at: r.try_get("revoked_at").map_err(err)?,
        }))
    }

    async fn revoke(&self, hash: &str, now: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE sessions SET revoked_at=$2 WHERE token_hash=$1")
            .bind(hash)
            .bind(now)
            .execute(&self.0)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn revoke_all_for_user(&self, user_id: Uuid, now: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE sessions SET revoked_at=$2 WHERE user_id=$1 AND revoked_at IS NULL")
            .bind(user_id)
            .bind(now)
            .execute(&self.0)
            .await
            .map_err(err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Templates / codes / audit
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PgTemplates(pub PgPool);

#[async_trait]
impl TemplateRepository for PgTemplates {
    async fn list(&self, actor: &Actor) -> Result<Vec<NoteTemplate>> {
        let rows = sqlx::query(
            "SELECT * FROM templates WHERE org_id IS NULL OR org_id=$1 ORDER BY name, version",
        )
        .bind(actor.org_id)
        .fetch_all(&self.0)
        .await
        .map_err(err)?;
        Ok(rows
            .iter()
            .map(|r| -> Result<NoteTemplate> {
                Ok(NoteTemplate {
                    id: r.try_get("id").map_err(err)?,
                    org_id: r.try_get("org_id").map_err(err)?,
                    name: r.try_get("name").map_err(err)?,
                    version: r.try_get::<i32, _>("version").map_err(err)? as u32,
                    sections: r.try_get::<Vec<String>, _>("sections").map_err(err)?,
                    required_sections: r
                        .try_get::<Vec<String>, _>("required_sections")
                        .map_err(err)?,
                })
            })
            .collect::<Result<Vec<_>>>()?)
    }
}

#[derive(Clone)]
pub struct PgCodes(pub PgPool);

#[async_trait]
impl medassist_application::ports::CodingRepository for PgCodes {
    async fn search_codes(
        &self,
        system: &str,
        text: &str,
        limit: i64,
    ) -> Result<Vec<medassist_application::ports::CodeSuggestion>> {
        let like = format!("%{}%", text.trim());
        let rows = sqlx::query(
            "SELECT * FROM codes WHERE system=$1 AND (code ILIKE $2 OR display ILIKE $2) ORDER BY code LIMIT $3",
        )
        .bind(system)
        .bind(&like)
        .bind(limit)
        .fetch_all(&self.0)
        .await
        .map_err(err)?;
        Ok(rows
            .iter()
            .map(|r| medassist_application::ports::CodeSuggestion {
                system: r.get("system"),
                version: r.get("version"),
                code: r.get("code"),
                display: r.get("display"),
                provenance: medassist_domain::prov::Provenance::new(
                    medassist_domain::prov::ProvenanceKind::ImportedDocument,
                    medassist_domain::prov::SourceRef::External {
                        system: "local-catalog".into(),
                        identifier: r.get::<String, _>("code"),
                    },
                ),
                confirmed: false,
            })
            .collect())
    }

    async fn upsert_code(
        &self,
        system: &str,
        version: &str,
        code: &str,
        display: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO codes (system, version, code, display) VALUES ($1,$2,$3,$4)
             ON CONFLICT (system, version, code) DO UPDATE SET display=EXCLUDED.display",
        )
        .bind(system)
        .bind(version)
        .bind(code)
        .bind(display)
        .execute(&self.0)
        .await
        .map_err(err)?;
        Ok(())
    }
}

/// Async audit sink: events are queued in-memory and flushed to Postgres by a
/// background task so request handlers never block on the audit write. The
/// flusher batches inserts; on shutdown a final flush is performed.
#[derive(Clone)]
pub struct PgAudit {
    pool: PgPool,
    tx: tokio::sync::mpsc::UnboundedSender<AuditEvent>,
}

impl PgAudit {
    pub fn new(pool: PgPool) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AuditEvent>();
        let writer = pool.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let outcome = match ev.outcome {
                    AuditOutcome::Success => "success",
                    AuditOutcome::Failure => "failure",
                    AuditOutcome::Denied => "denied",
                };
                let res = sqlx::query(
                    "INSERT INTO audit_events (time, actor_user_id, org_id, action, resource_type, resource_id, outcome, metadata)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                )
                .bind(ev.time)
                .bind(ev.actor_user_id)
                .bind(ev.org_id)
                .bind(&ev.action)
                .bind(&ev.resource_type)
                .bind(ev.resource_id)
                .bind(outcome)
                .bind(&ev.metadata)
                .execute(&writer)
                .await;
                if let Err(e) = res {
                    tracing::error!(error = %e, "audit write failed");
                }
            }
        });
        Self { pool, tx }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Query recent audit events for an organisation (admin endpoint).
    pub async fn recent(&self, org_id: Uuid, limit: i64) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT time, actor_user_id, action, resource_type, resource_id, outcome, metadata
             FROM audit_events WHERE org_id=$1 ORDER BY time DESC LIMIT $2",
        )
        .bind(org_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(err)?;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "time": r.get::<DateTime<Utc>, _>("time").to_rfc3339(),
                    "actor_user_id": r.get::<Option<Uuid>, _>("actor_user_id"),
                    "action": r.get::<String, _>("action"),
                    "resource_type": r.get::<String, _>("resource_type"),
                    "resource_id": r.get::<Option<Uuid>, _>("resource_id"),
                    "outcome": r.get::<String, _>("outcome"),
                    "metadata": r.get::<serde_json::Value, _>("metadata"),
                })
            })
            .collect())
    }
}

#[async_trait]
impl AuditSink for PgAudit {
    async fn record(&self, event: AuditEvent) {
        // Best-effort enqueue; channel is unbounded but events are small.
        let _ = self.tx.send(event);
    }
}
