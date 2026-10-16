//! Transcription use-cases: consent-gated capture, engine-agnostic job
//! creation, review flow and conversion into a clinical draft.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use medassist_domain::note::{NoteBody, SoapContent, SoapSection};
use medassist_domain::prov::{Provenance, ProvenanceKind, SourceRef};
use medassist_domain::transcription::{RecordingConsent, Transcription, TranscriptionState};
use medassist_domain::DomainError;

use crate::errors::{Actor, AppError};
use crate::ports::{
    AuditEvent, AuditOutcome, AuditSink, ConsentRepository, TranscriptionRepository,
};

/// Engine port. Implementations decide internally whether they provide
/// timestamps/confidence/diarisation — the contract never promises them.
#[async_trait::async_trait]
pub trait TranscriptionEngine: Send + Sync {
    fn id(&self) -> String;
    fn supports_timestamps(&self) -> bool;
    fn supports_confidence(&self) -> bool;
    fn supports_diarisation(&self) -> bool;
    /// Whether audio leaves the deployment boundary (remote service).
    fn is_external(&self) -> bool;
    async fn transcribe(
        &self,
        audio: &[u8],
        language: Option<&str>,
    ) -> Result<Vec<medassist_domain::transcription::TranscriptSegment>, AppError>;
}

pub struct TranscriptionService<T: TranscriptionRepository, C: ConsentRepository, A: AuditSink> {
    pub repo: T,
    pub consents: C,
    pub audit: A,
}

impl<T: TranscriptionRepository, C: ConsentRepository, A: AuditSink> TranscriptionService<T, C, A> {
    pub fn new(repo: T, consents: C, audit: A) -> Self {
        Self {
            repo,
            consents,
            audit,
        }
    }

    pub async fn record_consent(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        granted_by: String,
        scope: String,
        retention_policy: String,
        now: DateTime<Utc>,
    ) -> Result<RecordingConsent, AppError> {
        if granted_by.trim().is_empty()
            || scope.trim().is_empty()
            || retention_policy.trim().is_empty()
        {
            return Err(DomainError::validation(
                "consent",
                "granted_by, scope and retention policy are required",
            )
            .into());
        }
        let c = RecordingConsent {
            id: Uuid::new_v4(),
            org_id: actor.org_id,
            patient_id,
            granted_by: granted_by.trim().to_owned(),
            scope: scope.trim().to_owned(),
            retention_policy: retention_policy.trim().to_owned(),
            granted_at: now,
            revoked_at: None,
        };
        self.consents.insert(&c).await?;
        self.audit(actor, Some(c.id), "consent.grant", AuditOutcome::Success)
            .await;
        Ok(c)
    }

    /// Mark a transcription as human-reviewed. Required before its text may
    /// become part of a clinical draft (see `to_draft_note_body`).
    pub async fn mark_reviewed(
        &self,
        actor: &Actor,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        // Authorised read first (org-scoped), then persist the review flag.
        let _t = self.get(actor, id).await?;
        self.repo.mark_reviewed(actor, id, now).await?;
        self.audit(
            actor,
            Some(id),
            "transcription.review",
            AuditOutcome::Success,
        )
        .await;
        Ok(())
    }

