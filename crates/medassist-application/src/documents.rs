//! Document import use-cases: validation, hashing, persistence of metadata,
//! blob storage and extraction job creation. Originals are immutable;
//! extractions stay unreviewed until a professional confirms them.

use chrono::Utc;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::errors::{Actor, AppError};
use crate::ports::{
    AuditEvent, AuditOutcome, AuditSink, BlobStore, DocumentRepository, PatientRepository,
};
use medassist_domain::document::{ClinicalDocument, ProcessingState};

pub struct DocumentService<D: DocumentRepository, B: BlobStore, P: PatientRepository, A: AuditSink>
{
    pub docs: D,
    pub blobs: B,
    pub patients: P,
    pub audit: A,
}

impl<D: DocumentRepository, B: BlobStore, P: PatientRepository, A: AuditSink>
    DocumentService<D, B, P, A>
{
    pub fn new(docs: D, blobs: B, patients: P, audit: A) -> Self {
        Self {
            docs,
            blobs,
            patients,
            audit,
        }
    }

    /// Import a document for a patient. Validates filename/mime/size, computes
    /// SHA-256 over the received bytes, stores the original immutably and
    /// records metadata in state `Pending`.
    pub async fn import(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        encounter_id: Option<Uuid>,
        filename: String,
        content_type: String,
        bytes: Vec<u8>,
    ) -> Result<ClinicalDocument, AppError> {
        // Tenant check first (also prevents cross-org probing via hashes).
        // A missing patient for this actor means either it does not exist or
        // belongs to another org — both are answered identically (403) so the
        // API never leaks existence across tenants.
        let patient = match self.patients.get(actor, patient_id).await? {
            Some(p) => p,
            None => {
                self.audit(
                    actor,
                    Some(patient_id),
                    "document.import",
                    AuditOutcome::Denied,
                )
                .await;
                return Err(AppError::Forbidden("patient not visible to you"));
            }
        };
        if patient.org_id != actor.org_id && !actor.is_admin() {
            self.audit(
                actor,
                Some(patient_id),
                "document.import",
                AuditOutcome::Denied,
            )
            .await;
            return Err(AppError::Forbidden("patient is outside your organisation"));
        }

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        let kind =
            ClinicalDocument::validate_new(&filename, &content_type, bytes.len() as u64, &sha256)?;

        let id = Uuid::new_v4();
        let doc = ClinicalDocument {
            id,
            org_id: patient.org_id,
            patient_id,
            encounter_id,
            kind,
            filename: filename
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&filename)
                .to_owned(),
            content_type,
            size_bytes: bytes.len() as u64,
            sha256,
            received_at: None,
            state: ProcessingState::Pending,
            error: None,
            uploaded_by: actor.user_id,
            created_at: Utc::now(),
        };
        self.blobs.put(patient.org_id, id, &bytes).await?;
        self.docs.insert(&doc).await?;
        self.audit(actor, Some(id), "document.import", AuditOutcome::Success)
            .await;
        Ok(doc)
    }

    /// List documents attached to a patient (newest first), org-scoped.
    pub async fn list_for_patient(
        &self,
        actor: &Actor,
        patient_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<crate::ports::Page<ClinicalDocument>, AppError> {
        // Authorised patient read enforces visibility before listing.
        let _p = match self.patients.get(actor, patient_id).await? {
            Some(p) => p,
            None => return Err(AppError::NotFound("patient")),
        };
        self.docs
            .list_for_patient(actor, patient_id, limit, offset)
            .await
    }

    pub async fn get(&self, actor: &Actor, id: Uuid) -> Result<ClinicalDocument, AppError> {
        match self.docs.get(actor, id).await? {
            Some(d) => Ok(d),
            None => Err(AppError::NotFound("document")),
        }
    }

    pub async fn download(&self, actor: &Actor, id: Uuid) -> Result<Vec<u8>, AppError> {
        let d = self.get(actor, id).await?;
        let key = format!("{}/{}", d.org_id, d.id);
        self.blobs.get(&key).await
    }

    /// Mark extraction results as reviewed by a professional.
    pub async fn mark_reviewed(&self, actor: &Actor, id: Uuid) -> Result<(), AppError> {
        if !actor.has_role("practitioner") && !actor.is_admin() {
            self.audit(actor, Some(id), "document.review", AuditOutcome::Denied)
                .await;
            return Err(AppError::Forbidden("requires practitioner role"));
        }
        let d = self.get(actor, id).await?;
        if d.state != ProcessingState::Extracted {
            return Err(AppError::Unprocessable(format!(
                "cannot review document in state {:?}",
                d.state
            )));
        }
        self.docs
            .set_state(actor, id, ProcessingState::Reviewed, None)
            .await?;
        self.audit(actor, Some(id), "document.review", AuditOutcome::Success)
            .await;
        Ok(())
    }

    async fn audit(
        &self,
        actor: &Actor,
        resource_id: Option<Uuid>,
        action: &str,
        outcome: AuditOutcome,
    ) {
        self.audit
            .record(AuditEvent {
                time: Utc::now(),
                actor_user_id: Some(actor.user_id),
                org_id: Some(actor.org_id),
                action: action.into(),
                resource_type: "document".into(),
                resource_id,
                outcome,
                metadata: serde_json::json!({}),
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::ports::{Page, PatientQuery, PatientRepository};
    use medassist_domain::patient::{NewPatient, Patient};

    #[derive(Default)]
    struct MemDocs(Mutex<HashMap<Uuid, ClinicalDocument>>);

    #[async_trait::async_trait]
    impl DocumentRepository for MemDocs {
        async fn insert(&self, d: &ClinicalDocument) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(d.id, d.clone());
            Ok(())
        }
        async fn get(&self, _a: &Actor, id: Uuid) -> Result<Option<ClinicalDocument>, AppError> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
        async fn set_state(
            &self,
            _a: &Actor,
            id: Uuid,
            state: ProcessingState,
            error: Option<String>,
        ) -> Result<(), AppError> {
            if let Some(d) = self.0.lock().unwrap().get_mut(&id) {
                d.state = state;
                d.error = error;
            }
            Ok(())
        }
        async fn list_for_patient(
            &self,
            _a: &Actor,
            pid: Uuid,
            limit: i64,
            offset: i64,
        ) -> Result<Page<ClinicalDocument>, AppError> {
            let items: Vec<_> = self
                .0
                .lock()
                .unwrap()
                .values()
                .filter(|d| d.patient_id == pid)
                .cloned()
                .collect();
            Ok(Page {
                total: items.len() as i64,
                limit,
                offset,
                items,
            })
        }
    }

    #[derive(Default)]
    struct MemBlobs(Mutex<HashMap<String, Vec<u8>>>);

    #[async_trait::async_trait]
    impl BlobStore for MemBlobs {
        async fn put(&self, org: Uuid, doc: Uuid, bytes: &[u8]) -> Result<String, AppError> {
            let key = format!("{org}/{doc}");
            self.0.lock().unwrap().insert(key.clone(), bytes.to_vec());
            Ok(key)
        }
        async fn get(&self, key: &str) -> Result<Vec<u8>, AppError> {
            self.0
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or(AppError::NotFound("blob"))
        }
        async fn delete(&self, key: &str) -> Result<(), AppError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemPatients(Mutex<HashMap<Uuid, Patient>>);

    #[async_trait::async_trait]
    impl PatientRepository for MemPatients {
        async fn insert(&self, p: &Patient) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(p.id, p.clone());
            Ok(())
        }
        async fn get(&self, _a: &Actor, id: Uuid) -> Result<Option<Patient>, AppError> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
        async fn search(&self, _a: &Actor, q: &PatientQuery) -> Result<Page<Patient>, AppError> {
            let items: Vec<_> = self.0.lock().unwrap().values().cloned().collect();
            Ok(Page {
                total: items.len() as i64,
                limit: q.limit,
                offset: q.offset,
                items,
            })
        }
        async fn update(&self, p: &Patient) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(p.id, p.clone());
            Ok(())
        }
        async fn soft_delete(
            &self,
            _a: &Actor,
            id: Uuid,
            now: DateTimeHelper,
        ) -> Result<bool, AppError> {
            if let Some(p) = self.0.lock().unwrap().get_mut(&id) {
                p.deleted_at = Some(now);
                return Ok(true);
            }
            Ok(false)
        }
        async fn duplicate_candidates(
            &self,
            _a: &Actor,
            _n: String,
            _b: Option<chrono::NaiveDate>,
            _k: Vec<String>,
        ) -> Result<Vec<(Patient, f64)>, AppError> {
            Ok(vec![])
        }
    }

    type DateTimeHelper = chrono::DateTime<Utc>;

    #[derive(Default)]
    struct MemAudit(Mutex<Vec<AuditEvent>>);
    #[async_trait::async_trait]
    impl AuditSink for MemAudit {
        async fn record(&self, ev: AuditEvent) {
            self.0.lock().unwrap().push(ev);
        }
    }

    fn svc() -> DocumentService<MemDocs, MemBlobs, MemPatients, MemAudit> {
        DocumentService::new(
            MemDocs::default(),
            MemBlobs::default(),
            MemPatients::default(),
            MemAudit::default(),
        )
    }

    fn actor_with_org(org: Uuid) -> Actor {
        Actor {
            user_id: Uuid::new_v4(),
            org_id: org,
            roles: vec!["recorder".into()],
        }
    }

    fn seed_patient(
        s: &DocumentService<MemDocs, MemBlobs, MemPatients, MemAudit>,
        org: Uuid,
    ) -> Patient {
        let p = NewPatient {
            org_id: org,
            family_name: "Teste".into(),
            given_name: "Paciente".into(),
            birth_date: None,
            sex: None,
            contact: None,
            external_ids: vec![],
            notes: None,
        }
        .into_patient(Uuid::new_v4(), Utc::now());
        s.patients.0.lock().unwrap().insert(p.id, p.clone());
        p
    }

    #[tokio::test]
    async fn imports_pdf_computes_hash_and_stores_blob() {
        let s = svc();
        let org = Uuid::new_v4();
        let a = actor_with_org(org);
        let p = seed_patient(&s, org);
        let bytes = b"%PDF-1.4 fake synthetic pdf".to_vec();
        let d = s
            .import(
                &a,
                p.id,
                None,
                "laudo.pdf".into(),
                "application/pdf".into(),
                bytes.clone(),
            )
            .await
            .unwrap();
        assert_eq!(d.kind, medassist_domain::document::DocumentKind::Pdf);
        assert_eq!(d.sha256, {
            let mut h = Sha256::new();
            h.update(&bytes);
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        });
        let got = s.download(&a, d.id).await.unwrap();
        assert_eq!(got, bytes);
    }

    #[tokio::test]
    async fn rejects_unsafe_uploads_and_cross_org_patients() {
        let s = svc();
        let org = Uuid::new_v4();
        let a = actor_with_org(org);
        let p = seed_patient(&s, org);

        let err = s
            .import(
                &a,
                p.id,
                None,
                "../evil.pdf".into(),
                "application/pdf".into(),
                b"x".to_vec(),
            )
            .await;
        assert!(matches!(
            err,
            Err(AppError::Domain(
                medassist_domain::error::DomainError::Validation { .. }
            ))
        ));

        let other_org = Uuid::new_v4();
        let other_actor = actor_with_org(other_org);
        let err = s
            .import(
                &other_actor,
                p.id,
                None,
                "ok.pdf".into(),
                "application/pdf".into(),
                b"x".to_vec(),
            )
            .await;
        assert!(matches!(err, Err(AppError::Forbidden(_))));
    }

    #[tokio::test]
    async fn review_requires_extracted_state_and_practitioner() {
        let s = svc();
        let org = Uuid::new_v4();
        let a = actor_with_org(org);
        let p = seed_patient(&s, org);
        let d = s
            .import(
                &a,
                p.id,
                None,
                "r.txt".into(),
                "text/plain".into(),
                b"resultado".to_vec(),
            )
            .await
            .unwrap();
        // pending -> cannot review
        assert!(s.mark_reviewed(&a, d.id).await.is_err());
        // extracted but recorder lacks role
        s.docs
            .set_state(&a, d.id, ProcessingState::Extracted, None)
            .await
            .unwrap();
        let err = s.mark_reviewed(&a, d.id).await.unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
        // practitioner can
        let prac = Actor {
            user_id: a.user_id,
            org_id: org,
            roles: vec!["practitioner".into()],
        };
        s.mark_reviewed(&prac, d.id).await.unwrap();
        assert_eq!(
            s.get(&prac, d.id).await.unwrap().state,
            ProcessingState::Reviewed
        );
    }
}
