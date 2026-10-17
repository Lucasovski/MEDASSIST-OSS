# MedAssist OSS - Privacy and Data Protection

**License:** GPL-3.0-or-later  
**Status:** Work in Progress  
**Last Updated:** 2026-10-16

---

## Overview

MedAssist OSS is designed with **privacy-by-design** principles at its core. This document describes how the system handles personal data, ensures compliance with privacy regulations (particularly **LGPD - Lei Geral de Proteção de Dados** in Brazil), and implements technical and organizational measures to protect patient and user data.

**Important:** This document describes the technical implementation and design decisions. It does NOT constitute legal advice or certification of compliance. Organizations using MedAssist OSS must conduct their own legal review and implement additional measures as required by their jurisdiction and use case.

---

## Privacy Principles

### 1. Data Minimization

**Principle:** Collect and store only the minimum data necessary for the intended purpose.

**Implementation:**
- All domain models require only essential fields
- Optional fields are clearly marked and have sensible defaults
- No redundant data storage (normalize where appropriate)
- Sensitive data is not logged or stored unnecessarily

### 2. Purpose Limitation

**Principle:** Data is collected for specific, explicit, and legitimate purposes and not further processed in a manner incompatible with those purposes.

**Implementation:**
- Each data field has a clearly defined purpose
- Data processing is limited to clinical documentation support
- No data is used for marketing, advertising, or unrelated purposes

### 3. Data Quality

**Principle:** Keep personal data accurate and, where necessary, up to date.

**Implementation:**
- Audit trails track all changes to clinical data
- Version history preserves previous versions
- Users can correct inaccurate information
- System validates data integrity (hashes, checksums)

### 4. Transparency

**Principle:** Provide clear information about data collection, processing, and storage.

**Implementation:**
- Audit logging for all sensitive operations
- Clear documentation of data flows
- User-accessible audit trails (within their permissions)

### 5. Security

**Principle:** Implement appropriate technical and organizational measures to protect data.

**Implementation:**
- Encryption at rest (delegated to PostgreSQL)
- Encryption in transit (TLS)
- Access controls and authentication
- Secure session management
- Input validation and sanitization

### 6. Data Subject Rights

**Principle:** Respect data subject rights including access, rectification, erasure, and portability.

**Implementation:**
- API endpoints for data access
- Audit trails for data changes
- Soft deletion with configurable retention
- Data export capabilities (planned)

---

## Data Classification

### Personal Data Categories

| Category | Description | Examples | Sensitivity |
|----------|-------------|----------|-------------|
| **Identification Data** | Data that directly identifies an individual | Name, email, CPF, CNS, RG | High |
| **Clinical Data** | Health-related information | Diagnoses, treatments, medications, allergies | High |
| **Contact Data** | Communication information | Phone, address, email | Medium |
| **Technical Data** | System usage information | IP addresses, session tokens, timestamps | Medium |
| **Audit Data** | Operational logs | Who accessed what, when | Medium |

### Special Categories of Personal Data

Under LGPD, health data is considered **sensitive personal data** and requires enhanced protection:

- **Health Data:** Any information related to the physical or mental health of a data subject
- **Biometric Data:** Fingerprints, facial recognition, etc. (not currently collected)
- **Genetic Data:** Genetic information (not currently collected)

**MedAssist OSS primarily handles health data and identification data, both of which are subject to strict protection requirements.**

---

## Data Flows

### 1. Patient Data Flow

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Healthcare     │────▶│   MedAssist     │────▶│   PostgreSQL     │
│   Professional   │     │   API Server     │     │   Database       │
│   (Browser/App)  │◀────│                 │◀────│                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                              │
                              ▼
                     ┌─────────────────┐
                     │   Audit Log      │
                     │   (Access Track) │
                     └─────────────────┘
