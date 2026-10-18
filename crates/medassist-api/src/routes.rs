//! Route table + handlers for the versioned REST API. Thin transport over the
//! application services; every handler enforces authentication via extractor.


use axum::{
    extract::{Multipart, Path, Query, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use medassist_application::{
    ai::DocumentationRequest,
    clamp_page,
    errors::Actor,
    ports::PatientQuery as RepoPatientQuery,
};
use medassist_domain::identifier::ExternalIdentifier;
use medassist_domain::note::NoteBody;
use medassist_domain::patient::{NewPatient, Sex};
use medassist_domain::encounter::EncounterStatus;

use crate::error::ApiError;
use crate::state::{AppState, EngineChoice};

// ---------------------------------------------------------------------------
// Authentication extractor
// ---------------------------------------------------------------------------

/// Authenticated actor attached to requests by `require_auth`.
#[derive(Clone)]
pub struct Authed(pub Actor);

impl axum::extract::FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    #[allow(clippy::manual_async_fn)]
    fn from_request_parts<'a>(
        parts: &'a mut axum::http::request::Parts,
        state: &'a AppState,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'a>> {
        let _ = state;
        let res = parts
            .extensions
            .get::<Authed>()
            .cloned()
            .ok_or_else(ApiError::unauthorized);
        Box::pin(async move { res })
    }
}

#[axum::debug_handler]
async fn require_auth(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    {
        let token = extract_token(&req).to_string();
        if token.is_empty() {
            return ApiError::unauthorized().into_response();
        }
        match st.auth.resolve_session(&token, Utc::now()).await {
            Ok(actor) => {
                let mut req = req;
                req.extensions_mut().insert(Authed(actor));
                next.run(req).await
            }
            Err(e) => ApiError::from(e).into_response(),
        }
    }
}

fn extract_token(req: &Request) -> &str {
    if let Some(h) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Some(rest) = h.to_str().ok().and_then(|s| s.strip_prefix("Bearer ")) {
            return rest.trim();
        }
    }
    if let Some(cookie) = req.headers().get(axum::http::header::COOKIE) {
        if let Ok(s) = cookie.to_str() {
            for part in s.split(';') {
                let part = part.trim();
                if let Some(v) = part.strip_prefix("medassist_session=") {
                    return v.trim();
                }
            }
        }
    }
    ""
}

