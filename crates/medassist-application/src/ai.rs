//! AI documentation assistance: provider-agnostic contracts plus the
//! anti-hallucination guardrails that turn a raw model answer into a *safe*
//! draft (provenance-tagged, gap-flagged, never auto-finalised).

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use medassist_domain::note::{NoteBody, SoapContent, SoapSection};
use medassist_domain::prov::{Provenance, ProvenanceKind, SourceRef};

use crate::errors::AppError;

/// Input to a documentation-draft job. Only *authorised* context is included;
/// the service layer is responsible for selecting it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationRequest {
    pub job_id: Uuid,
    pub org_id: Uuid,
    pub patient_id: Uuid,
    pub encounter_id: Uuid,
    /// Sections requested by the template.
    pub sections: Vec<String>,
    /// Transcript text (if consented and available).
    pub transcript: Option<String>,
    /// Structured clinical facts already in the record (chief complaint,
    /// vitals as text, prior confirmed problems) — passed as labelled lines.
    pub authorised_context: Vec<String>,
}

/// Raw engine response: section texts only. The engine NEVER returns status,
/// finalisation or permission information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineDraft {
    pub subjective: Option<String>,
    pub objective: Option<String>,
    pub assessment: Option<String>,
    pub plan: Option<String>,
    /// Explicit gaps reported by the engine ("information not available").
    pub flagged_gaps: Vec<String>,
    /// Model identifier as reported by the provider (best effort).
    pub model: Option<String>,
}

/// Low-level language-model port. Implementations: local engines or explicitly
/// configured remote providers. `timeout`/cancellation are enforced by the
/// caller (job worker), engines must be cancel-safe.
#[async_trait]
pub trait LanguageModelProvider: Send + Sync {
    /// Human-readable engine id, e.g. "ollama/qwen2.5:7b" or "test-double".
    fn id(&self) -> String;
    /// Whether this provider sends data OUTSIDE the deployment boundary.
    fn is_external(&self) -> bool;
    async fn health(&self) -> Result<bool, AppError>;
    async fn generate_draft(
        &self,
        req: &DocumentationRequest,
        timeout: Duration,
    ) -> Result<EngineDraft, AppError>;
}

/// High-level clinical documentation engine port (may wrap an LLM plus rules).
#[async_trait]
pub trait ClinicalDocumentationEngine: Send + Sync {
    fn id(&self) -> String;
    async fn generate(&self, req: &DocumentationRequest) -> Result<NoteBody, AppError>;
}

// ---------------------------------------------------------------------------
// Guardrail validation
// ---------------------------------------------------------------------------

/// Numbers (integers/decimals) extracted from generated text, used to check
/// that the model did not invent measurements/doses absent from input.
fn numbers_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let mut num = String::new();
            while i < bytes.len() && (bytes[i].is_ascii_digit() || matches!(bytes[i], '.' | ',')) {
                // don't swallow trailing punctuation like "3." at sentence end
                if (bytes[i] == '.' || bytes[i] == ',')
                    && (i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit())
                {
                    break;
                }
                num.push(bytes[i]);
                i += 1;
            }
            out.push(num.replace(',', "."));
        }
        i += 1;
    }
    out
}