```

**Data Elements:**
- Patient identifiers (name, external IDs)
- Clinical observations (vital signs, symptoms)
- Diagnoses and treatments
- Medications and allergies
- Documents and images

### 2. Authentication Data Flow

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   User           │────▶│   MedAssist     │────▶│   PostgreSQL     │
│   (Login)        │     │   API Server     │     │   Database       │
│                 │◀────│                 │     │   (Users,        │
└─────────────────┘     └─────────────────┘     │    Sessions)     │
                                                └─────────────────┘
```

**Data Elements:**
- Email address
- Password hash (Argon2id)
- Session tokens
- Authentication timestamps

### 3. AI Processing Data Flow

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   User           │────▶│   MedAssist     │────▶│   AI Engine      │
│   (Request)      │     │   API Server     │     │   (Ollama)       │
│                 │◀────│                 │◀────│                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                              │
                              ▼
                     ┌─────────────────┐
                     │   Provenance     │
                     │   Tracking       │
                     └─────────────────┘
```

**Data Elements:**
- Clinical text (prompt to AI)
- AI-generated suggestions
- Provenance metadata (source, model, timestamp)

**Important:** AI processing is performed locally (self-hosted) and data does NOT leave the organization's infrastructure.

### 4. Document Upload Flow

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   User           │────▶│   MedAssist     │────▶│   Filesystem     │
│   (Upload)       │     │   API Server     │     │   Blob Store     │
│                 │◀────│                 │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                              │
                              ▼
                     ┌─────────────────┐
                     │   PostgreSQL     │
                     │   (Metadata)     │
                     └─────────────────┘
```

**Data Elements:**
- Document content (stored in filesystem)
- Document metadata (stored in PostgreSQL)
- SHA-256 hash (for integrity verification)
- Provenance information

---

## Data Storage

### PostgreSQL Database

**Location:** Self-hosted PostgreSQL server (configured via `DATABASE_URL`)

**Tables and Data Types:**

| Table | Data Type | Sensitivity | Retention |
|-------|-----------|-------------|-----------|
| `patients` | Identification, Clinical | High | Configurable |
| `encounters` | Clinical | High | Configurable |
| `notes` | Clinical | High | Configurable |
| `medications` | Clinical | High | Configurable |
| `allergies` | Clinical | High | Configurable |
| `vital_signs` | Clinical | High | Configurable |
| `documents` | Clinical, Identification | High | Configurable |
| `transcriptions` | Clinical, Audio | High | Configurable |
| `users` | Identification, Auth | High | Configurable |
| `sessions` | Auth | Medium | Session lifetime |
| `audit_events` | Audit | Medium | Configurable |
| `organisations` | Identification | Medium | Configurable |

### Filesystem Blob Store

**Location:** Local filesystem (path configurable via `BLOB_STORE_PATH`)

**Data Stored:**
- Uploaded documents (PDFs, images, etc.)
- Transcription audio files
- AI model files (optional)

**Organization:**
```
blob-store/
├── documents/
│   └── {org_id}/
│       └── {document_id}.{extension}
├── transcriptions/
│   └── {org_id}/
│       └── {transcription_id}.{extension}
└── models/
    └── {model_name}/
        └── model files
```

### Logs

**Location:** stdout (containerized) or file

**Log Types:**
- **Application Logs:** Operational information (INFO, DEBUG, etc.)
- **Error Logs:** Error conditions and stack traces
- **Audit Logs:** Sensitive access tracking (separate table)

**Log Retention:**
- Application logs: Configurable (default: 30 days)
- Error logs: Configurable (default: 90 days)
- Audit logs: Configurable (default: 7 years for compliance)

---

## Data Protection Measures

### 1. Access Control

**Multi-Tenant Isolation:**
- Every clinical table has an `org_id` column
- All queries filter by `org_id` from the authenticated user's session
- Prevents cross-organization data access

**Role-Based Access Control (RBAC):**
- **Admin:** Full access to organization data
- **Clinician:** Read/write access to clinical data
- **Patient:** Read-only access to their own data (planned)

