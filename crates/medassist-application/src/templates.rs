//! Template listing use-case. Built-in SOAP template always exists; org
//! templates come from storage. Rendering/validation rules live here so the
//! API stays thin.

use medassist_domain::note::SoapContent;

use crate::errors::{AppError, Actor};
use crate::ports::{NoteTemplate, TemplateRepository};

pub const BUILTIN_SOAP_ID: &str = "00000000-0000-4000-8000-000000000001";

pub fn builtin_templates() -> Vec<NoteTemplate> {
    let uuid = |s: &str| uuid::Uuid::parse_str(s).expect("valid built-in uuid");
    vec![
        NoteTemplate {
            id: uuid(BUILTIN_SOAP_ID),
            org_id: None,
            name: "SOAP".into(),
            version: 1,
            sections: SoapSections::ALL.iter().map(|s| s.to_string()).collect(),
            required_sections: vec!["subjective".into()],
        },
        NoteTemplate {
            id: uuid("00000000-0000-4000-8000-000000000002"),
            org_id: None,
            name: "SOAP completo (todas as seções obrigatórias)".into(),
            version: 1,
            sections: SoapSections::ALL.iter().map(|s| s.to_string()).collect(),
            required_sections: SoapSections::ALL.iter().map(|s| s.to_string()).collect(),
        },
    ]
}

/// Canonical SOAP section names.
pub struct SoapSections;
impl SoapSections {
    pub const ALL: [&'static str; 4] = ["subjective", "objective", "assessment", "plan"];
}

pub struct TemplateService<R: TemplateRepository> {
    pub repo: R,
}

impl<R: TemplateRepository> TemplateService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn list(&self, actor: &Actor) -> Result<Vec<NoteTemplate>, AppError> {
        let mut all = builtin_templates();
        all.extend(self.repo.list(actor).await?);
        Ok(all)
    }

    /// Which required sections are still missing from a draft body — used by
    /// the editor to show gaps without inventing content.
    pub fn missing_required(template: &NoteTemplate, body: &medassist_domain::note::NoteBody) -> Vec<String> {
        let present = match body {
            medassist_domain::note::NoteBody::Soap(s) => present_soap_sections(s),
            medassist_domain::note::NoteBody::FreeText { .. } => vec!["text"],
        };
        template.required_sections.iter().filter(|r| !present.contains(&r.as_str())).cloned().collect()
    }
}

/// Names of non-empty SOAP sections.
pub fn present_soap_sections(s: &SoapContent) -> Vec<&'static str> {
    SoapSections::ALL
        .iter()
        .zip([&s.subjective, &s.objective, &s.assessment, &s.plan])
        .filter_map(|(n, slot)| slot.as_ref().filter(|x| !x.text.trim().is_empty()).map(|_| *n))
        .collect()
}