fn actor_of(req: &Request) -> Result<Actor, ApiError> {
    req.extensions()
        .get::<Authed>()
        .map(|a| a.0.clone())
        .ok_or_else(ApiError::unauthorized)
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn api_v1(st: AppState) -> Router<AppState> {
    let authed = Router::new()
        // patients
        .route("/patients", get(list_patients).post(create_patient))
        .route("/patients/:id", get(get_patient).patch(update_patient).delete(delete_patient))
        .route("/patients/:id/encounters", get(list_encounters).post(create_encounter))
        // encounters
        .route("/encounters/:id", get(get_encounter).patch(patch_encounter))
        .route("/encounters/:id/transition", axum::routing::post(transition_encounter))
        .route("/encounters/:id/notes", get(list_notes).post(create_note))
        // notes
        .route("/notes/:id", get(get_note).patch(edit_note))
        .route("/notes/:id/versions", get(list_versions))
        .route("/notes/:id/review-section", axum::routing::post(review_section))
        .route("/notes/:id/finalize", axum::routing::post(finalize_note))
        .route("/notes/:id/amend", axum::routing::post(amend_note))
        // documents
        .route("/documents", axum::routing::post(upload_document))
        .route("/documents/:id", get(get_document))
        .route("/documents/:id/content", get(download_document))
        .route("/documents/:id/review", axum::routing::post(review_document))
        .route("/patients/:id/documents", get(list_patient_documents))
        // transcription
        .route("/consents", axum::routing::post(create_consent))
        .route("/transcriptions", axum::routing::post(start_transcription))
        .route("/transcriptions/:id", get(get_transcription))
        .route("/transcriptions/:id/cancel", axum::routing::post(cancel_transcription))
        .route("/transcriptions/:id/review", axum::routing::post(review_transcription))
        .route("/transcriptions/:id/draft-body", get(transcription_draft_body))
        // AI drafts (async jobs)
        .route("/ai/documentation-drafts", axum::routing::post(create_ai_draft))
        .route("/jobs/:id", get(get_job))
        // templates / coding / audit / capabilities
        .route("/templates", get(list_templates))
        .route("/codes/search", get(search_codes))
        .route("/audit-events", get(list_audit))
        .route("/system/capabilities", get(capabilities))
        .route("/auth/logout", axum::routing::post(logout))
        .layer(middleware::from_fn_with_state(st.clone(), require_auth));

    let public = Router::new().route("/auth/login", post(login));

    public.merge(authed).with_state(st)
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

pub async fn health_live() -> impl IntoResponse {
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") })),
    )
}

pub async fn health_ready(State(st): State<AppState>) -> Response {
    use sqlx::Row;
    match sqlx::query("SELECT 1 AS ok").fetch_one(&st.jobs.0).await {
        Ok(r) if r.try_get::<i32, _>("ok").unwrap_or(0) == 1 => {
            (axum::http::StatusCode::OK, Json(serde_json::json!({ "database": "up" }))).into_response()
        }
        _ => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "database": "down" })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResp {
    session_token: String,
    user_id: Uuid,
    org_id: Uuid,
    roles: Vec<String>,
}

#[derive(Deserialize)]
struct ExternalIdDto {
    system: String,
    value: String,
    issuer: Option<String>,
}

#[derive(Deserialize)]
struct NewPatientDto {
    family_name: String,
    given_name: String,
    birth_date: Option<chrono::NaiveDate>,
    sex: Option<String>,
    contact: Option<String>,
    #[serde(default)]
    external_ids: Vec<ExternalIdDto>,
    notes: Option<String>,
    /// Explicit opt-out of duplicate detection after human review.
    #[serde(default)]
    allow_duplicates: bool,
}

#[derive(Deserialize)]
struct UpdatePatientDto {
    family_name: Option<String>,
    given_name: Option<String>,
    birth_date: Option<Option<chrono::NaiveDate>>,
    sex: Option<Option<String>>,
    contact: Option<Option<String>>,
    notes: Option<Option<String>>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct NewEncounterDto {
    patient_id: Uuid,
    chief_complaint: Option<String>,
}

#[derive(Deserialize)]
struct TransitionDto {
    to: String,
}

#[derive(Deserialize)]
struct PatchEncounterDto {
    chief_complaint: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SoapDto {
    pub subjective: Option<String>,
    pub objective: Option<String>,
    pub assessment: Option<String>,
    pub plan: Option<String>,
}

#[derive(Deserialize)]
struct NewNoteDto {
    encounter_id: Uuid,
    soap: SoapDto,
    template_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct EditNoteDto {
    expected_version: u32,
    soap: SoapDto,
}

#[derive(Deserialize)]
struct ReviewSectionDto {
    section: String,
}

#[derive(Deserialize)]
struct FinalizeDto {
    #[serde(default)]
    override_gaps: bool,
}

#[derive(Deserialize)]
struct AmendDto {
    soap: SoapDto,
}

#[derive(Deserialize)]
struct ConsentDto {
    patient_id: Uuid,
    granted_by: String,
    scope: String,
    retention_policy: String,
}

#[derive(Deserialize)]
struct StartTranscriptionDto {
    patient_id: Uuid,
    encounter_id: Option<Uuid>,
    consent_id: Uuid,
    engine: Option<String>,
    retention_policy: String,
}

#[derive(Deserialize)]
struct AiDraftDto {
    encounter_id: Uuid,
    sections: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct CodeSearchDto {
    system: Option<String>,
    text: String,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn soap_to_body(dto: SoapDto, author: Uuid) -> NoteBody {
    use medassist_domain::note::{SoapContent, SoapSection};
    NoteBody::Soap(SoapContent {
        subjective: dto.subjective.filter(|t| !t.trim().is_empty()).map(|t| SoapSection::professional(author, t)),
        objective: dto.objective.filter(|t| !t.trim().is_empty()).map(|t| SoapSection::professional(author, t)),
        assessment: dto.assessment.filter(|t| !t.trim().is_empty()).map(|t| SoapSection::professional(author, t)),
        plan: dto.plan.filter(|t| !t.trim().is_empty()).map(|t| SoapSection::professional(author, t)),
    })
}

fn body_to_soap(body: &NoteBody) -> serde_json::Value {
    use medassist_domain::note::NoteBody::*;
    match body {
        Soap(s) => serde_json::json!({
            "kind": "soap",
            "subjective": s.subjective,
            "objective": s.objective,
            "assessment": s.assessment,
            "plan": s.plan,
            "missing_sections": s.missing_sections(),
            "unreviewed_sections": s.unreviewed_sections(),
        }),
        FreeText { text, provenance, reviewed } => serde_json::json!({
            "kind": "free_text", "text": text, "provenance": provenance, "reviewed": reviewed
        }),
    }
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

async fn login(
    State(st): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Response, ApiError> {
    let (token, actor) = st.auth.login(&req.email, &req.password, now()).await?;
    let cookie = format!(
        "medassist_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=28800"
    );
    let resp = Json(LoginResp {
        session_token: token.clone(),
        user_id: actor.user_id,
        org_id: actor.org_id,
        roles: actor.roles.clone(),
    });
        Ok((
            [(axum::http::header::SET_COOKIE, cookie)],
            resp,
        )
            .into_response())
}

#[axum::debug_handler]
async fn logout(
    State(st): State<AppState>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = extract_token(&req).to_string();
    st.auth.logout(&token, now()).await.map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Patients
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn create_patient(
    State(st): State<AppState>,
    Json(dto): Json<NewPatientDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let sex = match dto.sex.as_deref() {
        None => None,
        Some(s) => Some(match s {
            "female" => Sex::Female,
            "male" => Sex::Male,
            "intersex" => Sex::Intersex,
            "unknown" => Sex::Unknown,
            "undisclosed" => Sex::Undisclosed,
            _ => return Err(ApiError::bad_request(format!("invalid sex '{s}'"))),
        }),
    };
    let external_ids = dto
        .external_ids
        .into_iter()
        .map(|e| {
            let system = match e.system.as_str() {
                "cns" => medassist_domain::identifier::ExternalIdSystem::Cns,
                "mrn" => medassist_domain::identifier::ExternalIdSystem::Mrn,
                "health_plan" => medassist_domain::identifier::ExternalIdSystem::HealthPlan,
                "passport" => medassist_domain::identifier::ExternalIdSystem::Passport,
                _ => medassist_domain::identifier::ExternalIdSystem::Other,
            };
            ExternalIdentifier { system, value: e.value, issuer: e.issuer }
        })
        .collect();
    let input = NewPatient {
        org_id: actor.org_id,
        family_name: dto.family_name,
        given_name: dto.given_name,
        birth_date: dto.birth_date,
        sex,
        contact: dto.contact,
        external_ids,
        notes: dto.notes,
    };
    let p = st.patients.create(&actor, input, dto.allow_duplicates, now()).await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&p).unwrap())))
}

#[axum::debug_handler]
async fn list_patients(
    State(st): State<AppState>,
    Query(q): Query<SearchQuery>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let (limit, offset) = clamp_page(q.limit, q.offset);
    let page = st
        .patients
        .search(
            &actor,
            RepoPatientQuery { text: q.q, limit, offset },
        )
        .await?;
    Ok(Json(serde_json::json!({
        "items": page.items, "total": page.total, "limit": page.limit, "offset": page.offset
    })))
}

#[axum::debug_handler]
async fn get_patient(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let p = st.patients.get(&actor, id).await?;
    Ok(Json(serde_json::to_value(&p).unwrap()))
}

#[axum::debug_handler]
async fn update_patient(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<UpdatePatientDto>,

    Authed(actor): Authed,) -> Result<Json<serde_json::Value>, ApiError> {
    let parse_sex = |s: &Option<Option<String>>| -> Result<Option<Option<Sex>>, ApiError> {
        match s {
            None => Ok(None),
            Some(inner) => match inner.as_deref() {
                None => Ok(Some(None)),
                Some(v) => Ok(Some(Some(match v {
                    "female" => Sex::Female,
                    "male" => Sex::Male,
                    "intersex" => Sex::Intersex,
                    "unknown" => Sex::Unknown,
                    "undisclosed" => Sex::Undisclosed,
                    _ => return Err(ApiError::bad_request(format!("invalid sex '{v}'"))),
                }))),
            },
        }
    };
    let p = st
        .patients
        .update(
            &actor,
            id,
            dto.family_name,
            dto.given_name,
            dto.birth_date,
            parse_sex(&dto.sex)?,
            dto.contact,
            dto.notes,
            now(),
        )
        .await?;
    Ok(Json(serde_json::to_value(&p).unwrap()))
}

#[axum::debug_handler]
async fn delete_patient(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    st.patients.soft_delete(&actor, id, now()).await?;
    Ok(Json(serde_json::json!({ "ok": true, "deleted": id })))
}

// ---------------------------------------------------------------------------
// Encounters
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn create_encounter(
    State(st): State<AppState>,
    Path(patient_id): Path<Uuid>,
    Json(dto): Json<NewEncounterDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    if dto.patient_id != patient_id {
        return Err(ApiError::bad_request("path/query patient_id mismatch"));
    }
    let e = st
        .encounters
        .create(
            &actor,
            medassist_domain::encounter::NewEncounter {
                org_id: actor.org_id,
                patient_id,
                practitioner_id: actor.user_id,
                chief_complaint: dto.chief_complaint,
            },
            now(),
        )
        .await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&e).unwrap())))
}

#[axum::debug_handler]
async fn list_encounters(
    State(st): State<AppState>,
    Path(patient_id): Path<Uuid>,
    Query(q): Query<SearchQuery>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let (limit, offset) = clamp_page(q.limit, q.offset);
    let page = st.encounters.list_for_patient(&actor, patient_id, limit, offset).await?;
    Ok(Json(serde_json::json!({
        "items": page.items, "total": page.total, "limit": page.limit, "offset": page.offset
    })))
}

#[axum::debug_handler]
async fn get_encounter(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let e = st.encounters.get(&actor, id).await?;
    Ok(Json(serde_json::to_value(&e).unwrap()))
}

#[axum::debug_handler]
async fn patch_encounter(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<PatchEncounterDto>,

    Authed(actor): Authed,) -> Result<Json<serde_json::Value>, ApiError> {
    let cc = dto.chief_complaint.ok_or_else(|| ApiError::bad_request("chief_complaint required"))?;
    let e = st.encounters.update_chief_complaint(&actor, id, cc, now()).await?;
    Ok(Json(serde_json::to_value(&e).unwrap()))
}

#[axum::debug_handler]
async fn transition_encounter(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<TransitionDto>,

    Authed(actor): Authed,) -> Result<Json<serde_json::Value>, ApiError> {
    let to = match dto.to.as_str() {
        "open" => EncounterStatus::Open,
        "in_progress" => EncounterStatus::InProgress,
        "closed" => EncounterStatus::Closed,
        "finalised" => EncounterStatus::Finalised,
        "cancelled" => EncounterStatus::Cancelled,
        s => return Err(ApiError::bad_request(format!("invalid status '{s}'"))),
    };
    let e = st.encounters.transition(&actor, id, to, now()).await?;
    Ok(Json(serde_json::to_value(&e).unwrap()))
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn create_note(
    State(st): State<AppState>,
    Path(encounter_id): Path<Uuid>,
    Json(dto): Json<NewNoteDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    if dto.encounter_id != encounter_id {
        return Err(ApiError::bad_request("encounter_id mismatch"));
    }
    let body = soap_to_body(dto.soap, actor.user_id);
    let n = st.notes.create_draft(&actor, encounter_id, body, dto.template_id, now()).await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&n).unwrap())))
}

