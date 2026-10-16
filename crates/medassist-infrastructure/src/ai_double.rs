//! A TEST-ONLY deterministic language-model double. It is NOT an AI engine:
//! it echoes the authorised input verbatim into SOAP sections so the pipeline
//! (jobs, grounding validation, review flow) can be integration-tested
//! without any real model. Never present it as a functioning assistant.
//!
//! License: GPL-3.0-or-later.

use std::time::Duration;

use async_trait::async_trait;

use medassist_application::ai::{
    ClinicalDocumentationEngine, DocumentationRequest, EngineDraft, LanguageModelProvider,
};
use medassist_application::errors::AppError;
use medassist_domain::note::{NoteBody, SoapContent, SoapSection};
use medassist_domain::prov::Provenance;

pub struct DoubleLlm {
    /// When true, every call fails — used to exercise error paths.
    pub fail: std::sync::atomic::AtomicBool,
}

impl Default for DoubleLlm {
    fn default() -> Self {
        Self {
            fail: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl DoubleLlm {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_fail(&self, fail: bool) {
        self.fail.store(fail, std::sync::atomic::Ordering::SeqCst);
    }
    fn failing(&self) -> bool {
        self.fail.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl LanguageModelProvider for DoubleLlm {
    fn id(&self) -> String {
        "test-double-echo".into()
    }
    fn is_external(&self) -> bool {
        false
    }
    async fn health(&self) -> Result<bool, AppError> {
        Ok(!self.failing())
    }
    async fn generate_draft(
        &self,
        req: &DocumentationRequest,
        _timeout: Duration,
    ) -> Result<EngineDraft, AppError> {
        if self.failing() {
            return Err(AppError::ExternalService(
                "double-llm configured to fail".into(),
            ));
        }
        // Deterministic echo: only content present in the authorised inputs is
        // reproduced, so grounding validation passes for genuine data and the
        // fabricated-content tests remain meaningful.
        let transcript = req.transcript.clone().unwrap_or_default();
        let ctx = req.authorised_context.join("\n");
        let mut draft = EngineDraft {
            subjective: None,
            objective: None,
            assessment: None,
            plan: None,
            flagged_gaps: Vec::new(),
            model: Some(self.id()),
        };
        for want in &req.sections {
            match want.as_str() {
                "subjective" => draft.subjective = Some(transcript.clone()),
                "objective" => draft.objective = Some(ctx.clone()),
                "assessment" => draft.assessment = Some(format!("Contexto documentado: {ctx}")),
                "plan" => draft.plan = Some("Conforme contexto autorizado.".into()),
                other => draft.flagged_gaps.push(other.to_string()),
            }
        }
        Ok(draft)
    }
}

/// Engine wrapper over the double producing an unreviewed AI-provenance body.
pub struct DoubleEngine(pub DoubleLlm);

#[async_trait]
impl ClinicalDocumentationEngine for DoubleEngine {
    fn id(&self) -> String {
        "test-double-engine".into()
    }
    async fn generate(&self, req: &DocumentationRequest) -> Result<NoteBody, AppError> {
        let d = self.0.generate_draft(req, Duration::from_secs(5)).await?;
        let mk = |t: Option<String>| -> Option<SoapSection> {
            t.map(|text| SoapSection {
                text,
                provenance: Provenance::new(
                    medassist_domain::prov::ProvenanceKind::AiSuggestion,
                    medassist_domain::prov::SourceRef::AiJob {
                        id: req.job_id,
                        model: Some(self.id()),
                    },
                ),
                reviewed: false,
            })
        };
        Ok(NoteBody::Soap(Box::new(SoapContent {
            subjective: mk(d.subjective),
            objective: mk(d.objective),
            assessment: mk(d.assessment),
            plan: mk(d.plan),
        })))
    }
}