**Row-Level Security:**
- Application layer enforces data access rules
- Future: PostgreSQL Row-Level Security (RLS) policies

### 2. Authentication and Authorization

**Password Security:**
- **Algorithm:** Argon2id (memory-hard, resistant to GPU attacks)
- **Parameters:** Default Argon2id parameters (safe defaults)
- **Storage:** Password hashes only, never plaintext
- **Salting:** Unique salt per password

**Session Management:**
- **Tokens:** Opaque, random session tokens
- **Storage:** Session tokens stored in PostgreSQL
- **Expiration:** Configurable session timeout (default: 24 hours)
- **Lockout:** Temporary lockout after failed login attempts (default: 5 attempts, 15 minutes)

**Multi-Factor Authentication (Future):**
- TOTP (Time-based One-Time Password)
- Hardware security keys (WebAuthn)

### 3. Encryption

**At Rest:**
- PostgreSQL encryption (delegated to database configuration)
- Filesystem encryption (delegated to OS/filesystem)
- Recommendation: Use encrypted volumes for blob store

**In Transit:**
- **TLS:** Required for all HTTP connections
- **Certificate Validation:** Strict certificate validation
- **Cipher Suites:** Modern, secure cipher suites only

**Secrets Management:**
- **Environment Variables:** All secrets stored in environment variables
- **Never in Code:** Secrets never committed to version control
- **.env.example:** Template file with placeholder values
- **Secret Rotation:** Support for secret rotation without downtime

### 4. Input Validation and Sanitization

**Input Validation:**
- All external inputs validated at domain layer
- Type-safe parsing with `serde`
- Custom validation for clinical data (plausible ranges, etc.)

**Sanitization:**
- Path sanitization for filesystem operations
- SQL injection prevention via SQLx parameterized queries
- XSS prevention via proper HTML escaping (in Web UI)

**File Upload Validation:**
- File type validation (MIME type and magic bytes)
- File size limits (configurable, default: 10MB)
- File name sanitization
- Virus scanning integration (future)

### 5. Audit Trail

**Audit Logging:**
- All sensitive operations logged
- Audit events stored in dedicated table
- Immutable audit trail (no updates or deletions)

**Audited Operations:**
- Patient data access (read, create, update, delete)
- Clinical data modifications
- Authentication events (login, logout, failed attempts)
- Authorization changes (role assignments)
- Data export operations

**Audit Event Structure:**
```rust
pub struct AuditEvent {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Option<Uuid>,
    pub event_type: AuditEventType,
    pub entity_type: String,
    pub entity_id: Option<Uuid>,
    pub action: String,
    pub timestamp: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: JsonValue,
}
```

**Important:** Audit logs do NOT contain the actual clinical data, only metadata about the operation.

---

## LGPD Compliance

### Legal Basis for Processing

MedAssist OSS processes personal data under the following legal bases (as defined by LGPD):

| Processing Activity | Legal Basis | Article |
|---------------------|-------------|---------|
| Clinical documentation | Consent | Art. 11 |
| Patient identification | Consent | Art. 11 |
| Authentication | Legitimate interest | Art. 10 |
| Audit logging | Legal obligation | Art. 7, II |
| System operation | Legitimate interest | Art. 10 |

**Note:** The data controller (organization using MedAssist OSS) is responsible for:
1. Obtaining appropriate consent from data subjects
2. Providing privacy notices
3. Honoring data subject rights
4. Maintaining records of processing activities

### Data Subject Rights

MedAssist OSS provides technical capabilities to support data subject rights:

| Right | Implementation | Status |
|-------|----------------|--------|
| **Access** | API endpoints for data retrieval | ✅ Implemented |
| **Rectification** | Update endpoints for data correction | ✅ Implemented |
| **Erasure** | Soft deletion with configurable retention | ✅ Implemented |
| **Restriction** | Data access controls | ✅ Implemented |
| **Portability** | Data export capabilities | ⏳ Planned |
| **Objection** | Configurable processing preferences | ⏳ Planned |
| **Automated Decision-Making** | Human-in-the-loop for all AI | ✅ Implemented |