/// Digit-only view of a string — used for grounding checks so that unit or
/// punctuation differences ("130x85" vs "130 mmHg") don't cause false hits.
fn digits_only(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Validate an [`EngineDraft`] against its inputs. This is a *data-provenance*
/// guardrail, NOT a clinical validation: schema/number checks cannot certify
/// medical correctness. It blocks obvious fabrications of quantities and dates
/// and forces every produced section to carry AI provenance + unreviewed flag.
pub fn validate_draft(
    draft: &EngineDraft,
    req: &DocumentationRequest,
) -> Result<NoteBody, Vec<String>> {
    let mut violations = Vec::new();
    let haystack_text: String = {
        let mut h = req.transcript.clone().unwrap_or_default();
        h.push('\n');
        h.push_str(&req.authorised_context.join("\n"));
        h.to_lowercase()
    };
    let haystack_digits = digits_only(&haystack_text);

    let sections = [
        ("subjective", &draft.subjective),
        ("objective", &draft.objective),
        ("assessment", &draft.assessment),
        ("plan", &draft.plan),
    ];

    for (name, content) in sections.iter().filter(|(_, c)| c.as_deref().map(|x| !x.trim().is_empty()).unwrap_or(false)) {
        let text = content.as_ref().unwrap();
        // Every numeric token generated must have been grounded in the input.
        for n in numbers_in(text) {
            let nd = digits_only(&n);
            if !nd.is_empty() && !haystack_digits.contains(&nd) {
                violations.push(format!("{name}: fabricated quantity '{n}' not present in authorised input"));
            }
        }
        // ISO-like or dd/mm/yyyy date shapes must also be grounded.
        for tok in extract_date_like(text) {
            let td = digits_only(&tok);
            if !haystack_digits.contains(&td) {
                violations.push(format!("{name}: fabricated date '{tok}' not present in authorised input"));
            }
        }
    }

    if !violations.is_empty() {
        return Err(violations);
    }

    let job_src = SourceRef::AiJob { id: req.job_id, model: draft.model.clone() };
    let mk = |t: Option<String>| -> Option<SoapSection> {
        t.filter(|x| !x.trim().is_empty()).map(|text| SoapSection {
            text,
            provenance: Provenance::new(ProvenanceKind::AiSuggestion, job_src.clone()),
            reviewed: false, // human review is mandatory before finalisation
        })
    };

    Ok(NoteBody::Soap(Box::new(SoapContent {
        subjective: mk(draft.subjective.clone()),
        objective: mk(draft.objective.clone()),
        assessment: mk(draft.assessment.clone()),
        plan: mk(draft.plan.clone()),
    }))
}

fn extract_date_like(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        let w: String = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '-').into();
        let has_digits = w.chars().any(|c| c.is_ascii_digit());
        let sep = (w.contains('/') || w.contains('-')) && has_digits;
        // crude shape check: contains at least two separators groups
        if sep && w.len() >= 8 {
            out.push(w);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> DocumentationRequest {
        DocumentationRequest {
            job_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            patient_id: Uuid::new_v4(),
            encounter_id: Uuid::new_v4(),
            sections: vec!["subjective".into(), "objective".into(), "assessment".into(), "plan".into()],
            transcript: Some("paciente com cefaleia ha 3 dias. pa 130x85 mmhg".into()),
            authorised_context: vec!["queixa: dor de cabeça".into()],
        }
    }

    #[test]
    fn accepts_draft_groundedin_input() {
        let d = EngineDraft {
            subjective: Some("cefaleia há 3 dias".into()),
            objective: Some("PA 130x85 mmHg".into()),
            assessment: Some("cefaleia tensional relatada".into()),
            plan: Some("retorno em 3 dias".into()),
            flagged_gaps: vec!["exames laboratoriais indisponíveis".into()],
            model: Some("test-double".into()),
        };
        let body = validate_draft(&d, &req()).expect("should pass");
        match body {
            NoteBody::Soap(s) => {
                assert_eq!(s.plan.as_ref().unwrap().provenance.kind, ProvenanceKind::AiSuggestion);
                assert!(!s.plan.as_ref().unwrap().reviewed);
                assert!(!s.unreviewed_sections().is_empty());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn rejects_fabricated_numbers_and_dates() {
        let d = EngineDraft {
            subjective: Some("dor intensa 10/10".into()),
            objective: Some("PA 180x110 mmHg".into()),
            assessment: Some("hipertensão grade 3 desde 2019-05-04".into()),
            plan: Some("losartana 100 mg 2x ao dia".into()),
            flagged_gaps: vec![],
            model: None,
        };
        let errs = validate_draft(&d, &req()).unwrap_err();
        let joined = errs.join(";");
        assert!(joined.contains("fabricated quantity '180'"), "{joined}");
        assert!(joined.contains("fabricated quantity '100'"), "{joined}");
        assert!(joined.contains("fabricated date '2019-05-04'"), "{joined}");
    }

    #[test]
    fn number_tokenizer_handles_decimal_commas() {
        assert_eq!(numbers_in("dose 2,5 mg e PA 130x85."), ["2.5", "130", "85"]);
    }

    #[test]
    fn empty_sections_are_skipped_not_flagged_as_violation() {
        let d = EngineDraft::default();
        let body = validate_draft(&d, &req()).unwrap();
        match body {
            NoteBody::Soap(s) => assert_eq!(s.missing_sections().len(), 4),
            _ => panic!(),
        }
    }
}
