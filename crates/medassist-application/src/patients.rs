//! Patient use-cases: create with duplicate *flagging* (never auto-merge),
//! get/search/update, all org-scoped through the actor.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use medassist_domain::patient::{NewPatient, Patient};
use medassist_domain::DomainError;

use crate::errors::{Actor, AppError};
use crate::ports::{AuditEvent, AuditOutcome, AuditSink, Page, PatientQuery, PatientRepository};

/// Score at/above which creation surfaces duplicate candidates for review.
pub const DUPLICATE_REVIEW_THRESHOLD: f64 = 0.6;

pub struct PatientService<R: PatientRepository, A: AuditSink> {
    pub repo: R,
    pub audit: A,
}

impl<R: PatientRepository, A: AuditSink> PatientService<R, A> {
    pub fn new(repo: R, audit: A) -> Self {
        Self { repo, audit }
    }

    /// Create a patient. If strong duplicate candidates exist, the creation is
    /// *blocked* with [`DomainError::DuplicateCandidate`] listing candidate
    /// ids — a human must decide (merge manually or proceed with distinct id).
    pub async fn create(
        &self,
        actor: &Actor,
        input: NewPatient,
        allow_duplicates: bool,
        now: DateTime<Utc>,
    ) -> Result<Patient, AppError> {
        if input.org_id != actor.org_id && !actor.is_admin() {
            self.audit(actor, None, "patient.create", "denied").await;
            return Err(AppError::Forbidden(
                "cannot create patients for another organisation",
            ));
        }
        input.validate()?;

        if !allow_duplicates {
            let keys: Vec<String> = input
                .external_ids
                .iter()
                .map(|e| e.normalised_key())
                .collect();
            let candidates = self
                .repo
                .duplicate_candidates(
                    actor,
                    format!("{} {}", input.given_name, input.family_name),
                    input.birth_date,
                    keys,
                )
                .await?;
            let strong: Vec<Uuid> = candidates
                .into_iter()
                .filter(|(_, score)| *score >= DUPLICATE_REVIEW_THRESHOLD)
                .map(|(p, _)| p.id)
                .collect();
            if !strong.is_empty() {
                self.audit(actor, None, "patient.create.duplicate_blocked", "failure")
                    .await;
                return Err(DomainError::DuplicateCandidate(
                    strong
                        .iter()
                        .map(|u| u.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                )
                .into());
            }
        }

        let id = Uuid::new_v4();
        let patient = input.into_patient(id, now);
        self.repo.insert(&patient).await?;
        self.audit(actor, Some(patient.id), "patient.create", "success")
            .await;
        Ok(patient)
    }

    pub async fn get(&self, actor: &Actor, id: Uuid) -> Result<Patient, AppError> {
        match self.repo.get(actor, id).await? {
            Some(p) => Ok(p),
            None => {
                self.audit(actor, Some(id), "patient.read", "denied").await;
                Err(AppError::NotFound("patient"))
            }
        }
    }

    pub async fn search(&self, actor: &Actor, q: PatientQuery) -> Result<Page<Patient>, AppError> {
        self.repo.search(actor, &q).await
    }

    /// Update mutable demographics. Identity fields (id/org) are not editable.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        actor: &Actor,
        id: Uuid,
        family_name: Option<String>,
        given_name: Option<String>,
        birth_date: Option<Option<chrono::NaiveDate>>,
        sex: Option<Option<medassist_domain::patient::Sex>>,
        contact: Option<Option<String>>,
        notes: Option<Option<String>>,
        now: DateTime<Utc>,
    ) -> Result<Patient, AppError> {
        let mut p = self.get(actor, id).await?;
        if let Some(v) = family_name {
            if v.trim().is_empty() {
                return Err(DomainError::validation("family_name", "must not be empty").into());
            }
            p.family_name = v.trim().to_owned();
        }
        if let Some(v) = given_name {
            p.given_name = v.trim().to_owned();
        }
        if let Some(v) = birth_date {
            if let Some(bd) = v {
                if bd > now.date_naive() {
                    return Err(
                        DomainError::validation("birth_date", "cannot be in the future").into(),
                    );
                }
            }
            p.birth_date = v;
        }
        if let Some(v) = sex {
            p.sex = v;
        }
        if let Some(v) = contact {
            p.contact = v.filter(|c| !c.trim().is_empty());
        }
        if let Some(v) = notes {
            p.notes = v.filter(|n| !n.trim().is_empty());
        }
        p.updated_at = now;
        self.repo.update(&p).await?;
        self.audit(actor, Some(p.id), "patient.update", "success")
            .await;
        Ok(p)
    }

    /// Soft delete (tombstone). Hard deletion/retention is an ops policy.
    pub async fn soft_delete(
        &self,
        actor: &Actor,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        if !actor.has_role("recorder") && !actor.is_admin() {
            self.audit(actor, Some(id), "patient.delete", "denied")
                .await;
            return Err(AppError::Forbidden("requires recorder or admin role"));
        }
        let removed = self.repo.soft_delete(actor, id, now).await?;
        if !removed {
            return Err(AppError::NotFound("patient"));
        }
        self.audit(actor, Some(id), "patient.soft_delete", "success")
            .await;
        Ok(())
    }

    async fn audit(&self, actor: &Actor, resource_id: Option<Uuid>, action: &str, outcome: &str) {
        self.audit
            .record(AuditEvent {
                time: Utc::now(),
                actor_user_id: Some(actor.user_id),
                org_id: Some(actor.org_id),
                action: action.to_owned(),
                resource_type: "patient".to_owned(),
                resource_id,
                outcome: match outcome {
                    "success" => AuditOutcome::Success,
                    "denied" => AuditOutcome::Denied,
                    _ => AuditOutcome::Failure,
                },
                metadata: serde_json::json!({}),
            })
            .await;
    }
}