#[axum::debug_handler]
async fn list_notes(
    State(st): State<AppState>,
    Path(encounter_id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let ns = st.notes.list_for_encounter(&actor, encounter_id).await?;
    Ok(Json(serde_json::json!({ "items": ns })))
}

#[axum::debug_handler]
async fn get_note(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let n = st.notes.get(&actor, id).await?;
    Ok(Json(serde_json::to_value(&n).unwrap()))
}

#[axum::debug_handler]
async fn edit_note(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<EditNoteDto>,

    Authed(actor): Authed,) -> Result<Json<serde_json::Value>, ApiError> {
    let body = soap_to_body(dto.soap, actor.user_id);
    let n = st.notes.edit(&actor, id, dto.expected_version, body, now()).await?;
    Ok(Json(serde_json::to_value(&n).unwrap()))
}

#[axum::debug_handler]
async fn review_section(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<ReviewSectionDto>,

    Authed(actor): Authed,) -> Result<Json<serde_json::Value>, ApiError> {
    let n = st.notes.review_section(&actor, id, &dto.section, now()).await?;
    Ok(Json(serde_json::to_value(&n).unwrap()))
}

#[axum::debug_handler]
async fn finalize_note(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<FinalizeDto>,

    Authed(actor): Authed,) -> Result<Json<serde_json::Value>, ApiError> {
    let n = st.notes.finalise(&actor, id, dto.override_gaps, now()).await?;
    Ok(Json(serde_json::to_value(&n).unwrap()))
}

#[axum::debug_handler]
async fn amend_note(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(dto): Json<AmendDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let body = soap_to_body(dto.soap, actor.user_id);
    let n = st.notes.amend(&actor, id, body, now()).await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&n).unwrap())))
}

