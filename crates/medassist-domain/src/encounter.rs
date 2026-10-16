//! Clinical encounter (visit) aggregate with lifecycle states.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterStatus {
    /// Scheduled / opened but not started.
    Open,
    /// In progress.
    InProgress,
    /// Clinician has closed the encounter; documentation may still be drafted.
    Closed,
    /// Fully reviewed and signed off.
    Finalised,
    /// Cancelled without care being delivered.
    Cancelled,
}

impl EncounterStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EncounterStatus::Open => "open",
            EncounterStatus::InProgress => "in_progress",
            EncounterStatus::Closed => "closed",
            EncounterStatus::Finalised => "finalised",
            EncounterStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "in_progress" => Self::InProgress,
            "closed" => Self::Closed,
            "finalised" => Self::Finalised,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    /// Allowed transitions. A cancelled or finalised encounter is terminal.
    pub fn can_transition_to(&self, to: &Self) -> bool {
        use EncounterStatus::*;
        matches!(
            (*self, to),
            (Open, InProgress)
                | (Open, Cancelled)
                | (InProgress, Closed)
                | (InProgress, Cancelled)
                | (Closed, Finalised)
                | (Closed, InProgress) // reopen for correction before sign-off
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encounter {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub practitioner_id: Uuid,
    pub status: EncounterStatus,
    /// Chief complaint as reported (subjective). Optional at creation.
    pub chief_complaint: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEncounter {
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub practitioner_id: Uuid,
    pub chief_complaint: Option<String>,
}

impl NewEncounter {
    pub fn validate(&self) -> Result<(), DomainError> {
        if let Some(cc) = &self.chief_complaint {
            if cc.chars().count() > 2000 {
                return Err(DomainError::validation(
                    "chief_complaint",
                    "must be at most 2000 characters",
                ));
            }
        }
        Ok(())
    }

    pub fn into_encounter(self, id: Uuid, now: DateTime<Utc>) -> Encounter {
        Encounter {
            id,
            org_id: self.org_id,
            patient_id: self.patient_id,
            practitioner_id: self.practitioner_id,
            status: EncounterStatus::Open,
            chief_complaint: self.chief_complaint.filter(|c| !c.trim().is_empty()),
            started_at: None,
            ended_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Encounter {
    pub fn transition(
        &mut self,
        to: EncounterStatus,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if !self.status.can_transition_to(&to) {
            return Err(DomainError::IllegalTransition {
                from: self.status.as_str().to_owned(),
                to: to.as_str().to_owned(),
            });
        }
        self.status = to;
        self.updated_at = now;
        match to {
            EncounterStatus::InProgress if self.started_at.is_none() => self.started_at = Some(now),
            EncounterStatus::Closed | EncounterStatus::Cancelled => self.ended_at = Some(now),
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc() -> Encounter {
        NewEncounter {
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            practitioner_id: Uuid::new_v4(),
            chief_complaint: Some("dor de cabeça".into()),
        }
        .into_encounter(Uuid::new_v4(), Utc::now())
    }

    #[test]
    fn happy_path_transitions() {
        let mut e = enc();
        let now = Utc::now();
        e.transition(EncounterStatus::InProgress, now).unwrap();
        assert_eq!(e.started_at, Some(now));
        e.transition(EncounterStatus::Closed, now).unwrap();
        assert!(e.ended_at.is_some());
        e.transition(EncounterStatus::Finalised, now).unwrap();
        assert_eq!(e.status, EncounterStatus::Finalised);
    }

    #[test]
    fn cannot_finalise_from_open() {
        let mut e = enc();
        let err = e
            .transition(EncounterStatus::Finalised, Utc::now())
            .unwrap_err();
        assert!(matches!(err, DomainError::IllegalTransition { .. }));
    }

    #[test]
    fn finalised_is_terminal() {
        let mut e = enc();
        let now = Utc::now();
        e.transition(EncounterStatus::InProgress, now).unwrap();
        e.transition(EncounterStatus::Closed, now).unwrap();
        e.transition(EncounterStatus::Finalised, now).unwrap();
        assert!(e.transition(EncounterStatus::InProgress, now).is_err());
        assert!(e.transition(EncounterStatus::Cancelled, now).is_err());
    }

    #[test]
    fn rejects_overlong_chief_complaint() {
        let n = NewEncounter {
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            practitioner_id: Uuid::new_v4(),
            chief_complaint: Some("x".repeat(2001)),
        };
        assert!(n.validate().is_err());
    }
}
