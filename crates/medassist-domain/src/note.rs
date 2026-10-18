//! Clinical notes with SOAP structure, draft lifecycle, versioning and
//! conflict detection. An AI suggestion never becomes a final record by itself:
//! finalisation is an explicit professional action on a reviewed note.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::prov::Provenance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteStatus {
    Draft,
    InReview,
    Finalised,
    /// Correction of a finalised note (amendment/retificação).
    Amended,
}

impl NoteStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NoteStatus::Draft => "draft",
            NoteStatus::InReview => "in_review",
            NoteStatus::Finalised => "finalised",
            NoteStatus::Amended => "amended",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "draft" => Self::Draft,
            "in_review" => Self::InReview,
            "finalised" => Self::Finalised,
            "amended" => Self::Amended,
            _ => return None,
        })
    }
}

/// A single SOAP section body with provenance metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoapSection {
    /// Markdown/plain-text content.
    pub text: String,
    pub provenance: Provenance,
    /// True once a human has explicitly reviewed this section.
    pub reviewed: bool,
}

impl SoapSection {
    pub fn professional(user_id: Uuid, text: impl Into<String>) -> Self {
        Self { text: text.into(), provenance: Provenance::professional(user_id), reviewed: true }
    }
}

/// Structured SOAP content for a note. Sections are optional but tracked so
/// the UI can flag what is missing (never auto-filled with invented data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SoapContent {
    pub subjective: Option<SoapSection>,
    pub objective: Option<SoapSection>,
    pub assessment: Option<SoapSection>,
    pub plan: Option<SoapSection>,
}

impl SoapContent {
    /// Names of sections that are still absent — used to signal gaps.
    pub fn missing_sections(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.subjective.is_none() { v.push("subjective"); }
        if self.objective.is_none() { v.push("objective"); }
        if self.assessment.is_none() { v.push("assessment"); }
        if self.plan.is_none() { v.push("plan"); }
        v
    }

    /// Sections that contain unreviewed non-professional content (AI drafts,
    /// transcriptions, imported extractions). These must be surfaced in the UI.
    pub fn unreviewed_sections(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (name, sec) in [
            ("subjective", &self.subjective),
            ("objective", &self.objective),
            ("assessment", &self.assessment),
            ("plan", &self.plan),
        ] {
            if let Some(s) = sec {
                if !s.reviewed {
                    v.push(name);
                }
            }
        }
        v
    }

    pub fn is_empty(&self) -> bool {
        self.subjective.is_none()
            && self.objective.is_none()
            && self.assessment.is_none()
            && self.plan.is_none()
    }
}

/// Either structured SOAP or free text; both carry provenance at section level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteBody {
    Soap(Box<SoapContent>),
    FreeText { text: String, provenance: Provenance, reviewed: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub org_id: Uuid,
    pub encounter_id: Uuid,
    pub patient_id: Uuid,
    pub author_id: Uuid,
    pub template_id: Option<Uuid>,
    pub status: NoteStatus,
    /// Monotonic version, incremented on every persisted edit.
    pub version: u32,
    pub body: NoteBody,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finalised_at: Option<DateTime<Utc>>,
    pub finalised_by: Option<Uuid>,
    /// If this note amends a previously finalised note, points to it.
    pub amends_note_id: Option<Uuid>,
}