### Data Protection Officer (DPO)

Organizations using MedAssist OSS should designate a Data Protection Officer (DPO) responsible for:
- Monitoring compliance with LGPD
- Advising on data protection impact assessments
- Serving as contact point for data subjects and supervisory authorities

MedAssist OSS provides audit capabilities to assist the DPO in their duties.

### Data Protection Impact Assessment (DPIA)

Organizations should conduct a DPIA before deploying MedAssist OSS, considering:
- The nature, scope, context, and purposes of processing
- The risks to data subjects' rights and freedoms
- The measures to address risks and demonstrate compliance

MedAssist OSS provides documentation and technical measures to support DPIA completion.

---

## Security Measures

### 1. Network Security

**TLS Configuration:**
- Minimum TLS version: 1.2 (recommended: 1.3)
- Cipher suites: Modern, secure suites only
- Certificate validation: Strict (no self-signed in production)

**Firewall Rules:**
- Restrict API access to authorized networks
- Rate limiting at network level
- DDoS protection (recommended)

### 2. Application Security

**OWASP Top 10 Mitigations:**

| Risk | Mitigation | Status |
|------|------------|--------|
| Injection | Parameterized queries (SQLx), input validation | ✅ |
| Broken Authentication | Strong password hashing, session management | ✅ |
| Sensitive Data Exposure | Encryption, no plaintext secrets | ✅ |
| XML External Entities | Not applicable (no XML parsing) | N/A |
| Broken Access Control | RBAC, multi-tenant isolation | ✅ |
| Security Misconfiguration | Secure defaults, configuration validation | ✅ |
| Cross-Site Scripting (XSS) | HTML escaping (Web UI) | ⏳ Planned |
| Insecure Deserialization | Safe deserialization with serde | ✅ |
| Using Components with Known Vulnerabilities | Dependency scanning (cargo-deny) | ✅ |
| Insufficient Logging & Monitoring | Comprehensive audit logging | ✅ |

**Additional Measures:**
- CSRF protection (Web UI)
- CORS configuration
- Content Security Policy (CSP)
- Security headers

### 3. Database Security

**PostgreSQL Configuration:**
- Separate database user with limited privileges
- No superuser access for application
- Connection pooling with limits
- Query timeouts

**Backup and Recovery:**
- Regular backups (recommended: daily)
- Encrypted backups
- Offsite backup storage
- Tested recovery procedures

### 4. Operational Security

**Access Management:**
- Principle of least privilege
- Regular access reviews
- Immediate revocation of access for terminated users

**Incident Response:**
- Incident detection and alerting
- Incident response procedures
- Breach notification requirements (LGPD: 2 business days for authority, reasonable time for data subjects)

**Business Continuity:**
- Disaster recovery plan
- Backup and restore testing
- Failover capabilities (future)

---

## Data Retention and Deletion

### Retention Policies

| Data Type | Default Retention | Configurable | Legal Requirement |
|-----------|-------------------|--------------|------------------|
| Clinical Data | 20 years | Yes | LGPD, medical records laws |
| Patient Identification | 20 years | Yes | LGPD, medical records laws |
| Authentication Data | 1 year (inactive) | Yes | LGPD |
| Session Data | Session lifetime | Yes | Operational |
| Audit Logs | 7 years | Yes | LGPD, compliance |
| Documents | 20 years | Yes | Medical records laws |
| Transcriptions | 20 years | Yes | Medical records laws |

**Note:** Retention periods may vary by jurisdiction and should be configured according to local legal requirements.

### Deletion Procedures

**Soft Deletion:**
- Data marked as deleted but not physically removed
- Can be restored within retention period
- Audit trail preserved

**Hard Deletion:**
- Physical removal of data after retention period
- Automated cleanup processes
- Manual deletion for data subject requests

