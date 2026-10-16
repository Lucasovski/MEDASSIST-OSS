//! Patient aggregate. Name alone is never an identifier; demographic fields
//! are validated defensively and duplicate detection is advisory only.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::identifier::ExternalIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    Female,
    Male,
    Intersex,
    Unknown,
    Undisclosed,
}

/// A patient record. `org_id` scopes the record to one organisation so that
/// multi-tenant isolation can be enforced at the storage layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patient {
    pub id: Uuid,
    pub org_id: Uuid,
    pub family_name: String,
    pub given_name: String,
    pub birth_date: Option<NaiveDate>,
    pub sex: Option<Sex>,
    /// Free-form contact info (phone/email/address). Treated as sensitive PII.
    pub contact: Option<String>,
    pub external_ids: Vec<ExternalIdentifier>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Input used by use-cases to create a patient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPatient {
    pub org_id: Uuid,
    pub family_name: String,
    pub given_name: String,
    pub birth_date: Option<NaiveDate>,
    pub sex: Option<Sex>,
    pub contact: Option<String>,
    pub external_ids: Vec<ExternalIdentifier>,
    pub notes: Option<String>,
}

impl NewPatient {
    /// Full domain validation. Returns normalised values on success.
    pub fn validate(&self) -> Result<(), DomainError> {
        let fam = self.family_name.trim();
        let giv = self.given_name.trim();
        if fam.is_empty() {
            return Err(DomainError::validation("family_name", "must not be empty"));
        }
        if fam.chars().count() > 200 {
            return Err(DomainError::validation(
                "family_name",
                "must be at most 200 characters",
            ));
        }
        // Given name may legitimately be single-name individuals; still keep
        // *some* identification: require at least one non-space char overall.
        if giv.is_empty() && fam.is_empty() {
            return Err(DomainError::validation("given_name", "patient must have a name"));
        }
        if giv.chars().count() > 200 {
            return Err(DomainError::validation(
                "given_name",
                "must be at most 200 characters",
            ));
        }
        if let Some(bd) = self.birth_date {
            if bd > Utc::now().date_naive() {
                return Err(DomainError::validation(
                    "birth_date",
                    "cannot be in the future",
                ));
            }
            // Guard against implausible ages (>130y) which usually mean typos.
            let now = Utc::now().date_naive();
            if now.year() - bd.year() > 130 {
                return Err(DomainError::validation(
                    "birth_date",
                    "implausible birth year (possible typo)",
                ));
            }
        }
        for eid in &self.external_ids {
            eid.validate()?;
        }
        if let Some(c) = &self.contact {
            if c.chars().count() > 500 {
                return Err(DomainError::validation("contact", "too long (max 500 chars)"));
            }
        }
        Ok(())
    }

    pub fn into_patient(self, id: Uuid, now: DateTime<Utc>) -> Patient {
        Patient {
            id,
            org_id: self.org_id,
            family_name: self.family_name.trim().to_owned(),
            given_name: self.given_name.trim().to_owned(),
            birth_date: self.birth_date,
            sex: self.sex,
            contact: self.contact.map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()),
            external_ids: self.external_ids,
            notes: self.notes.filter(|n| !n.trim().is_empty()),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// Heuristic duplicate-candidate score against an existing patient, in
    /// [0,1]. This NEVER merges records; it only surfaces candidates for human
    /// review. Exact external-id match => strong candidate.
    pub fn duplicate_score_against(&self, other: &Patient) -> f64 {
        let mut score = 0.0f64;
        let keys_new: Vec<String> = self.external_ids.iter().map(|e| e.normalised_key()).collect();
        let keys_old: Vec<String> = other.external_ids.iter().map(|e| e.normalised_key()).collect();
        if keys_new.iter().any(|k| keys_old.contains(k)) {
            score += 0.7;
        }
        let norm = |s: &str| -> String {
            s.trim().to_lowercase().chars().filter(|c| !c.is_whitespace()).collect()
        };
        if !self.given_name.trim().is_empty()
            && norm(&self.given_name) == norm(&other.given_name)
            && norm(&self.family_name) == norm(&other.family_name)
        {
            score += 0.2;
        }
        if let (Some(a), Some(b)) = (self.birth_date, other.birth_date) {
            if a == b {
                score += 0.3;
            }
        }
        score.min(1.0)
    }
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NewPatient {
        NewPatient {
            org_id: Uuid::new_v4(),
            family_name: "Silva".into(),
            given_name: "Maria".into(),
            birth_date: NaiveDate::from_ymd_opt(1980, 5, 10),
            sex: Some(Sex::Female),
            contact: None,
            external_ids: vec![],
            notes: None,
        }
    }

    #[test]
    fn rejects_empty_family_name() {
        let mut p = sample();
        p.family_name = "   ".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_future_birth_date() {
        let mut p = sample();
        p.birth_date = Some(Utc::now().date_naive() + chrono::Duration::days(1));
        assert!(p.validate().is_err());
    }

    #[test]
    fn accepts_valid_patient_and_trims() {
        let mut p = sample();
        p.family_name = "  Silva ".into();
        p.validate().unwrap();
        let now = Utc::now();
        let ent = p.into_patient(Uuid::new_v4(), now);
        assert_eq!(ent.family_name, "Silva");
        assert!(ent.deleted_at.is_none());
    }

    #[test]
    fn duplicate_score_flags_exact_external_id_and_dob() {
        let now = Utc::now();
        let eid = ExternalIdentifier { system: crate::identifier::ExternalIdSystem::Mrn, value: "MRN-1".into(), issuer: None };
        let base = sample();
        let existing = {
            let mut q = sample();
            q.external_ids = vec![eid.clone()];
            q.into_patient(Uuid::new_v4(), now)
        };
        let mut p = sample();
        p.external_ids = vec![eid];
        let score = p.duplicate_score_against(&existing);
        assert!(score >= 0.9, "score was {score}");
    }

    #[test]
    fn duplicate_score_low_for_different_people() {
        let now = Utc::now();
        let other = NewPatient {
            family_name: "Oliveira".into(),
            given_name: "João".into(),
            birth_date: NaiveDate::from_ymd_opt(1995, 1, 1),
            ..sample()
        }
        .into_patient(Uuid::new_v4(), now);
        let score = sample().duplicate_score_against(&other);
        assert!(score < 0.2, "score was {score}");
    }
}