    /// Create a pending transcription. MUST fail without a valid consent —
    /// silent/background recording is impossible by construction here.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        encounter_id: Option<Uuid>,
        consent_id: Uuid,
        engine: &str,
        retention_policy: String,
        now: DateTime<Utc>,
    ) -> Result<Transcription, AppError> {
        let consent = self
            .consents
            .get(actor, consent_id)
            .await?
            .ok_or_else(|| AppError::Unprocessable("unknown consent".into()))?;
        if consent.patient_id != patient_id {
            self.audit(
                actor,
                Some(consent_id),
                "transcription.start.consent_mismatch",
                AuditOutcome::Denied,
            )
            .await;
            return Err(AppError::Forbidden("consent does not cover this patient"));
        }
        if !consent.is_valid_at(now) {
            self.audit(
                actor,
                Some(consent_id),
                "transcription.start.consent_invalid",
                AuditOutcome::Denied,
            )
            .await;
            return Err(AppError::Unprocessable(
                "consent is revoked or not yet valid".into(),
            ));
        }
        let t = Transcription {
            id: Uuid::new_v4(),
            org_id: actor.org_id,
            patient_id,
            encounter_id,
            state: TranscriptionState::Pending,
            engine: engine.to_owned(),
            engine_version: None,
            language: Some("pt-BR".into()),
            segments: vec![],
            reviewed: false,
            consent_id,
            audio_retention_policy: retention_policy,
            created_at: now,
            updated_at: now,
        };
        self.repo.insert(&t).await?;
        self.audit(
            actor,
            Some(t.id),
            "transcription.start",
            AuditOutcome::Success,
        )
        .await;
        Ok(t)
    }

    pub async fn cancel(
        &self,
        actor: &Actor,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let t = self.get(actor, id).await?;
        if t.state == TranscriptionState::Completed {
            return Err(DomainError::IllegalTransition {
                from: "completed".into(),
                to: "cancelled".into(),
            }
            .into());
        }
        self.repo
            .fail(actor, id, "cancelled_by_user".into(), now)
            .await?;
        self.audit(
            actor,
            Some(id),
            "transcription.cancel",
            AuditOutcome::Success,
        )
        .await;
        Ok(())
    }

    pub async fn get(&self, actor: &Actor, id: Uuid) -> Result<Transcription, AppError> {
        match self.repo.get(actor, id).await? {
            Some(t) => Ok(t),
            None => Err(AppError::NotFound("transcription")),
        }
    }

    /// Convert a *reviewed* transcription into a SOAP subjective draft with
    /// provenance pointing back at the transcription.
    pub async fn to_draft_note_body(&self, actor: &Actor, id: Uuid) -> Result<NoteBody, AppError> {
        let t = self.get(actor, id).await?;
        if t.state != TranscriptionState::Completed {
            return Err(AppError::Unprocessable(
                "transcription is not completed".into(),
            ));
        }
        if !t.reviewed {
            return Err(AppError::Unprocessable(
                "transcription must be reviewed/corrected before becoming a clinical draft".into(),
            ));
        }
        let text = t.full_text();
        if text.trim().is_empty() {
            return Err(AppError::Unprocessable("empty transcription".into()));
        }
        Ok(NoteBody::Soap(Box::new(SoapContent {
            subjective: Some(SoapSection {
                text,
                provenance: Provenance::new(
                    ProvenanceKind::Transcription,
                    SourceRef::Transcription {
                        id: t.id,
                        start_ms: None,
                        end_ms: None,
                    },
                ),
                reviewed: true, // human already reviewed the transcript itself
            }),
            ..Default::default()
        })))
    }

    async fn audit(
        &self,
        actor: &Actor,
        resource_id: Option<Uuid>,
        action: &str,
        outcome: AuditOutcome,
    ) {
        self.audit
            .record(AuditEvent {
                time: Utc::now(),
                actor_user_id: Some(actor.user_id),
                org_id: Some(actor.org_id),
                action: action.into(),
                resource_type: "transcription".into(),
                resource_id,
                outcome,
                metadata: serde_json::json!({}),
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::ports::Page;
    use medassist_domain::transcription::TranscriptSegment;

    #[derive(Default)]
    struct MemTranscriptions(Mutex<HashMap<Uuid, Transcription>>);

    #[async_trait::async_trait]
    impl TranscriptionRepository for MemTranscriptions {
        async fn insert(&self, t: &Transcription) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(t.id, t.clone());
            Ok(())
        }
        async fn get(&self, _a: &Actor, id: Uuid) -> Result<Option<Transcription>, AppError> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
        async fn complete(
            &self,
            _a: &Actor,
            id: Uuid,
            segments: Vec<TranscriptSegment>,
            engine: &str,
            _ev: Option<String>,
            now: DateTime<Utc>,
        ) -> Result<(), AppError> {
            let mut g = self.0.lock().unwrap();
            let t = g.get_mut(&id).ok_or(AppError::NotFound("t"))?;
            t.segments = segments;
            t.engine = engine.into();
            t.state = TranscriptionState::Completed;
            t.updated_at = now;
            Ok(())
        }
        async fn fail(
            &self,
            _a: &Actor,
            id: Uuid,
            _err: String,
            now: DateTime<Utc>,
        ) -> Result<(), AppError> {
            if let Some(t) = self.0.lock().unwrap().get_mut(&id) {
                t.state = TranscriptionState::Cancelled;
                t.updated_at = now;
            }
            Ok(())
        }
        async fn mark_reviewed(
            &self,
            _a: &Actor,
            id: Uuid,
            _now: DateTime<Utc>,
        ) -> Result<(), AppError> {
            if let Some(t) = self.0.lock().unwrap().get_mut(&id) {
                t.reviewed = true;
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemConsents(Mutex<HashMap<Uuid, RecordingConsent>>);

    #[async_trait::async_trait]
    impl ConsentRepository for MemConsents {
        async fn insert(&self, c: &RecordingConsent) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(c.id, c.clone());
            Ok(())
        }
        async fn get(&self, _a: &Actor, id: Uuid) -> Result<Option<RecordingConsent>, AppError> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
    }

    #[derive(Default)]
    struct MemAudit(Mutex<Vec<AuditEvent>>);
    #[async_trait::async_trait]
    impl AuditSink for MemAudit {
        async fn record(&self, ev: AuditEvent) {
            self.0.lock().unwrap().push(ev);
        }
    }

    fn svc() -> TranscriptionService<MemTranscriptions, MemConsents, MemAudit> {
        TranscriptionService::new(
            MemTranscriptions::default(),
            MemConsents::default(),
            MemAudit::default(),
        )
    }

    fn actor() -> Actor {
        Actor {
            user_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            roles: vec!["practitioner".into()],
        }
    }

    #[tokio::test]
    async fn start_requires_valid_consent_for_same_patient() {
        let s = svc();
        let a = actor();
        let pid = Uuid::new_v4();
        let err = s
            .start(
                &a,
                pid,
                None,
                Uuid::new_v4(),
                "double",
                "keep_until_finalisation".into(),
                Utc::now(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unprocessable(_)));

        let c = s
            .record_consent(
                &a,
                pid,
                "paciente".into(),
                "áudio da consulta".into(),
                "delete_after_finalisation".into(),
                Utc::now(),
            )
            .await
            .unwrap();
        let t = s
            .start(
                &a,
                pid,
                None,
                c.id,
                "double",
                "delete_after_finalisation".into(),
                Utc::now(),
            )
            .await
            .unwrap();
        assert_eq!(t.state, TranscriptionState::Pending);

        // other patient with same consent -> forbidden
        let other = Uuid::new_v4();
        let err = s
            .start(&a, other, None, c.id, "double", "x".into(), Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));

        // revoked consent -> refused
        let revoked = RecordingConsent {
            revoked_at: Some(Utc::now() - chrono::Duration::seconds(1)),
            ..c.clone()
        };
        s.consents.0.lock().unwrap().insert(revoked.id, revoked);
        let err = s
            .start(&a, pid, None, c.id, "double", "x".into(), Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unprocessable(_)));
    }

    #[tokio::test]
    async fn unreviewed_transcription_cannot_become_draft() {
        let s = svc();
        let a = actor();
        let pid = Uuid::new_v4();
        let c = s
            .record_consent(
                &a,
                pid,
                "paciente".into(),
                "áudio".into(),
                "policy".into(),
                Utc::now(),
            )
            .await
            .unwrap();
        let t = s
            .start(&a, pid, None, c.id, "double", "policy".into(), Utc::now())
            .await
            .unwrap();
        // complete it via repo directly (worker would do this)
        s.repo
            .complete(
                &a,
                t.id,
                vec![TranscriptSegment {
                    start_ms: 0,
                    end_ms: 2000,
                    text: "relato".into(),
                    confidence: None,
                    speaker: None,
                }],
                "double",
                None,
                Utc::now(),
            )
            .await
            .unwrap();
        let err = s.to_draft_note_body(&a, t.id).await.unwrap_err();
        assert!(matches!(err, AppError::Unprocessable(_)));
        s.repo.mark_reviewed(&a, t.id, Utc::now()).await.unwrap();
        let body = s.to_draft_note_body(&a, t.id).await.unwrap();
        match body {
            NoteBody::Soap(x) => {
                let sec = x.subjective.unwrap();
                assert_eq!(sec.provenance.kind, ProvenanceKind::Transcription);
                assert!(sec.reviewed);
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn cancel_sets_state_and_audits() {
        let s = svc();
        let a = actor();
        let pid = Uuid::new_v4();
        let c = s
            .record_consent(&a, pid, "p".into(), "s".into(), "r".into(), Utc::now())
            .await
            .unwrap();
        let t = s
            .start(&a, pid, None, c.id, "double", "r".into(), Utc::now())
            .await
            .unwrap();
        s.cancel(&a, t.id, Utc::now()).await.unwrap();
        let got = s.get(&a, t.id).await.unwrap();
        assert_eq!(got.state, TranscriptionState::Cancelled);
        assert!(s
            .audit
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.action == "transcription.cancel"));
    }

    // silence unused import warning helper
    #[allow(dead_code)]
    fn _page_type_used(_: Page<u8>) {}
}
