# MedAssist OSS - Privacy & Data Protection

**License:** GPL-3.0-or-later  
**Version:** 0.1.0  
**Last Updated:** 2026-10-16

---

## Overview

MedAssist OSS is designed with **privacy-by-design** principles, treating all clinical data as sensitive personal information in accordance with **LGPD (Lei Geral de Proteção de Dados - Brazil)** and similar data protection regulations.

This document outlines:
- Data classification and handling
- Data flows and retention policies
- Technical and organizational measures
- User rights and compliance
- Clinical safety considerations

---

## Data Classification

### 1. Personal Data (LGPD Art. 5, XII)

Any information related to an identified or identifiable natural person.

**In MedAssist OSS, this includes:**
- Patient identifiers (name, UUID, external IDs like CNS)
- Demographic information (birth date, sex, contact info)
- Clinical history and encounters
- Diagnoses, medications, allergies
- Vital signs measurements
- Documents and lab results
- Audio recordings and transcriptions
- Consent records

### 2. Sensitive Personal Data (LGPD Art. 11)

Personal data concerning health or intimate life.

**In MedAssist OSS, ALL clinical data is considered sensitive:**
- Health conditions and diagnoses
- Medical history
- Treatment information
- Genetic information (if collected)
- Biometric data (if collected)

### 3. Anonymous Data

Data that cannot be related back to an individual.

**In MedAssist OSS:**
- Aggregated statistics (if implemented)
- De-identified research data (if implemented with proper safeguards)

---

## Data Flow Architecture

### 1. Data at Rest

```
┌─────────────────────────────────────────────────────────────┐
│                    PostgreSQL Database                         │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Clinical Tables (patients, encounters, notes, etc.)       │ │
│  │  - org_id: Multi-tenant isolation                         │ │
│  │  - All data encrypted at storage level (future)            │ │
│  │  - Row-level access control via org_id filtering          │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Audit Tables (audit_events)                              │ │
│  │  - No clinical content stored                             │ │
│  │  - Actor, action, resource, timestamp, outcome only        │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    Filesystem Blob Store                       │
│  - Documents: Original files stored immutably                 │
│  - SHA-256 hash verification for integrity                    │
│  - Path sanitization prevents directory traversal            │
│  - Configurable base directory (MEDASSIST_BLOB_DIR)           │
└─────────────────────────────────────────────────────────────┘
```

### 2. Data in Transit

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│     Client       │────▶│   Reverse Proxy  │────▶│     API         │
│  (Browser/UI)    │     │   (TLS Term)    │     │  (Axum)         │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                                        │
                                                        ▼
                                                 ┌─────────────────┐
                                                 │  PostgreSQL     │
                                                 │  (TLS Recommended)│
                                                 └─────────────────┘