**Deletion Workflow:**
1. User requests deletion (via API or admin interface)
2. System validates request (authentication, authorization)
3. System performs soft deletion
4. System schedules hard deletion (after retention period)
5. System logs deletion in audit trail
6. System notifies requester of completion

---

## Third-Party Services

### AI Models and Services

**Important:** MedAssist OSS is designed for **local-first AI inference** to minimize data exposure.

| Service | Data Location | Data Shared | Sensitivity |
|---------|---------------|-------------|-------------|
| Ollama (Local) | Self-hosted | No external sharing | None (local) |
| Test Double | In-process | No external sharing | None (test data only) |
| Future: Cloud AI | External provider | Potentially shared | High (requires assessment) |

**Recommendation:** Use local AI models (Ollama) to keep all data within your infrastructure.

### Dependencies

All dependencies are audited for:
- License compatibility (GPL-3.0-or-later)
- Security vulnerabilities (cargo-deny)
- Privacy implications

See `THIRD_PARTY_LICENSES.md` for complete dependency list.

---

## User Responsibilities

### System Administrators

- Configure appropriate access controls
- Implement backup and recovery procedures
- Monitor system logs and alerts
- Keep software up to date
- Conduct regular security audits

### Healthcare Professionals

- Verify AI-generated suggestions before use
- Maintain patient confidentiality
- Use appropriate authentication credentials
- Report suspicious activity
- Follow organizational data handling policies

### Patients (Future)

- Provide accurate information
- Review and correct their data as needed
- Use strong, unique passwords
- Report privacy concerns

---

## Privacy by Design Checklist

- [x] Data minimization implemented
- [x] Purpose limitation documented
- [x] Multi-tenant isolation enforced
- [x] Access controls implemented
- [x] Authentication and authorization secure
- [x] Audit trail comprehensive
- [x] Encryption at rest (delegated)
- [x] Encryption in transit (TLS)
- [x] Input validation and sanitization
- [x] Secure defaults
- [x] Human-in-the-loop for AI
- [x] Provenance tracking
- [x] Immutable audit logs
- [ ] Data portability implemented
- [ ] Data subject rights fully supported
- [ ] Privacy impact assessment template
- [ ] Incident response procedures documented

---

## Limitations and Disclaimers

### Not Legal Advice

This document describes the technical implementation of privacy and data protection measures in MedAssist OSS. It does NOT constitute legal advice. Organizations using MedAssist OSS should:

1. Consult with legal counsel to ensure compliance with applicable laws
2. Conduct their own privacy impact assessments
3. Implement additional measures as required by their specific use case and jurisdiction

### Not Certified

MedAssist OSS is NOT certified for:
- HIPAA compliance (US)
- GDPR compliance (EU)
- LGPD compliance (Brazil)
- Any other regulatory compliance

Certification requires independent audit and verification, which is the responsibility of the deploying organization.

### Not for Production Use Without Review

MedAssist OSS is open-source software provided "as is" without warranty. Before deploying to production:

1. Conduct a thorough security review
2. Perform penetration testing
3. Validate all privacy and security controls
4. Implement appropriate operational procedures
5. Obtain necessary certifications and approvals

### AI Limitations

- AI output is NOT guaranteed to be accurate
- AI output must ALWAYS be reviewed by a qualified healthcare professional
- AI output should NOT be used as the sole basis for clinical decisions
- MedAssist OSS is a tool to assist healthcare professionals, not replace them

---

## References

- [LGPD - Lei Geral de Proteção de Dados (Brazil)](https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/L13709.htm)
- [GDPR - General Data Protection Regulation (EU)](https://gdpr-info.eu/)
- [HIPAA - Health Insurance Portability and Accountability Act (US)](https://www.hhs.gov/hipaa/index.html)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [NIST Cybersecurity Framework](https://www.nist.gov/cyberframework)
- [ISO/IEC 27001](https://www.iso.org/isoiec-27001-information-security.html)

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-16 | MedAssist OSS Contributors | Initial privacy documentation |