#[axum::debug_handler]
async fn list_versions(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    // Read directly from the version store through the note repo port is not
    // exposed; use SQL here — versions are immutable snapshots.
    let rows = sqlx::query(
        "SELECT nv.version, nv.edited_by, nv.created_at, nv.body
         FROM note_versions nv JOIN notes n ON n.id = nv.note_id
         WHERE nv.note_id = $1 AND n.org_id = $2
         ORDER BY nv.version ASC",
    )
    .bind(id)
    .bind(actor.org_id)
    .fetch_all(&st.jobs.0)
    .await
    .map_err(|e| ApiError::from(medassist_application::AppError::storage(e)))?;
    use sqlx::Row;
    let items: Vec<_> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "version": r.get::<i32, _>("version"),
                "edited_by": r.get::<Uuid, _>("edited_by"),
                "created_at": r.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
                "body": r.get::<serde_json::Value, _>("body"),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "note_id": id, "versions": items })))
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn upload_document(
    State(st): State<AppState>,
    mut mp: Multipart,
    Authed(actor): Authed,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {

    let mut patient_id: Option<Uuid> = None;
    let mut encounter_id: Option<Uuid> = None;
    let mut filename = String::new();
    let mut field_ct = String::new();
    let mut bytes: Vec<u8> = Vec::new();

    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::bad_request(format!("multipart error: {e}")))? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "patient_id" => {
                let t = field.text().await.map_err(|_| ApiError::bad_request("bad patient_id"))?;
                patient_id = Some(Uuid::parse_str(&t).map_err(|_| ApiError::bad_request("patient_id must be uuid"))?);
            }
            "encounter_id" => {
                let t = field.text().await.map_err(|_| ApiError::bad_request("bad encounter_id"))?;
                if !t.trim().is_empty() {
                    encounter_id = Some(Uuid::parse_str(t.trim()).map_err(|_| ApiError::bad_request("encounter_id must be uuid"))?);
                }
            }
            "file" => {
                filename = field.file_name().unwrap_or("document").to_string();
                field_ct = field.content_type().map(|m| m.to_string()).unwrap_or_else(|| "application/octet-stream".to_string());
                bytes = field.bytes().await.map_err(|_| ApiError::bad_request("failed reading file"))?.to_vec();
            }
            _ => {}
        }
    }
    let patient_id = patient_id.ok_or_else(|| ApiError::bad_request("patient_id field required"))?;
    if bytes.is_empty() {
        return Err(ApiError::bad_request("file field required and non-empty"));
    }
    let doc = st
        .documents
        .import(&actor, patient_id, encounter_id, filename, field_ct, bytes)
        .await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&doc).unwrap())))
}