Requirements:
- TLS 1.2+ for all external connections
- HttpOnly, Secure, SameSite=Strict cookies
- No sensitive data in URLs or headers
```

### 3. Data in Processing

```
┌─────────────────────────────────────────────────────────────┐
│                    Memory (During Request)                     │
│  - Data loaded only for authenticated actor's org             │
│  - Minimal data loaded (pagination, filtering)                 │
│  - No caching of clinical data in memory                      │
│  - Sensitive fields masked in logs                            │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    AI Processing                               │
│  - Local inference preferred (no data leaves deployment)     │
│  - If remote AI used: explicit opt-in with warnings            │
│  - All AI output treated as untrusted (draft, unreviewed)      │
│  - Human review mandatory before finalization                 │
└─────────────────────────────────────────────────────────────┘
```

---

## Data Retention Policies

### 1. Clinical Data

| Data Type | Default Retention | Rationale |
|-----------|------------------|-----------|
| Patient records | Indefinite (or per org policy) | Medical records typically have long-term legal requirements |
| Encounters | Indefinite | Part of patient medical history |
| Notes (SOAP) | Indefinite | Clinical documentation |
| Documents/Labs | Indefinite | Supporting clinical evidence |
| Medications | Indefinite | Treatment history |
| Allergies | Indefinite | Critical safety information |
| Vital signs | Indefinite | Clinical observations |

**Note:** Organizations must configure their own retention policies based on:
- Local regulations (LGPD, HIPAA, etc.)
- Professional medical record requirements
- Legal obligations

### 2. Temporary Data

| Data Type | Retention | Automatic Cleanup |
|-----------|-----------|-------------------|
| Audio recordings | Configurable (default: 30 days) | Yes, after transcription |
| Transcription drafts | Until reviewed | Yes, after note creation |
| Session tokens | 24 hours (configurable) | Yes, on expiry |
| Job state | Until completion + 7 days | Yes |
| Audit logs | Configurable (default: 7 years) | Optional |

### 3. Deletion

- **Soft Delete:** Clinical data is soft-deleted (marked as inactive) to preserve audit trail
- **Hard Delete:** Available for temporary data (audio, drafts)
- **Purging:** Organizations can implement custom purge policies via CLI

---

## Technical Privacy Measures

### 1. Multi-Tenant Isolation

**Implementation:**
- Every clinical table has `org_id UUID NOT NULL`
- All queries filter by `WHERE org_id = ?`
- Cross-org access attempts are logged and denied
- Tests verify cross-org isolation

**Guarantees:**
- Organization A cannot access Organization B's data
- Even with direct database access, org_id filtering is enforced
- Audit trail records org_id for all operations

### 2. Provenance Tracking

**Every piece of clinical data carries:**
- `ProvenanceKind`: Human, AiSuggestion, Imported, Transcription
- `SourceRef`: Who/what created it (user_id, job_id, document_id, etc.)
- `reviewed`: Boolean indicating human review status

**Rules:**
- AI suggestions can never silently overwrite human edits
- AI output is always marked as `reviewed: false`
- Human review is mandatory before finalization
- Provenance is preserved through all transformations

### 3. Authentication & Authorization

**Password Security:**
- Argon2id hashing (memory-hard, resistant to GPU attacks)
- Configurable parameters (iterations, memory, parallelism)
- Never stored in plaintext

**Session Security:**
- Opaque session tokens (UUID v4)
- HttpOnly cookies (not accessible to JavaScript)
- Secure cookies (HTTPS only in production)
- SameSite=Strict (prevents CSRF)
- Expiring tokens (configurable TTL)
- Revocable sessions

**Lockout:**
- Failed attempt counter per user
- Configurable threshold and lockout duration
- Prevents brute-force attacks

### 4. Audit Trail

**Recorded for every operation:**
- Actor (user_id, org_id, roles)
- Action (create, read, update, delete, etc.)
- Resource type and ID
- Timestamp (UTC)
- Outcome (success/failure)
- Metadata (non-sensitive context)

**NOT recorded:**
- Full clinical content
- Credentials or tokens
- Sensitive personal data

### 5. Input Validation

**All inputs are validated:**
- Domain models validate on construction
- Service layer enforces business rules
- API layer validates DTOs
- Database constraints (FK, unique, not null)

**Protects against:**
- SQL injection (parameterized queries)
- Path traversal (blob store path sanitization)
- XSS (proper escaping in responses)
- CSRF (SameSite cookies, tokens)
- Overflow (numeric validation)

### 6. Data Minimization

**Principles:**
- Only load data needed for the operation
- Pagination for large result sets
- Filtering by org_id at database level
- No bulk exports without explicit authorization

**Implementation:**
- Repository methods accept limit/offset parameters
- Search endpoints return paginated results
- Related data loaded lazily (not eagerly)

---

## User Rights (LGPD Compliance)

### 1. Right to Information (LGPD Art. 18, I)

**Implementation:**
- Patients can request their data via API (future: patient portal)
- All data accessible through proper authentication
- Audit trail shows who accessed what and when

### 2. Right to Correction (LGPD Art. 18, II)

**Implementation:**
- All clinical data is editable with provenance tracking
- Amendments create new versions (SOAP note amendments)
- Original data preserved for audit
- Correction reason can be recorded

### 3. Right to Deletion (LGPD Art. 18, III)

**Implementation:**
- Soft delete available for all clinical entities
- Hard delete available for temporary data
- Organizations can implement custom deletion policies
- Deletion requests logged in audit trail

**Limitations:**
- Legal obligations may prevent deletion (medical records)
- Deletion may be limited to anonymization in some cases

### 4. Right to Data Portability (LGPD Art. 18, IV)

**Implementation:**
- JSON export of patient data (future)
- FHIR export format (future)
- Structured, machine-readable formats

### 5. Right to Revoke Consent (LGPD Art. 18, V)

**Implementation:**
- Consent records tracked separately
- Consent can be revoked via API
- Revocation logged in audit trail
- Data collected under revoked consent is preserved (legal requirement) but not used for new processing

---

## Clinical Safety Considerations

### 1. AI Assistance Safety

**Principles:**
- AI is a **support tool**, not a diagnostic tool
- All AI output is **unreviewed draft** until human confirmation
- AI can **never** auto-finalize clinical notes
- AI can **never** silently overwrite human edits

**Guardrails:**
- Anti-hallucination validation (numbers, dates must be grounded in input)
- Provenance tracking (clear distinction between human and AI content)
- Human review mandatory (UI must surface unreviewed sections)
- Gap flagging (missing information is explicit, not invented)

### 2. Transcription Safety

**Requirements:**
- Explicit consent required before any recording
- Consent is separate from general terms of service
- Consent validity window (configurable, default: 1 year)
- Consent can be revoked at any time
- Audio files retained only as long as necessary

**Implementation:**
- `Consent` entity tracks patient consent
- `Transcription` entity requires `consent_id`
- Transcription cannot start without valid consent
- Consent validation in service layer

### 3. Document Integrity

**Measures:**
- SHA-256 hash of all uploaded documents
- Hash stored with document metadata
- Hash verified on download
- Original files never modified (immutable storage)

### 4. Audit for Clinical Safety

**Recorded:**
- Who created/modified clinical data
- When it was created/modified
- What was changed (via version history)
- Whether AI was involved
- Whether human review occurred

---

## Operational Security

### 1. Deployment Security

**Requirements:**
- Environment variables for all secrets (never in code)
- Production mode refuses insecure defaults
- TLS termination at reverse proxy
- Database encryption at rest (recommended)
- Regular backups with encryption

**Configuration:**
```bash
# Required environment variables
DATABASE_URL=postgres://user:password@host:port/db
MEDASSIST_BIND=0.0.0.0:8080
MEDASSIST_ENV=production
MEDASSIST_BLOB_DIR=/var/lib/medassist/blobs

