//! Vital signs as structured observations with unit-safe numeric values.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::prov::Provenance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VitalSignType {
    SystolicBp,
    DiastolicBp,
    HeartRate,
    TemperatureCelsius,
    RespiratoryRate,
    SpO2,
    WeightKg,
    HeightCm,
    GlucoseMgL,
}

impl VitalSignType {
    /// Canonical unit expected for each type. Values must be stored in the
    /// canonical unit — no implicit conversions are performed anywhere.
    pub fn canonical_unit(&self) -> &'static str {
        match self {
            VitalSignType::SystolicBp | VitalSignType::DiastolicBp | VitalSignType::GlucoseMgL => "mmHg",
            VitalSignType::HeartRate | VitalSignType::RespiratoryRate => "bpm",
            VitalSignType::TemperatureCelsius => "°C",
            VitalSignType::SpO2 => "%",
            VitalSignType::WeightKg => "kg",
            VitalSignType::HeightCm => "cm",
        }
    }

    /// Plausible physiologic range used only to catch data-entry errors —
    /// NOT a clinical judgement about the patient.
    pub fn plausibility_range(&self) -> (f64, f64) {
        match self {
            VitalSignType::SystolicBp => (50.0, 300.0),
            VitalSignType::DiastolicBp => (20.0, 200.0),
            VitalSignType::HeartRate => (20.0, 300.0),
            VitalSignType::TemperatureCelsius => (25.0, 45.0),
            VitalSignType::RespiratoryRate => (4.0, 80.0),
            VitalSignType::SpO2 => (30.0, 100.0),
            VitalSignType::WeightKg => (0.3, 500.0),
            VitalSignType::HeightCm => (25.0, 260.0),
            VitalSignType::GlucoseMgL => (10.0, 1000.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VitalSign {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub encounter_id: Option<Uuid>,
    pub kind: VitalSignType,
    pub value: f64,
    /// Must equal the canonical unit for `kind` (enforced by validation).
    pub unit: String,
    pub measured_at: DateTime<Utc>,
    pub provenance: Provenance,
}

impl VitalSign {
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.value.is_finite() {
            return Err(DomainError::validation("value", "must be finite"));
        }
        if self.unit != self.kind.canonical_unit() {
            return Err(DomainError::validation(
                "unit",
                format!(
                    "expected canonical unit '{}' for {:?}; convert explicitly before storing",
                    self.kind.canonical_unit(),
                    self.kind
                ),
            ));
        }
        let (lo, hi) = self.kind.plausibility_range();
        if self.value < lo || self.value > hi {
            return Err(DomainError::validation(
                "value",
                format!("out of plausible range [{lo}, {hi}] for {:?}", self.kind),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prov::{ProvenanceKind, SourceRef};

    fn vs(kind: VitalSignType, value: f64, unit: &str) -> VitalSign {
        VitalSign {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            encounter_id: None,
            kind,
            value,
            unit: unit.into(),
            measured_at: Utc::now(),
            provenance: Provenance::new(ProvenanceKind::ObjectiveMeasurement, SourceRef::Practitioner { user_id: Uuid::new_v4() }),
        }
    }

    #[test]
    fn accepts_plausible_measurement() {
        vs(VitalSignType::SystolicBp, 130.0, "mmHg").validate().unwrap();
        vs(VitalSignType::TemperatureCelsius, 36.5, "°C").validate().unwrap();
    }

    #[test]
    fn rejects_wrong_unit_no_implicit_conversion() {
        // Fahrenheit entered where Celsius is canonical -> rejected.
        let v = vs(VitalSignType::TemperatureCelsius, 98.6, "°F");
        assert!(matches!(v.validate(), Err(DomainError::Validation { .. })));
    }

    #[test]
    fn rejects_implausible_values_and_nan() {
        assert!(vs(VitalSignType::HeartRate, 500.0, "bpm").validate().is_err());
        assert!(vs(VitalSignType::SpO2, f64::NAN, "%").validate().is_err());
    }
}