#[axum::debug_handler]
async fn get_document(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let d = st.documents.get(&actor, id).await?;
    Ok(Json(serde_json::to_value(&d).unwrap()))
}

#[axum::debug_handler]
async fn download_document(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Response, ApiError> {
    let actor = actor_of(&req)?;
    let d = st.documents.get(&actor, id).await?;
    let bytes = st.documents.download(&actor, id).await?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, d.content_type.clone()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", d.filename.replace('"', "")),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[axum::debug_handler]
async fn review_document(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    st.documents.mark_reviewed(&actor, id).await?;
    Ok(Json(serde_json::json!({ "ok": true, "state": "reviewed" })))
}

#[axum::debug_handler]
async fn list_patient_documents(
    State(st): State<AppState>,
    Path(patient_id): Path<Uuid>,
    Query(q): Query<SearchQuery>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let (limit, offset) = clamp_page(q.limit, q.offset);
    let page = st.documents.list_for_patient(&actor, patient_id, limit, offset).await?;
    Ok(Json(serde_json::json!({
        "items": page.items, "total": page.total, "limit": page.limit, "offset": page.offset
    })))
}

// ---------------------------------------------------------------------------
// Transcription / consent
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn create_consent(
    State(st): State<AppState>,
    Json(dto): Json<ConsentDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let c = st
        .transcriptions
        .record_consent(&actor, dto.patient_id, dto.granted_by, dto.scope, dto.retention_policy, now())
        .await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&c).unwrap())))
}

