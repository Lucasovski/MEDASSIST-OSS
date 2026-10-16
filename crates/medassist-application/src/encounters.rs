//! Encounter use-cases with lifecycle transitions and org isolation checks.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use medassist_domain::encounter::{Encounter, EncounterStatus, NewEncounter};
use medassist_domain::DomainError;

use crate::errors::{Actor, AppError};
use crate::ports::{
    AuditEvent, AuditOutcome, AuditSink, EncounterRepository, Page, PatientRepository,
};

pub struct EncounterService<R: EncounterRepository, P: PatientRepository, A: AuditSink> {
    pub repo: R,
    pub patients: P,
    pub audit: A,
}

impl<R: EncounterRepository, P: PatientRepository, A: AuditSink> EncounterService<R, P, A> {
    pub fn new(repo: R, patients: P, audit: A) -> Self {
        Self {
            repo,
            patients,
            audit,
        }
    }

    pub async fn create(
        &self,
        actor: &Actor,
        input: NewEncounter,
        now: DateTime<Utc>,
    ) -> Result<Encounter, AppError> {
        if input.org_id != actor.org_id && !actor.is_admin() {
            self.audit(actor, None, "encounter.create", "denied").await;
            return Err(AppError::Forbidden(
                "cannot create encounters for another organisation",
            ));
        }
        input.validate()?;
        // The patient must exist *within the actor's org* — a random id from
        // another tenant must not be linkable.
        let patient = self
            .patients
            .get(actor, input.patient_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        if patient.org_id != input.org_id {
            return Err(DomainError::validation(
                "patient_id",
                "patient belongs to a different organisation",
            )
            .into());
        }
        let id = Uuid::new_v4();
        let enc = input.into_encounter(id, now);
        self.repo.insert(&enc).await?;
        self.audit(actor, Some(enc.id), "encounter.create", "success")
            .await;
        Ok(enc)
    }

    pub async fn get(&self, actor: &Actor, id: Uuid) -> Result<Encounter, AppError> {
        match self.repo.get(actor, id).await? {
            Some(e) => Ok(e),
            None => {
                self.audit(actor, Some(id), "encounter.read", "denied")
                    .await;
                Err(AppError::NotFound("encounter"))
            }
        }
    }

    pub async fn list_for_patient(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Page<Encounter>, AppError> {
        self.repo
            .list_for_patient(actor, patient_id, limit, offset)
            .await
    }

    pub async fn transition(
        &self,
        actor: &Actor,
        id: Uuid,
        to: EncounterStatus,
        now: DateTime<Utc>,
    ) -> Result<Encounter, AppError> {
        let mut e = self.get(actor, id).await?;
        e.transition(to, now)?;
        self.repo.update(&e).await?;
        self.audit(
            actor,
            Some(e.id),
            &format!("encounter.transition.{}", to.as_str()),
            "success",
        )
        .await;
        Ok(e)
    }

    pub async fn update_chief_complaint(
        &self,
        actor: &Actor,
        id: Uuid,
        complaint: String,
        now: DateTime<Utc>,
    ) -> Result<Encounter, AppError> {
        let mut e = self.get(actor, id).await?;
        if matches!(
            e.status,
            EncounterStatus::Finalised | EncounterStatus::Cancelled
        ) {
            return Err(DomainError::ImmutableField.into());
        }
        if complaint.chars().count() > 2000 {
            return Err(DomainError::validation(
                "chief_complaint",
                "must be at most 2000 characters",
            )
            .into());
        }
        e.chief_complaint = Some(complaint.trim().to_owned()).filter(|c| !c.is_empty());
        e.updated_at = now;
        self.repo.update(&e).await?;
        self.audit(actor, Some(e.id), "encounter.update", "success")
            .await;
        Ok(e)
    }

    async fn audit(&self, actor: &Actor, resource_id: Option<Uuid>, action: &str, outcome: &str) {
        self.audit
            .record(AuditEvent {
                time: Utc::now(),
                actor_user_id: Some(actor.user_id),
                org_id: Some(actor.org_id),
                action: action.to_owned(),
                resource_type: "encounter".to_owned(),
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