# Optional
MEDASSIST_LLM_ID=ollama:qwen2.5:7b
MEDASSIST_LLM_EXTERNAL=false
```

### 2. Network Security

**Requirements:**
- Firewall rules limiting access to API port
- Database not exposed to public internet
- AI providers (if used) configured with proper authentication
- Rate limiting on API endpoints

### 3. Incident Response

**Data Breach Procedures:**
1. Contain the breach (isolate affected systems)
2. Assess scope (what data was accessed)
3. Notify affected individuals (within LGPD timeline: "promptly")
4. Notify authorities (ANPD within 2 business days for severe breaches)
5. Investigate and remediate root cause
6. Document incident and response

---

## Compliance Checklist

### LGPD (Brazil)

- [x] Lawful basis for processing (consent, contract, legal obligation)
- [x] Data minimization (only collect what's necessary)
- [x] Purpose limitation (data used only for stated purposes)
- [x] Storage limitation (retention policies configurable)
- [x] Data subject rights (access, correction, deletion, portability)
- [x] Security measures (encryption, access control, audit)
- [x] Data processing records (audit trail)
- [ ] DPO appointment (required for some organizations)
- [ ] DPIA for high-risk processing (recommended for healthcare)

### HIPAA (USA - if applicable)

- [ ] Business Associate Agreements (if hosting for US clients)
- [ ] PHI safeguards
- [ ] Breach notification procedures

### GDPR (EU - if applicable)

- [ ] DPO appointment (if processing EU resident data)
- [ ] Data processing agreements
- [ ] Right to be forgotten implementation

---

## Data Subject Requests

### Handling a Request

1. **Verify Identity:** Confirm requester is the data subject or authorized representative
2. **Log Request:** Record in audit trail
3. **Process Request:**
   - Access: Provide data in readable format
   - Correction: Allow edits with provenance
   - Deletion: Soft-delete with audit preservation
   - Portability: Export in structured format
   - Consent Revocation: Mark consent as revoked
4. **Respond:** Within LGPD timeline (15 days, extendable to 15 more)
5. **Document:** Record response in audit trail

### Automation (Future)

- Self-service patient portal for data access
- Automated export generation
- Consent management interface

---

## Third-Party Data Processing

### 1. AI Model Providers

**If using remote AI providers:**
- Explicit opt-in required
- Data processing agreement recommended
- Patient consent for data leaving deployment boundary
- Clear disclosure of what data is sent

**Local AI (Recommended):**
- No data leaves deployment boundary
- No third-party processing
- Full control over data

### 2. Cloud Hosting

**If self-hosted:**
- Full control over data
- Responsible for all compliance

**If using cloud provider:**
- Data processing agreement required
- Verify cloud provider's compliance certifications
- Encrypt data at rest and in transit
- Consider jurisdiction of data storage

---

## Training and Awareness

### For Developers

- Privacy-by-design principles
- LGPD requirements
- Secure coding practices
- Incident response procedures

### For Users (Healthcare Professionals)

- AI is a support tool, not a replacement for judgment
- Human review is mandatory for all AI output
- Provenance indicates source of all information
- Audit trail tracks all access and changes

---

## References

- [LGPD (Lei Geral de Proteção de Dados)](https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/L13709.htm)
- [ANPD (Autoridade Nacional de Proteção de Dados)](https://www.gov.br/anpd/pt-br)
- [GDPR (General Data Protection Regulation)](https://gdpr-info.eu/)
- [HIPAA (Health Insurance Portability and Accountability Act)](https://www.hhs.gov/hipaa/index.html)
- [ISO 27001 (Information Security Management)](https://www.iso.org/isoiec-27001-information-security.html)
- [WCAG 2.2 AA (Accessibility)](https://www.w3.org/WAI/standards-guidelines/wcag/)