#[axum::debug_handler]
async fn start_transcription(
    State(st): State<AppState>,
    Json(dto): Json<StartTranscriptionDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let engine = dto.engine.unwrap_or_else(|| st.llm_id.clone());
    let t = st
        .transcriptions
        .start(&actor, dto.patient_id, dto.encounter_id, dto.consent_id, &engine, dto.retention_policy, now())
        .await?;
    Ok((axum::http::StatusCode::CREATED, Json(serde_json::to_value(&t).unwrap())))
}

#[axum::debug_handler]
async fn get_transcription(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let t = st.transcriptions.get(&actor, id).await?;
    Ok(Json(serde_json::to_value(&t).unwrap()))
}

#[axum::debug_handler]
async fn cancel_transcription(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    st.transcriptions.cancel(&actor, id, now()).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[axum::debug_handler]
async fn review_transcription(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    st.transcriptions.mark_reviewed(&actor, id, now()).await?;
    Ok(Json(serde_json::json!({ "ok": true, "reviewed": true })))
}

#[axum::debug_handler]
async fn transcription_draft_body(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let body = st.transcriptions.to_draft_note_body(&actor, id).await?;
    Ok(Json(body_to_soap(&body)))
}

// ---------------------------------------------------------------------------
// AI drafts + jobs
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn create_ai_draft(
    State(st): State<AppState>,
    Json(dto): Json<AiDraftDto>,

    Authed(actor): Authed,) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    use medassist_application::ports::JobRepository;
    let enc = st.encounters.get(&actor, dto.encounter_id).await?;
    let job = medassist_domain::job::AsyncJob::new(Uuid::new_v4(), actor.org_id, medassist_domain::job::JobKind::AiDocumentationDraft, now());
    st.jobs.insert(&job).await?;
    let request = DocumentationRequest {
        job_id: job.id,
        org_id: actor.org_id,
        patient_id: enc.patient_id,
        encounter_id: enc.id,
        sections: dto.sections.unwrap_or_else(|| vec!["subjective".into(), "objective".into(), "assessment".into(), "plan".into()]),
        transcript: None,
        authorised_context: enc.chief_complaint.clone().map(|c| vec![format!("queixa: {c}")]).unwrap_or_default(),
    };
    let note = st
        .notes
        .create_draft(
            &actor,
            enc.id,
            NoteBody::Soap(medassist_domain::note::SoapContent::default()),
            None,
            now(),
        )
        .await?;
    // Persist the job->note link and enqueue work for the background worker.
    let payload = serde_json::json!({ "request": request, "note_id": note.id, "actor_user_id": actor.user_id });
    sqlx::query("UPDATE jobs SET result_ref=$2 WHERE id=$1")
        .bind(job.id)
        .bind(payload.to_string())
        .execute(&st.jobs.0)
        .await
        .map_err(|e| ApiError::from(medassist_application::AppError::storage(e)))?;
    st.sink
        .record(medassist_application::ports::AuditEvent {
            time: now(),
            actor_user_id: Some(actor.user_id),
            org_id: Some(actor.org_id),
            action: "ai.draft.requested".into(),
            resource_type: "job".into(),
            resource_id: Some(job.id),
            outcome: medassist_application::ports::AuditOutcome::Success,
            metadata: serde_json::json!({ "engine": st.llm_id, "external": st.llm_external }),
        })
        .await;
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({ "job_id": job.id, "note_id": note.id, "state": job.state.as_str() })),
    ))
}

