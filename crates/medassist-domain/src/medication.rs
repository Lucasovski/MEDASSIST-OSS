//! Medication statements: what the patient *reports using* vs. what a
//! professional *prescribes* are distinct states — never conflated.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::prov::Provenance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MedicationStatus {
    /// Patient reported taking it (from anamnesis/transcription).
    ReportedByPatient,
    /// Prescribed by a professional during an encounter.
    Prescribed,
    /// Confirmed active medication in the record.
    ActiveConfirmed,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MedicationRoute {
    Oral,
    Intravenous,
    Intramuscular,
    Subcutaneous,
    Topical,
    Inhaled,
    Other,
}

/// Structured dose fields — kept separate so they can be validated and never
/// paraphrased into prose where errors hide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dose {
    pub amount: f64,
    /// Canonical unit string, e.g. "mg", "g", "mL", "UI".
    pub unit: String,
}

impl Dose {
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.amount.is_finite() || self.amount <= 0.0 {
            return Err(DomainError::validation("dose.amount", "must be a positive finite number"));
        }
        if self.unit.trim().is_empty() || self.unit.len() > 24 {
            return Err(DomainError::validation("dose.unit", "must be a non-empty short unit string"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MedicationStatement {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    /// Free-text name as documented/normalised later against a dictionary.
    pub name: String,
    pub status: MedicationStatus,
    pub dose: Option<Dose>,
    pub route: Option<MedicationRoute>,
    /// e.g. "12/12h", "diário" — structured frequency code is future work.
    pub frequency: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub provenance: Provenance,
    pub created_at: DateTime<Utc>,
}

impl MedicationStatement {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.name.trim().is_empty() {
            return Err(DomainError::validation("name", "medication name must not be empty"));
        }
        if self.name.chars().count() > 300 {
            return Err(DomainError::validation("name", "medication name too long"));
        }
        if let Some(d) = &self.dose {
            d.validate()?;
        }
        if let (Some(s), Some(e)) = (self.start_date, self.end_date) {
            if e < s {
                return Err(DomainError::InvalidDateRange { start: s.to_string(), end: e.to_string() });
            }
        }
        // Reported medications must not silently become prescriptions.
        if matches!(self.status, MedicationStatus::Prescribed | MedicationStatus::ActiveConfirmed)
            && !self.provenance.kind.is_confirmed_fact()
        {
            return Err(DomainError::ProvenanceRequired(self.provenance.kind.as_str().to_owned()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prov::{ProvenanceKind, SourceRef};

    fn stmt(status: MedicationStatus, kind: ProvenanceKind) -> MedicationStatement {
        MedicationStatement {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            name: "Losartana 50 mg".into(),
            status,
            dose: Some(Dose { amount: 50.0, unit: "mg".into() }),
            route: Some(MedicationRoute::Oral),
            frequency: Some("12/12h".into()),
            start_date: NaiveDate::from_ymd_opt(2024, 1, 1),
            end_date: None,
            provenance: Provenance::new(kind, SourceRef::Practitioner { user_id: Uuid::new_v4() }),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn prescribed_requires_professional_provenance() {
        let ok = stmt(MedicationStatus::Prescribed, ProvenanceKind::ProfessionalEntry);
        ok.validate().unwrap();
        let bad = stmt(MedicationStatus::Prescribed, ProvenanceKind::AiSuggestion);
        assert!(matches!(bad.validate(), Err(DomainError::ProvenanceRequired(_))));
    }

    #[test]
    fn patient_reported_medication_is_allowed_with_patient_provenance() {
        let s = stmt(MedicationStatus::ReportedByPatient, ProvenanceKind::PatientReport);
        s.validate().unwrap();
    }

    #[test]
    fn rejects_nonpositive_dose_and_bad_dates() {
        let mut s = stmt(MedicationStatus::ActiveConfirmed, ProvenanceKind::ProfessionalEntry);
        s.dose = Some(Dose { amount: -1.0, unit: "mg".into() });
        assert!(s.validate().is_err());
        s.dose = Some(Dose { amount: 50.0, unit: "mg".into() });
        s.end_date = NaiveDate::from_ymd_opt(2023, 1, 1); // before start
        assert!(matches!(s.validate(), Err(DomainError::InvalidDateRange { .. })));
    }
}
