//! Allergy records with confirmation state and structured severity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::prov::Provenance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllergySeverity {
    Mild,
    Moderate,
    Severe,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllergyConfirmation {
    /// Documented by a professional.
    Confirmed,
    /// Reported but not yet verified.
    Unverified,
    /// Previously recorded, now disputed/removed on review.
    EnteredInError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Allergy {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    /// Substance or class name, e.g. "Dipirona", "Penicilina".
    pub substance: String,
    pub reaction: Option<String>,
    pub severity: AllergySeverity,
    pub confirmation: AllergyConfirmation,
    pub provenance: Provenance,
    pub created_at: DateTime<Utc>,
}

impl Allergy {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.substance.trim().is_empty() {
            return Err(DomainError::validation(
                "substance",
                "allergy substance must not be empty",
            ));
        }
        if self.substance.chars().count() > 200 {
            return Err(DomainError::validation(
                "substance",
                "allergy substance too long",
            ));
        }
        // A confirmed allergy must come from a confirmed source.
        if self.confirmation == AllergyConfirmation::Confirmed
            && !self.provenance.kind.is_confirmed_fact()
        {
            return Err(DomainError::ProvenanceRequired(
                self.provenance.kind.as_str().to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prov::{ProvenanceKind, SourceRef};

    fn a(conf: AllergyConfirmation, kind: ProvenanceKind) -> Allergy {
        Allergy {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            substance: "Dipirona".into(),
            reaction: Some("urticária".into()),
            severity: AllergySeverity::Moderate,
            confirmation: conf,
            provenance: Provenance::new(
                kind,
                SourceRef::Practitioner {
                    user_id: Uuid::new_v4(),
                },
            ),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn unverified_ai_reported_allergy_is_allowed_but_flagged() {
        let x = a(
            AllergyConfirmation::Unverified,
            ProvenanceKind::Transcription,
        );
        x.validate().unwrap();
    }

    #[test]
    fn confirmed_allergy_needs_professional_provenance() {
        let x = a(AllergyConfirmation::Confirmed, ProvenanceKind::AiSuggestion);
        assert!(matches!(
            x.validate(),
            Err(DomainError::ProvenanceRequired(_))
        ));
        let y = a(
            AllergyConfirmation::Confirmed,
            ProvenanceKind::ProfessionalEntry,
        );
        y.validate().unwrap();
    }

    #[test]
    fn empty_substance_rejected() {
        let mut x = a(
            AllergyConfirmation::Unverified,
            ProvenanceKind::PatientReport,
        );
        x.substance = " ".into();
        assert!(x.validate().is_err());
    }
}