impl Note {
    pub fn new_draft(
        id: Uuid,
        org_id: Uuid,
        encounter_id: Uuid,
        patient_id: Uuid,
        author_id: Uuid,
        body: NoteBody,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            org_id,
            encounter_id,
            patient_id,
            author_id,
            template_id: None,
            status: NoteStatus::Draft,
            version: 1,
            body,
            created_at: now,
            updated_at: now,
            finalised_at: None,
            finalised_by: None,
            amends_note_id: None,
        }
    }

    /// Apply an edit with optimistic concurrency control. `expected_version`
    /// comes from the client; mismatches raise [`DomainError::VersionConflict`]
    /// so human edits are never silently overwritten by async jobs.
    pub fn apply_edit(
        &mut self,
        expected_version: u32,
        new_body: NoteBody,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if matches!(self.status, NoteStatus::Finalised | NoteStatus::Amended) {
            return Err(DomainError::ImmutableField);
        }
        if expected_version != self.version {
            return Err(DomainError::VersionConflict { expected: expected_version, found: self.version });
        }
        self.body = new_body;
        self.version += 1;
        self.updated_at = now;
        Ok(())
    }

    /// Mark a section as reviewed by the professional.
    pub fn mark_section_reviewed(&mut self, section: &str, now: DateTime<Utc>) -> Result<(), DomainError> {
        if matches!(self.status, NoteStatus::Finalised | NoteStatus::Amended) {
            return Err(DomainError::ImmutableField);
        }
        let found = match &mut self.body {
            NoteBody::Soap(s) => match section {
                "subjective" => s.subjective.as_mut().map(|x| {
                    x.reviewed = true;
                }),
                "objective" => s.objective.as_mut().map(|x| {
                    x.reviewed = true;
                }),
                "assessment" => s.assessment.as_mut().map(|x| {
                    x.reviewed = true;
                }),
                "plan" => s.plan.as_mut().map(|x| {
                    x.reviewed = true;
                }),
                _ => None,
            },
            NoteBody::FreeText { reviewed, .. } if section == "text" => {
                *reviewed = true;
                Some(())
            }
            _ => None,
        };
        match found {
            Some(()) => {
                self.version += 1;
                self.updated_at = now;
                Ok(())
            }
            None => Err(DomainError::NotFound(format!("section '{section}'"))),
        }
    }

    /// Explicit professional finalisation. Refuses while any section is
    /// unreviewed or required SOAP sections are missing — the clinician must
    /// pass `override_gaps` only after seeing the warnings (recorded in audit).
    pub fn finalise(
        &mut self,
        by: Uuid,
        now: DateTime<Utc>,
        override_gaps: bool,
    ) -> Result<(), DomainError> {
        if self.status != NoteStatus::Draft && self.status != NoteStatus::InReview {
            return Err(DomainError::IllegalTransition {
                from: self.status.as_str().to_owned(),
                to: "finalised".into(),
            });
        }
        let mut problems: Vec<String> = Vec::new();
        match &self.body {
            NoteBody::Soap(s) => {
                for m in s.missing_sections() {
                    problems.push(format!("missing section: {m}"));
                }
                for u in s.unreviewed_sections() {
                    problems.push(format!("unreviewed section: {u}"));
                }
                if s.is_empty() {
                    problems.push("empty note".into());
                }
            }
            NoteBody::FreeText { text, reviewed, .. } => {
                if text.trim().is_empty() {
                    problems.push("empty note".into());
                }
                if !reviewed {
                    problems.push("content not reviewed".into());
                }
            }
        }
        if !problems.is_empty() && !override_gaps {
            return Err(DomainError::validation(
                "note",
                format!("cannot finalise: {}", problems.join("; ")),
            ));
        }
        self.status = NoteStatus::Finalised;
        self.finalised_at = Some(now);
        self.finalised_by = Some(by);
        self.version += 1;
        self.updated_at = now;
        Ok(())
    }

    /// A finalised note is immutable; corrections create an amendment note.
    pub fn create_amendment(&self, new_id: Uuid, body: NoteBody, now: DateTime<Utc>) -> Result<Note, DomainError> {
        if self.status != NoteStatus::Finalised {
            return Err(DomainError::IllegalTransition {
                from: self.status.as_str().to_owned(),
                to: "amended-source".into(),
            });
        }
        let mut n = Note::new_draft(new_id, self.org_id, self.encounter_id, self.patient_id, self.author_id, body, now);
        n.amends_note_id = Some(self.id);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prov::{ProvenanceKind, SourceRef};

    fn soap_with_ai_assessment(author: Uuid) -> NoteBody {
        NoteBody::Soap(Box::new(SoapContent {
            subjective: Some(SoapSection::professional(author, "cefaleia há 3 dias")),
            objective: Some(SoapSection::professional(author, "PA 130x85, FC 78")),
            assessment: Some(SoapSection {
                text: "provável enxaqueca".into(),
                provenance: Provenance::new(
                    ProvenanceKind::AiSuggestion,
                    SourceRef::AiJob { id: Uuid::new_v4(), model: Some("test-model".into()) },
                ),
                reviewed: false,
            }),
            plan: None,
        }))
    }

    fn mk_note(author: Uuid) -> Note {
        Note::new_draft(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), author, soap_with_ai_assessment(author), Utc::now())
    }

    #[test]
    fn gap_detection_lists_missing_and_unreviewed() {
        let author = Uuid::new_v4();
        let n = mk_note(author);
        match &n.body {
            NoteBody::Soap(s) => {
                assert_eq!(s.missing_sections(), vec!["plan"]);
                assert_eq!(s.unreviewed_sections(), vec!["assessment"]);
            }
            _ => panic!("expected soap"),
        }
    }

    #[test]
    fn finalise_blocked_until_reviewed_and_complete() {
        let author = Uuid::new_v4();
        let mut n = mk_note(author);
        let err = n.finalise(author, Utc::now(), false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing section: plan"), "{msg}");
        assert!(msg.contains("unreviewed section: assessment"), "{msg}");

        // Fill plan + review assessment then it succeeds.
        if let NoteBody::Soap(s) = &mut n.body {
            s.plan = Some(SoapSection::professional(author, "analgésico e retorno em 7 dias"));
        }
        n.mark_section_reviewed("assessment", Utc::now()).unwrap();
        n.finalise(author, Utc::now(), false).unwrap();
        assert_eq!(n.status, NoteStatus::Finalised);
    }

    #[test]
    fn version_conflict_prevents_silent_overwrite() {
        let author = Uuid::new_v4();
        let mut n = mk_note(author);
        let v = n.version;
        // Stale client (e.g. an async AI job that started before a human edit).
        let stale = v - 1;
        let err = n.apply_edit(stale, soap_with_ai_assessment(author), Utc::now()).unwrap_err();
        assert!(matches!(err, DomainError::VersionConflict { .. }));
        // Correct version works and bumps.
        n.apply_edit(v, soap_with_ai_assessment(author), Utc::now()).unwrap();
        assert_eq!(n.version, v + 1);
    }

    #[test]
    fn finalised_note_is_immutable_and_amendment_links_back() {
        let author = Uuid::new_v4();
        let mut n = mk_note(author);
        if let NoteBody::Soap(s) = &mut n.body {
            s.plan = Some(SoapSection::professional(author, "plano"));
            s.assessment.as_mut().unwrap().reviewed = true;
        }
        n.finalise(author, Utc::now(), false).unwrap();
        let err = n.apply_edit(n.version, n.body.clone(), Utc::now()).unwrap_err();
        assert!(matches!(err, DomainError::ImmutableField));

        let amended = n.create_amendment(Uuid::new_v4(), soap_with_ai_assessment(author), Utc::now()).unwrap();
        assert_eq!(amended.amends_note_id, Some(n.id));
        assert_eq!(amended.status, NoteStatus::Draft);
    }

    #[test]
    fn ai_only_note_cannot_be_finalised_without_override_even_when_reviewed_flag_absent() {
        let author = Uuid::new_v4();
        let body = NoteBody::Soap(Box::new(SoapContent {
            subjective: Some(SoapSection {
                text: "relato do paciente".into(),
                provenance: Provenance::new(ProvenanceKind::Transcription, SourceRef::Transcription { id: Uuid::new_v4(), start_ms: None, end_ms: None }),
                reviewed: false,
            }),
            ..Default::default()
        }));
        let mut n = Note::new_draft(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), author, body, Utc::now());
        assert!(n.finalise(author, Utc::now(), false).is_err());
        // Explicit override is allowed (and would be audited at app layer).
        n.finalise(author, Utc::now(), true).unwrap();
    }
}
