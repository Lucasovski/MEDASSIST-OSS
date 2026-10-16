//! Imported documents and lab-result extraction state. The original file is
//! always preserved; extractions link back to it and stay "pending review"
//! until a professional confirms them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Pdf,
    PlainText,
    Csv,
    Image,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingState {
    Pending,
    Processing,
    Extracted,
    Failed,
    /// Manually reviewed and confirmed by a professional.
    Reviewed,
}

/// Metadata record for an uploaded document (bytes live in blob storage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClinicalDocument {
    pub id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub encounter_id: Option<Uuid>,
    pub kind: DocumentKind,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    /// SHA-256 hex of the original bytes (integrity + dedupe evidence).
    pub sha256: String,
    pub received_at: Option<DateTime<Utc>>,
    pub state: ProcessingState,
    pub error: Option<String>,
    pub uploaded_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Max upload size enforced at domain level (infrastructure must enforce too).
pub const MAX_DOCUMENT_BYTES: u64 = 25 * 1024 * 1024;

impl ClinicalDocument {
    pub fn validate_new(filename: &str, content_type: &str, size_bytes: u64, sha256: &str) -> Result<DocumentKind, DomainError> {
        if filename.trim().is_empty() || filename.len() > 255 {
            return Err(DomainError::validation("filename", "invalid filename length"));
        }
        // Path-traversal defence: filenames must not contain separators.
        if filename.contains('/') || filename.contains('\\') || filename.contains('\0') {
            return Err(DomainError::validation("filename", "must not contain path separators"));
        }
        if size_bytes == 0 {
            return Err(DomainError::validation("size", "empty file"));
        }
        if size_bytes > MAX_DOCUMENT_BYTES {
            return Err(DomainError::validation("size", "file exceeds 25 MiB limit"));
        }
        if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(DomainError::validation("sha256", "must be 64 hex chars"));
        }
        let kind = match content_type {
            "application/pdf" => DocumentKind::Pdf,
            "text/plain" => DocumentKind::PlainText,
            "text/csv" => DocumentKind::Csv,
            "image/png" | "image/jpeg" | "image/webp" => DocumentKind::Image,
            other => {
                return Err(DomainError::validation(
                    "content_type",
                    format!("unsupported media type '{other}'"),
                ))
            }
        };
        Ok(kind)
    }
}

/// A single structured lab result extracted from a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabResult {
    pub id: Uuid,
    pub document_id: Uuid,
    pub patient_id: Uuid,
    /// e.g. "Hemoglobina glicada (HbA1c)"
    pub name: String,
    pub value: String,
    pub unit: Option<String>,
    /// Reference interval as printed by the lab, kept verbatim.
    pub reference_range: Option<String>,
    pub collected_at: Option<DateTime<Utc>>,
    pub laboratory: Option<String>,
    pub method: Option<String>,
    pub reviewed: bool,
}

impl LabResult {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.name.trim().is_empty() {
            return Err(DomainError::validation("name", "lab result name required"));
        }
        if self.value.trim().is_empty() {
            return Err(DomainError::validation("value", "lab result value required"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_types() {
        assert_eq!(
            ClinicalDocument::validate_new("laudo.pdf", "application/pdf", 1000, &"a".repeat(64)).unwrap(),
            DocumentKind::Pdf
        );
        assert_eq!(
            ClinicalDocument::validate_new("gluc.csv", "text/csv", 10, &"b".repeat(64)).unwrap(),
            DocumentKind::Csv
        );
    }

    #[test]
    fn rejects_unsafe_or_oversized_uploads() {
        let hash = "c".repeat(64);
        assert!(ClinicalDocument::validate_new("../../etc/passwd", "text/plain", 10, &hash).is_err());
        assert!(ClinicalDocument::validate_new("ok.txt", "text/plain", 0, &hash).is_err());
        assert!(ClinicalDocument::validate_new("big.txt", "text/plain", MAX_DOCUMENT_BYTES + 1, &hash).is_err());
        assert!(ClinicalDocument::validate_new("x.exe", "application/octet-stream", 10, &hash).is_err());
        assert!(ClinicalDocument::validate_new("ok.txt", "text/plain", 10, "nothash").is_err());
    }

    #[test]
    fn lab_result_requires_name_value() {
        let r = LabResult {
            id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            name: "".into(),
            value: "12".into(),
            unit: None,
            reference_range: None,
            collected_at: None,
            laboratory: None,
            method: None,
            reviewed: false,
        };
        assert!(r.validate().is_err());
    }
}