#[axum::debug_handler]
async fn get_job(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    use medassist_application::ports::JobRepository;
    let actor = actor_of(&req)?;
    let j = st
        .jobs
        .get(&actor, id)
        .await?
        .ok_or(medassist_application::AppError::NotFound("job"))?;
    Ok(Json(serde_json::json!({
        "id": j.id, "kind": j.kind, "state": j.state.as_str(),
        "attempts": j.attempts, "max_attempts": j.max_attempts,
        "error": j.error, "result_ref": j.result_ref,
    })))
}

// ---------------------------------------------------------------------------
// Templates / codes / audit / capabilities
// ---------------------------------------------------------------------------

#[axum::debug_handler]
async fn list_templates(
    State(st): State<AppState>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let ts = st.templates.list(&actor).await?;
    Ok(Json(serde_json::to_value(&ts).unwrap()))
}

#[axum::debug_handler]
async fn search_codes(
    State(st): State<AppState>,
    Query(q): Query<CodeSearchDto>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let _ = &actor;
    use medassist_application::ports::CodingRepository;
    let hits = st.codes.search_codes(q.system.as_deref().unwrap_or("cid10"), &q.text, q.limit.unwrap_or(10).clamp(1, 50)).await?;
    Ok(Json(serde_json::to_value(&hits).unwrap()))
}

#[axum::debug_handler]
async fn list_audit(
    State(st): State<AppState>,
    Query(q): Query<AuditQuery>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let (limit, offset) = clamp_page(q.limit, q.offset);
    let (events, total) = st.audit.list(&actor, limit, offset).await?;
    let items: Vec<_> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "time": e.time.to_rfc3339(),
                "actor_user_id": e.actor_user_id,
                "action": e.action,
                "resource_type": e.resource_type,
                "resource_id": e.resource_id,
                "outcome": format!("{:?}", e.outcome).to_lowercase(),
                "metadata": e.metadata,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items, "total": total, "limit": limit, "offset": offset })))
}

#[axum::debug_handler]
async fn capabilities(
    State(st): State<AppState>,
    req: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor_of(&req)?;
    let engine_id = match &st.engine {
        EngineChoice::Ollama(_) => st.llm_id.clone(),
        EngineChoice::Double(_) => st.llm_id.clone(),
    };
    Ok(Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "authenticated_as": { "user_id": actor.user_id, "roles": actor.roles },
        "ai": {
            "documentation_engine": engine_id,
            "data_leaves_deployment": st.llm_external,
            "test_double_in_use": matches!(st.engine, EngineChoice::Double(_)),
        },
        "features": {
            "patients": true, "encounters": true, "notes": true, "soap": true,
            "note_versions": true, "documents": true, "transcription_pipeline": true,
            "ai_drafts": true, "coding_search": true, "audit": true, "fhir_export": true,
        }
    })))
}
