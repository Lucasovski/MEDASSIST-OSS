//! Transcription domain model: consent, segments with timestamps, review.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionState {
    /// Audio captured, awaiting engine processing.
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// Engine confidence in [0,1] when the engine provides it (optional —
    /// never fabricated).
    pub confidence: Option<f32>,
    /// Speaker label only if the engine really supports diarisation.
    pub speaker: Option<String>,
}

impl TranscriptSegment {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.end_ms < self.start_ms {
            return Err(DomainError::InvalidDateRange {
                start: self.start_ms.to_string(),
                end: self.end_ms.to_string(),
            });
        }
        if let Some(c) = self.confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err(DomainError::validation(
                    "confidence",
                    "must be within [0,1]",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcription {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub encounter_id: Option<Uuid>,
    pub state: TranscriptionState,
    pub engine: String,
    pub engine_version: Option<String>,
    pub language: Option<String>,
    pub segments: Vec<TranscriptSegment>,
    /// True after a human corrected/approved the text.
    pub reviewed: bool,
    /// Id of the recorded consent covering this audio capture.
    pub consent_id: Uuid,
    pub audio_retention_policy: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Transcription {
    /// Concatenated full text (segments joined by space).
    pub fn full_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.trim())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        for seg in &self.segments {
            seg.validate()?;
        }
        Ok(())
    }
}

/// Explicit consent record for audio recording/transcription. Consent is
/// captured before any capture begins; recording without a valid consent id
/// must be rejected at the application layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingConsent {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub granted_by: String,
    pub scope: String,
    pub retention_policy: String,
    pub granted_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl RecordingConsent {
    pub fn is_valid_at(&self, now: DateTime<Utc>) -> bool {
        self.granted_at <= now && self.revoked_at.map(|r| r > now).unwrap_or(true)
    }
}

/// Generic consent entity used for other purposes too.
pub type Consent = RecordingConsent;

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: u64, end_ms: u64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
            confidence: Some(0.9),
            speaker: None,
        }
    }

    #[test]
    fn segment_time_order_enforced() {
        assert!(seg(1000, 500, "x").validate().is_err());
        assert!(seg(0, 1000, "ok").validate().is_ok());
    }

    #[test]
    fn confidence_bounds() {
        let mut s = seg(0, 10, "x");
        s.confidence = Some(1.5);
        assert!(s.validate().is_err());
    }

    #[test]
    fn consent_validity_window() {
        let now = Utc::now();
        let c = RecordingConsent {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            granted_by: "paciente".into(),
            scope: "audio da consulta".into(),
            retention_policy: "delete_after_note_finalisation".into(),
            granted_at: now - chrono::Duration::days(1),
            revoked_at: None,
        };
        assert!(c.is_valid_at(now));
        let revoked = RecordingConsent {
            revoked_at: Some(now - chrono::Duration::hours(1)),
            ..c.clone()
        };
        assert!(!revoked.is_valid_at(now));
    }

    #[test]
    fn full_text_joins_segments() {
        let t = Transcription {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            encounter_id: None,
            state: TranscriptionState::Completed,
            engine: "test-double".into(),
            engine_version: None,
            language: Some("pt-BR".into()),
            segments: vec![
                seg(0, 1000, "bom dia"),
                seg(1000, 2000, "como o senhor está?"),
            ],
            reviewed: false,
            consent_id: Uuid::new_v4(),
            audio_retention_policy: "delete_after_note_finalisation".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(t.full_text(), "bom dia como o senhor está?");
        t.validate().unwrap();
    }
}
