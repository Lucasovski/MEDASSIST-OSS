//! Stable internal identifiers and external identifier handling.
//!
//! Safety rule: a patient's *name is never* used as a unique identifier.
//! Internal identity is a stable UUID minted at creation time; external
//! identifiers (MRN, SUS card, insurance ids) are modelled separately and may
//! be ambiguous — duplicate detection only *flags candidates*, it never merges
//! patients automatically.

use crate::DomainError;
use serde::{Deserialize, Serialize};

/// Newtype for the stable internal patient identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatientId(pub uuid::Uuid);

impl PatientId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for PatientId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIdSystem {
    /// Brazilian public health system card number (Cartão Nacional de Saúde).
    Cns,
    /// Local medical record number assigned by the clinic.
    Mrn,
    /// Insurance / health plan member id.
    HealthPlan,
    Passport,
    Other,
}

/// An external identifier attached to a patient. Not guaranteed unique.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIdentifier {
    pub system: ExternalIdSystem,
    pub value: String,
    /// Optional issuer/organisation scope for systems like MRN.
    pub issuer: Option<String>,
}

impl ExternalIdentifier {
    pub fn validate(&self) -> Result<(), DomainError> {
        let trimmed = self.value.trim();
        if trimmed.is_empty() {
            return Err(DomainError::validation("external_id.value", "must not be empty"));
        }
        if trimmed.len() > 128 {
            return Err(DomainError::validation(
                "external_id.value",
                "must be at most 128 characters",
            ));
        }
        match self.system {
            // CNS format: 16 digits, first digit 1..9 (simplified structural check).
            ExternalIdSystem::Cns => {
                let digits: Vec<char> = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() != 16 || !digits[0].to_digit(10).map(|d| (1..=9).contains(&d)).unwrap_or(false) {
                    return Err(DomainError::validation(
                        "external_id.value",
                        "CNS must have 16 digits starting with 1-9",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Normalised comparison key used ONLY to surface duplicate candidates.
    pub fn normalised_key(&self) -> String {
        format!(
            "{:?}:{}",
            self.system,
            self.value.trim().to_uppercase().replace([' ', '-', '.'], "")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_blank_external_id() {
        let id = ExternalIdentifier { system: ExternalIdSystem::Mrn, value: "   ".into(), issuer: None };
        assert!(id.validate().is_err());
    }

    #[test]
    fn validates_cns_shape() {
        let good = ExternalIdentifier {
            system: ExternalIdSystem::Cns,
            value: "789 1234567890 1234".replace(' ', ""),
            issuer: None,
        };
        // 19 chars? no: 1 + 10 + 4 = 15 digits -> invalid on purpose, test both:
        assert!(good.validate().is_err());

        let ok = ExternalIdentifier {
            system: ExternalIdSystem::Cns,
            value: "7123456789012345".into(),
            issuer: None,
        };
        assert!(ok.validate().is_ok());

        let bad_start = ExternalIdentifier {
            system: ExternalIdSystem::Cns,
            value: "0123456789012345".into(),
            issuer: None,
        };
        assert!(bad_start.validate().is_err());
    }

    #[test]
    fn normalisation_ignores_separators_and_case() {
        let a = ExternalIdentifier { system: ExternalIdSystem::HealthPlan, value: "ab-123 x".into(), issuer: None };
        let b = ExternalIdentifier { system: ExternalIdSystem::HealthPlan, value: "AB123X ".into(), issuer: None };
        assert_eq!(a.normalised_key(), b.normalised_key());
    }
}
