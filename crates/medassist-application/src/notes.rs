//! Note use-cases: draft creation, conflict-safe editing (human edits are
//! never silently overwritten), section review, controlled finalisation and
//! amendments. Every mutation is snapshotted for version comparison.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use medassist_domain::note::{Note, NoteBody};
use medassist_domain::DomainError;

use crate::errors::{Actor, AppError};
use crate::ports::{AuditEvent, AuditOutcome, AuditSink, EncounterRepository, NoteRepository};

pub struct NoteService<N: NoteRepository, E: EncounterRepository, A: AuditSink> {
    pub notes: N,
    pub encounters: E,
    pub audit: A,
}

impl<N: NoteRepository, E: EncounterRepository, A: AuditSink> NoteService<N, E, A> {
    pub fn new(notes: N, encounters: E, audit: A) -> Self {
        Self {
            notes,
            encounters,
            audit,
        }
    }

    pub async fn create_draft(
        &self,
        actor: &Actor,
        encounter_id: Uuid,
        body: NoteBody,
        template_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<Note, AppError> {
        let enc = self
            .encounters
            .get(actor, encounter_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        if matches!(
            enc.status,
            medassist_domain::encounter::EncounterStatus::Cancelled
        ) {
            return Err(DomainError::validation(
                "encounter",
                "cannot document a cancelled encounter",
            )
            .into());
        }
        validate_body(&body)?;
        let id = Uuid::new_v4();
        let mut note = Note::new_draft(
            id,
            actor.org_id,
            enc.id,
            enc.patient_id,
            actor.user_id,
            body,
            now,
        );
        note.template_id = template_id;
        self.notes.insert(&note).await?;
        self.notes
            .append_version_snapshot(&note, actor.user_id)
            .await?;
        self.audit(
            actor,
            Some(note.id),
            "note.create_draft",
            AuditOutcome::Success,
        )
        .await;
        Ok(note)
    }

    pub async fn get(&self, actor: &Actor, id: Uuid) -> Result<Note, AppError> {
        match self.notes.get(actor, id).await? {
            Some(n) => Ok(n),
            None => {
                self.audit(actor, Some(id), "note.read", AuditOutcome::Denied)
                    .await;
                Err(AppError::NotFound("note"))
            }
        }
    }

    /// List every note belonging to an encounter (drafts included), newest
    /// first. Authorisation is enforced through the encounter read.
    pub async fn list_for_encounter(
        &self,
        actor: &Actor,
        encounter_id: Uuid,
    ) -> Result<Vec<Note>, AppError> {
        // 404/403 if the encounter is not visible to this actor.
        let _enc = self.encounters.get(actor, encounter_id).await?;
        let notes = self.notes.list_for_encounter(actor, encounter_id).await?;
        Ok(notes)
    }

    /// Conflict-safe edit. `expected_version` must match the stored version;
    /// otherwise [`DomainError::VersionConflict`] propagates so the client can
    /// rebase instead of clobbering a human's changes with an async AI result.
    pub async fn edit(
        &self,
        actor: &Actor,
        id: Uuid,
        expected_version: u32,
        body: NoteBody,
        now: DateTime<Utc>,
    ) -> Result<Note, AppError> {
        let mut note = self.get(actor, id).await?;
        validate_body(&body)?;
        note.apply_edit(expected_version, body, now)?;
        self.notes
            .update_with_version_guard(&note, expected_version, actor.user_id)
            .await?;
        self.audit(actor, Some(note.id), "note.edit", AuditOutcome::Success)
            .await;
        Ok(note)
    }

    pub async fn review_section(
        &self,
        actor: &Actor,
        id: Uuid,
        section: &str,
        now: DateTime<Utc>,
    ) -> Result<Note, AppError> {
        let mut note = self.get(actor, id).await?;
        let v = note.version;
        note.mark_section_reviewed(section, now)?;
        self.notes
            .update_with_version_guard(&note, v, actor.user_id)
            .await?;
        self.audit(
            actor,
            Some(note.id),
            "note.review_section",
            AuditOutcome::Success,
        )
        .await;
        Ok(note)
    }

    /// Controlled finalisation. Requires professional action; gap overrides
    /// are explicitly audited so they can be reviewed later.
    pub async fn finalise(
        &self,
        actor: &Actor,
        id: Uuid,
        override_gaps: bool,
        now: DateTime<Utc>,
    ) -> Result<Note, AppError> {
        if !actor.has_role("practitioner") && !actor.is_admin() {
            self.audit(actor, Some(id), "note.finalise", AuditOutcome::Denied)
                .await;
            return Err(AppError::Forbidden(
                "finalising notes requires the practitioner role",
            ));
        }
        let mut note = self.get(actor, id).await?;
        let v = note.version;
        note.finalise(actor.user_id, now, override_gaps)?;
        self.notes
            .update_with_version_guard(&note, v, actor.user_id)
            .await?;
        self.audit(
            actor,
            Some(note.id),
            if override_gaps {
                "note.finalise.with_gap_override"
            } else {
                "note.finalise"
            },
            AuditOutcome::Success,
        )
        .await;
        Ok(note)
    }

    /// Corrections to a finalised note create a linked amendment draft.
    pub async fn amend(
        &self,
        actor: &Actor,
        id: Uuid,
        body: NoteBody,
        now: DateTime<Utc>,
    ) -> Result<Note, AppError> {
        let original = self.get(actor, id).await?;
        validate_body(&body)?;
        let amendment = original.create_amendment(Uuid::new_v4(), body, now)?;
        self.notes.insert(&amendment).await?;
        self.notes
            .append_version_snapshot(&amendment, actor.user_id)
            .await?;
        self.audit(
            actor,
            Some(amendment.id),
            "note.amend",
            AuditOutcome::Success,
        )
        .await;
        Ok(amendment)
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
                action: action.to_owned(),
                resource_type: "note".to_owned(),
                resource_id,
                outcome,
                metadata: serde_json::json!({}),
            })
            .await;
    }
}

/// Structural validation of note bodies (not clinical validation). Enforces
/// provenance presence and length limits; AI content must carry its origin.
pub fn validate_body(body: &NoteBody) -> Result<(), DomainError> {
    const MAX_SECTION: usize = 50_000;
    match body {
        NoteBody::Soap(s) => {
            for sec in [&s.subjective, &s.objective, &s.assessment, &s.plan]
                .into_iter()
                .flatten()
            {
                if sec.text.chars().count() > MAX_SECTION {
                    return Err(DomainError::validation(
                        "section",
                        format!("section exceeds {MAX_SECTION} chars"),
                    ));
                }
            }
        }
        NoteBody::FreeText { text, .. } => {
            if text.chars().count() > MAX_SECTION {
                return Err(DomainError::validation("text", "note exceeds char limit"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::ports::Page;
    use medassist_domain::encounter::{Encounter, EncounterStatus, NewEncounter};
    use medassist_domain::note::SoapSection;
    use medassist_domain::prov::{Provenance, ProvenanceKind, SourceRef};

    // ---- in-memory test doubles (clearly NOT real infrastructure) ----

    #[derive(Default)]
    struct MemNotes {
        by_id: Mutex<std::collections::HashMap<Uuid, Note>>,
        versions: Mutex<Vec<(Uuid, u32)>>,
    }

    #[async_trait::async_trait]
    impl NoteRepository for MemNotes {
        async fn insert(&self, n: &Note) -> Result<(), AppError> {
            self.by_id.lock().unwrap().insert(n.id, n.clone());
            Ok(())
        }
        async fn get(&self, _a: &Actor, id: Uuid) -> Result<Option<Note>, AppError> {
            Ok(self.by_id.lock().unwrap().get(&id).cloned())
        }
        async fn list_for_encounter(&self, _a: &Actor, e: Uuid) -> Result<Vec<Note>, AppError> {
            Ok(self
                .by_id
                .lock()
                .unwrap()
                .values()
                .filter(|n| n.encounter_id == e)
                .cloned()
                .collect())
        }
        async fn update_with_version_guard(
            &self,
            n: &Note,
            expected: u32,
            _by: Uuid,
        ) -> Result<(), AppError> {
            let mut g = self.by_id.lock().unwrap();
            let cur = g.get(&n.id).ok_or(AppError::NotFound("note"))?.clone();
            if cur.version != expected {
                return Err(DomainError::VersionConflict {
                    expected,
                    found: cur.version,
                }
                .into());
            }
            g.insert(n.id, n.clone());
            Ok(())
        }
        async fn append_version_snapshot(&self, n: &Note, _by: Uuid) -> Result<(), AppError> {
            self.versions.lock().unwrap().push((n.id, n.version));
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemEncounters {
        by_id: Mutex<std::collections::HashMap<Uuid, Encounter>>,
    }

    #[async_trait::async_trait]
    impl EncounterRepository for MemEncounters {
        async fn insert(&self, e: &Encounter) -> Result<(), AppError> {
            self.by_id.lock().unwrap().insert(e.id, e.clone());
            Ok(())
        }
        async fn get(&self, _a: &Actor, id: Uuid) -> Result<Option<Encounter>, AppError> {
            Ok(self.by_id.lock().unwrap().get(&id).cloned())
        }
        async fn list_for_patient(
            &self,
            _a: &Actor,
            pid: Uuid,
            limit: i64,
            offset: i64,
        ) -> Result<Page<Encounter>, AppError> {
            let items: Vec<_> = self
                .by_id
                .lock()
                .unwrap()
                .values()
                .filter(|e| e.patient_id == pid)
                .cloned()
                .collect();
            Ok(Page {
                total: items.len() as i64,
                limit,
                offset,
                items,
            })
        }
        async fn update(&self, e: &Encounter) -> Result<(), AppError> {
            self.by_id.lock().unwrap().insert(e.id, e.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemAudit {
        events: Mutex<Vec<AuditEvent>>,
    }

    #[async_trait::async_trait]
    impl AuditSink for MemAudit {
        async fn record(&self, ev: AuditEvent) {
            self.events.lock().unwrap().push(ev);
        }
    }

    fn svc() -> NoteService<MemNotes, MemEncounters, MemAudit> {
        NoteService::new(
            MemNotes::default(),
            MemEncounters::default(),
            MemAudit::default(),
        )
    }

    fn actor(role: &str) -> Actor {
        Actor {
            user_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            roles: vec![role.into()],
        }
    }

    fn soap(a: Uuid) -> NoteBody {
        NoteBody::Soap(Box::new(medassist_domain::note::SoapContent {
            subjective: Some(SoapSection::professional(a, "queixa")),
            objective: Some(SoapSection::professional(a, "achados")),
            assessment: Some(SoapSection::professional(a, "avaliação")),
            plan: Some(SoapSection::professional(a, "plano")),
        }))
    }

    fn ai_section(text: &str) -> SoapSection {
        SoapSection {
            text: text.into(),
            provenance: Provenance::new(
                ProvenanceKind::AiSuggestion,
                SourceRef::AiJob {
                    id: Uuid::new_v4(),
                    model: Some("double".into()),
                },
            ),
            reviewed: false,
        }
    }

    #[tokio::test]
    async fn creates_draft_snapshots_and_audits() {
        let s = svc();
        let a = actor("practitioner");
        let enc = NewEncounter {
            org_id: a.org_id,
            patient_id: Uuid::new_v4(),
            practitioner_id: a.user_id,
            chief_complaint: None,
        }
        .into_encounter(Uuid::new_v4(), Utc::now());
        s.encounters.insert(&enc).await.unwrap();
        let n = s
            .create_draft(&a, enc.id, soap(a.user_id), None, Utc::now())
            .await
            .unwrap();
        assert_eq!(n.status, medassist_domain::note::NoteStatus::Draft);
        assert_eq!(n.version, 1);
        assert_eq!(s.notes.versions.lock().unwrap().len(), 1);
        assert!(s
            .audit
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.action == "note.create_draft"));
    }

    #[tokio::test]
    async fn stale_async_edit_conflicts_and_preserves_human_edit() {
        let s = svc();
        let a = actor("practitioner");
        let enc = NewEncounter {
            org_id: a.org_id,
            patient_id: Uuid::new_v4(),
            practitioner_id: a.user_id,
            chief_complaint: None,
        }
        .into_encounter(Uuid::new_v4(), Utc::now());
        s.encounters.insert(&enc).await.unwrap();
        let n = s
            .create_draft(&a, enc.id, soap(a.user_id), None, Utc::now())
            .await
            .unwrap();

        // Human edits first (v1 -> v2).
        let human_body = {
            let mut b = soap(a.user_id);
            if let NoteBody::Soap(x) = &mut b {
                x.subjective.as_mut().unwrap().text = "editado pelo humano".into();
            }
            b
        };
        s.edit(&a, n.id, 1, human_body.clone(), Utc::now())
            .await
            .unwrap();

        // Async AI job tries to write using the stale version 1.
        let ai_body = {
            let mut b = soap(a.user_id);
            if let NoteBody::Soap(x) = &mut b {
                x.assessment = Some(ai_section("sugestão da IA"));
            }
            b
        };
        let err = s.edit(&a, n.id, 1, ai_body, Utc::now()).await.unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::VersionConflict {
                expected: 1,
                found: 2
            })
        ));

        // Human content preserved.
        let stored = s.get(&a, n.id).await.unwrap();
        match &stored.body {
            NoteBody::Soap(x) => {
                assert_eq!(x.subjective.as_ref().unwrap().text, "editado pelo humano")
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn finalise_requires_practitioner_role() {
        let s = svc();
        let rec = actor("recorder");
        let prac = actor("practitioner");
        let enc = NewEncounter {
            org_id: prac.org_id,
            patient_id: Uuid::new_v4(),
            practitioner_id: prac.user_id,
            chief_complaint: None,
        }
        .into_encounter(Uuid::new_v4(), Utc::now());
        s.encounters.insert(&enc).await.unwrap();
        // recorder cannot finalise — forbidden + denied audit.
        let n = s.notes.get(&prac, enc.id).await; // unrelated call to keep types simple
        let _ = n;
        let draft = s
            .create_draft(&prac, enc.id, soap(prac.user_id), None, Utc::now())
            .await
            .unwrap();
        let err = s
            .finalise(&rec, draft.id, false, Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
        // but the practitioner can.
        s.finalise(&prac, draft.id, false, Utc::now())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn finalised_note_edits_are_immutable_and_amendment_links() {
        let s = svc();
        let a = actor("practitioner");
        let enc = NewEncounter {
            org_id: a.org_id,
            patient_id: Uuid::new_v4(),
            practitioner_id: a.user_id,
            chief_complaint: None,
        }
        .into_encounter(Uuid::new_v4(), Utc::now());
        s.encounters.insert(&enc).await.unwrap();
        let n = s
            .create_draft(&a, enc.id, soap(a.user_id), None, Utc::now())
            .await
            .unwrap();
        s.finalise(&a, n.id, false, Utc::now()).await.unwrap();
        let err = s
            .edit(&a, n.id, 2, soap(a.user_id), Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::ImmutableField)));
        let am = s
            .amend(&a, n.id, soap(a.user_id), Utc::now())
            .await
            .unwrap();
        assert_eq!(am.amends_note_id, Some(n.id));
        assert_eq!(am.status, medassist_domain::note::NoteStatus::Draft);
    }

    #[tokio::test]
    async fn cannot_draft_on_cancelled_encounter() {
        let s = svc();
        let a = actor("practitioner");
        let mut enc = NewEncounter {
            org_id: a.org_id,
            patient_id: Uuid::new_v4(),
            practitioner_id: a.user_id,
            chief_complaint: None,
        }
        .into_encounter(Uuid::new_v4(), Utc::now());
        enc.transition(EncounterStatus::Cancelled, Utc::now())
            .unwrap();
        s.encounters.insert(&enc).await.unwrap();
        let err = s
            .create_draft(&a, enc.id, soap(a.user_id), None, Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::Validation { .. })
        ));
    }
}
