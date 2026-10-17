# MedAssist OSS - Architecture Documentation

**License:** GPL-3.0-or-later  
**Status:** Work in Progress  
**Last Updated:** 2026-10-16

---

## Overview

MedAssist OSS is a **self-hostable, privacy-first clinical documentation system** that uses AI to assist healthcare professionals while maintaining strict human oversight and data isolation.

This document describes the **architectural patterns, crate structure, dependency rules, and design decisions** that guide the development of MedAssist OSS.

---

## Architecture Pattern: Clean/Hexagonal Architecture

MedAssist OSS follows a **Clean/Hexagonal Architecture** pattern with strict layer separation:

```
┌─────────────────────────────────────────────────────────────┐
│                        Presentation Layer                        │
│   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐  │
│   │   API (Axum)    │  │   CLI (Clap)    │  │   Web UI    │  │
│   │                 │  │                 │  │  (Leptos)   │  │
│   └────────┬────────┘  └────────┬────────┘  └──────┬──────┘  │
└─────────────┼──────────────────────┼─────────────────────┼────────┘
              │                      │                    │
              ▼                      ▼                    ▼
┌─────────────────────────────────────────────────────────────┐
│                      Application Layer                          │
│   ┌─────────────────────────────────────────────────────────┐│
│   │  Use Cases / Services (Business Logic)                    ││
│   │  - Patient Service, Encounter Service, Note Service       ││
│   │  - AI Service, Auth Service, Audit Service               ││
│   │  - Document Service, Transcription Service              ││
│   │  - Template Service, Code Service                         ││
│   └─────────────────────────────────────────────────────────┘│
│   ┌─────────────────────────────────────────────────────────┐│
│   │  Ports (Traits/Interfaces)                                  ││
│   │  - Repository traits, AI engine traits, Blob store traits  ││
│   │  - Auth traits, Audit traits                               ││
│   └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        Domain Layer                              │
│   ┌─────────────────────────────────────────────────────────┐│
│   │  Domain Models (Pure Rust, No Dependencies)                ││
│   │  - Patient, Encounter, Note, SOAP, Medication, Allergy     ││
│   │  - VitalSigns, Transcription, Job, Consent, Identifier     ││
│   │  - Provenance, SourceRef, AuditEvent                       ││
│   └─────────────────────────────────────────────────────────┘│
│   ┌─────────────────────────────────────────────────────────┐│
│   │  Domain Logic & Validation                                  ││
│   │  - State machines (Note, Encounter, Job)                   ││
│   │  - Validation rules, business invariants                   ││
│   │  - Error types (DomainError)                               ││
│   └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Infrastructure Layer                        │
│   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐  │
│   │  PostgreSQL      │  │  Filesystem      │  │  AI Engines  │  │
│   │  (SQLx)          │  │  Blob Store      │  │  (Ollama,    │  │
│   │                 │  │                 │  │   etc.)      │  │
│   └────────┬────────┘  └────────┬────────┘  └──────┬──────┘  │
└─────────────┼──────────────────────┼─────────────────────┼────────┘
              │                      │                    │
              ▼                      ▼                    ▼
┌─────────────────────────────────────────────────────────────┐
│                      External Systems                            │
│   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐  │
│   │  PostgreSQL      │  │  Local Filesystem│  │  LLM Models  │  │
│   │  Database        │  │                 │  │  (Local)     │  │
│   └─────────────────┘  └─────────────────┘  └─────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Dependency Rules

| Layer | Can Depend On | Cannot Depend On |
|-------|---------------|------------------|
| **Domain** | Nothing (pure Rust) | Any other layer |
| **Application** | Domain | Infrastructure, API, CLI |
| **Infrastructure** | Domain, Application ports | API, CLI |
| **API** | Domain, Application, Infrastructure | CLI |
| **CLI** | Domain, Application, Infrastructure | API |

---

## Crate Structure

The project is organized as a **Cargo workspace** with 5 crates:

```
medassist-oss/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── medassist-domain/         # Domain models and logic
│   ├── medassist-application/    # Use cases and ports
│   ├── medassist-infrastructure/ # Concrete implementations
│   ├── medassist-api/            # HTTP API (Axum)
│   └── medassist-cli/            # Command-line tool
├── migrations/                   # Database migrations
├── docs/                         # Documentation
└── .github/                      # CI/CD workflows
```

### 1. medassist-domain

**Purpose:** Core domain models and business logic, completely framework-free.

**Dependencies:** None (pure Rust 2021)

**Key Types:**
- `Patient`, `Encounter`, `Note`, `SoapContent`, `NoteBody`
- `Medication`, `Allergy`, `VitalSigns`, `Transcription`
- `Job`, `Consent`, `Identifier`, `Provenance`, `SourceRef`
- `AuditEvent`, `DomainError`

**Key Features:**
- State machines for Note, Encounter, Job
- Validation logic with `DomainError`
- Provenance tracking for all clinical data
- Anti-hallucination guardrails for AI-generated content

### 2. medassist-application

**Purpose:** Application services (use cases) and port definitions (traits).

**Dependencies:**
- `medassist-domain` (domain models)
- `thiserror`, `anyhow` (error handling)
- `async-trait` (async traits in Rust 2021)

**Key Types:**
- `PatientService`, `EncounterService`, `NoteService`
- `AiService`, `AuthService`, `AuditService`
- `DocumentService`, `TranscriptionService`
- `TemplateService`, `CodeService`
- `AppError` (application-level errors)

**Key Features:**
- Business logic orchestration
- Port definitions (traits) for infrastructure
- Error translation from domain to application
- Transaction management (via repository traits)

### 3. medassist-infrastructure

**Purpose:** Concrete implementations of application ports.

**Dependencies:**
- `medassist-domain` (domain models)
- `medassist-application` (port traits)
- `sqlx` + `tokio` (PostgreSQL async)
- `argon2` + `rand` (password hashing)
- `reqwest` (HTTP client for AI providers)
- `sha2` (document integrity hashing)

**Key Types:**
- `PgPatients`, `PgEncounters`, `PgNotes` (PostgreSQL repositories)
- `PgUsers`, `PgSessions`, `PgAudit` (auth and audit)
- `PgDocuments`, `PgTranscriptions` (document and transcription)
- `BlobFs` (filesystem blob store)
- `AiOllama` (Ollama adapter)
- `AiDouble` (test double for CI)
- `Db` (PostgreSQL connection pool wrapper)

**Key Features:**
- SQLx-based PostgreSQL repositories
- Filesystem blob storage with path sanitization
- AI engine adapters (Ollama, test double)
- Database migration runner

### 4. medassist-api

**Purpose:** HTTP API server using Axum.

**Dependencies:**
- `medassist-domain` (domain models)
- `medassist-application` (services)
- `medassist-infrastructure` (repositories, blob store)
- `axum` + `tower` + `tower-http` (web framework)
- `serde` + `serde_json` (serialization)
- `multer` (multipart uploads)

**Key Types:**
- `AppState` (application state with services)
- `ApiError` (HTTP error envelope)
- `ApiResult<T>` (HTTP result type)

**Key Features:**
- RESTful API endpoints
- Health checks (`/healthz`, `/readyz`)
- Authentication and authorization
- Request tracing middleware
- Error handling with typed responses

**Endpoints:**
- `GET /healthz` - Liveness probe
- `GET /readyz` - Readiness probe (with DB check)
- `POST /login` - User login
- `POST /logout` - User logout
- `GET /sessions/me` - Current session info
- `GET /patients` - List patients
- `POST /patients` - Create patient
- `GET /patients/:id` - Get patient
- `PATCH /patients/:id` - Update patient
- `DELETE /patients/:id` - Delete patient
- `GET /patients/:id/encounters` - List encounters
- `POST /patients/:id/encounters` - Create encounter
- `POST /patients/:id/notes` - Create note
- `GET /patients/:id/notes` - List notes
- `GET /notes/:id` - Get note
- `POST /notes/:id/review` - Review note
- `POST /notes/:id/finalize` - Finalize note
- `POST /notes/:id/amend` - Amend note
- `GET /documents` - List documents
- `POST /documents/upload` - Upload document
- `GET /documents/:id/download` - Download document
- `POST /transcriptions/consent` - Request transcription consent
- `POST /transcriptions/start` - Start transcription
- `POST /transcriptions/:id/cancel` - Cancel transcription
- `POST /transcriptions/:id/review` - Review transcription
- `POST /ai/draft` - Generate AI draft
- `GET /system/capabilities` - Get system capabilities
- `GET /system/audit` - Get audit log
- `GET /system/templates` - List templates
- `GET /system/codes` - Search codes

### 5. medassist-cli

**Purpose:** Command-line administration tool.

**Dependencies:**
- `medassist-domain` (domain models)
- `medassist-application` (services)
- `medassist-infrastructure` (repositories, DB)
- `clap` (argument parsing)
- `dotenvy` (environment variable loading)
- `tracing` + `tracing-subscriber` (logging)

**Key Commands:**
- `medassist migrate` - Apply database migrations
- `medassist setup` - Create database and apply migrations
- `medassist create-admin` - Create administrator user
- `medassist reset-password` - Reset user password
- `medassist check-db` - Verify database connection

---

## Design Decisions

### 1. Multi-Tenant Isolation

**Decision:** Every clinical table has an `org_id` column, and all queries filter by it.

**Rationale:**
- Security-by-design: prevent cross-organization data leakage
- Compliance: meet data isolation requirements
- Scalability: support multiple organizations on a single instance

**Implementation:**
- PostgreSQL schema with `org_id` UUID FK on all clinical tables
- Application layer enforces org_id filtering
- API layer validates org_id from session

### 2. Provenance Tracking

**Decision:** Every field carries provenance metadata (`ProvenanceKind` and `SourceRef`).

**Rationale:**
- Audit trail: track source of every piece of clinical data
- Prevent AI from silently overwriting human edits
- Support human-in-the-loop workflow

**Provenance Kinds:**
- `Human` - Entered directly by a human
- `AiSuggestion` - Generated by AI, requires human review
- `Imported` - Imported from external system
- `Transcription` - Transcribed from audio

### 3. Human-in-the-Loop

**Decision:** AI output is always treated as a draft that requires human review before finalization.

**Rationale:**
- Clinical safety: AI is not trusted to produce verified clinical information
- Legal compliance: human oversight is required for medical documentation
- Quality assurance: catch AI errors before they affect patient care

**Implementation:**
- `reviewed: false` flag on AI-generated content
- `finalize()` operation requires `reviewed: true`
- Audit log tracks who reviewed and when

### 4. Anti-Hallucination Guardrails

**Decision:** Validate AI output against authorized input to prevent fabricated data.

**Rationale:**
- Prevent AI from inventing quantities, dates, and facts
- Ensure AI output is grounded in actual patient data
- Maintain clinical accuracy

**Implementation:**
- `validate_draft()` function in AI service
- Checks numeric values against input ranges
- Checks dates against input date ranges
- Rejects output with unverified claims

### 5. Immutable Originals

**Decision:** Original documents and notes are never modified; only amended.

**Rationale:**
- Audit integrity: maintain complete history of changes
- Legal compliance: meet record-keeping requirements
- Transparency: show exactly what was changed and when

**Implementation:**
- SHA-256 hashing of all uploaded documents
- Immutable blob storage (filesystem)
- Version history for notes with amendment links
- Optimistic concurrency control with version numbers

### 6. Optimistic Concurrency Control

**Decision:** Use version numbers to prevent silent overwrites.

**Rationale:**
- Prevent lost updates in collaborative environments
- Detect conflicts when multiple users edit the same record
- Maintain data integrity

**Implementation:**
- `version: u64` field on mutable entities
- `expected_version: Option<u64>` parameter for updates
- Conflict detection in repository layer

### 7. State Machines

**Decision:** Model lifecycle as explicit state machines.

**Rationale:**
- Prevent invalid state transitions
- Make valid transitions explicit and type-safe
- Catch bugs at compile time

**State Machines:**
- **Note:** Draft → Review → Finalized → Amended
- **Encounter:** Open → InProgress → Finalized → Cancelled
- **Job:** Pending → Running → Completed → Failed → Cancelled
- **Transcription:** PendingConsent → Consented → InProgress → Completed → Cancelled

### 8. Error Handling Strategy

**Decision:** Use typed errors with clear categorization.

**Rationale:**
- Type safety: catch error handling bugs at compile time
- Debuggability: provide context for failures
- API consistency: return appropriate HTTP status codes

**Error Hierarchy:**
- `DomainError` - Domain validation failures (400 Bad Request)
- `AppError` - Application-level failures (wraps DomainError)
- `ApiError` - HTTP error envelope with status code
- `InfrastructureError` - Database, filesystem, network failures (500 Internal Server Error)

---

## Data Flow Patterns

### 1. API Request Flow

```
HTTP Request
    │
    ▼
┌─────────────────────┐
│  Axum Router         │  ← Route to handler
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  Middleware          │  ← Auth, tracing, CORS
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  Handler             │  ← Parse request, call service
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  Application Service │  ← Orchestrate use case
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  Repository          │  ← Persist data
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  PostgreSQL          │  ← Store data
└─────────────────────┘
```

### 2. CLI Command Flow

```
CLI Command
    │
    ▼
┌─────────────────────┐
│  Clap Parser          │  ← Parse arguments
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  Command Handler     │  ← Execute command
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  Infrastructure       │  ← Use repositories, DB
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  PostgreSQL          │  ← Execute queries
└─────────────────────┘
```

---

## Security Architecture

### Authentication
- **Algorithm:** Argon2id (memory-hard, resistant to GPU attacks)
- **Storage:** Password hashes only, never plaintext
- **Parameters:** Default Argon2id parameters (safe defaults)

### Authorization
- **Model:** Role-based access control (RBAC)
- **Roles:** admin, clinician, patient (planned)
- **Enforcement:** API layer validates permissions

### Session Management
- **Storage:** Opaque tokens in PostgreSQL
- **Expiration:** Configurable session timeout
- **Lockout:** Failed login attempts trigger temporary lockout

### Data Isolation
- **Strategy:** Row-level security via `org_id` filtering
- **Enforcement:** Application layer filters all queries by org_id
- **Validation:** API layer validates org_id from session

### Input Validation
- **Strategy:** Validate all inputs at domain layer
- **Implementation:** `DomainError::Validation` for invalid inputs
- **Scope:** All external inputs (API, CLI, file uploads)

---

## Privacy Architecture

### Data Minimization
- **Principle:** Collect only necessary data
- **Implementation:** Minimal required fields on all models
- **Storage:** No redundant data storage

### Data Retention
- **Strategy:** Configurable retention policies
- **Implementation:** Soft deletion with retention periods
- **Audit:** All deletions logged in audit trail

### Encryption
- **At Rest:** PostgreSQL encryption (delegated to database)
- **In Transit:** TLS for all HTTP connections
- **Secrets:** Environment variables, never in code

### Access Logging
- **Strategy:** Log all sensitive access
- **Implementation:** Audit log for clinical data access
- **Scope:** Read, write, delete operations on clinical data

---

## AI Integration Architecture

### AI Engine Abstraction

```
┌─────────────────────────────────────────────────────────────┐
│                    AI Service (Application)                      │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  AiService Trait                                          ││
│  │  - generate_draft()                                       ││
│  │  - summarize()                                            ││
│  │  - extract_structured_data()                              ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  Ollama Adapter  │ │  Test Double     │ │  Other Adapters  │
│  (Local LLM)     │ │  (Deterministic) │ │  (Planned)       │
│                 │ │                 │ │                 │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

### Guardrails

1. **Anti-Hallucination:**
   - Validate AI output against input
   - Reject unverified claims
   - Ground all numbers and dates in input

2. **Provenance Tracking:**
   - Mark all AI output as `ProvenanceKind::AiSuggestion`
   - Track source model and version
   - Require human review before finalization

3. **Safety Filters:**
   - Remove PII from AI prompts
   - Filter sensitive data from AI input
   - Sanitize AI output before display

4. **Rate Limiting:**
   - Limit AI requests per user/organization
   - Queue requests during high load
   - Timeout long-running requests

---

## Deployment Architecture

### Single Instance Deployment

```
┌─────────────────────────────────────────────────────────────┐
│                        Host Machine                              │
│                                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │  MedAssist   │    │  PostgreSQL  │    │  Ollama / LLM    │  │
│  │  API Server  │───▶│  Database    │    │  (Optional)      │  │
│  │  (Axum)      │    │             │    │                 │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                    Reverse Proxy                           ││
│  │  (Nginx, Traefik, Caddy) - TLS termination, rate limiting  ││
│  └─────────────────────────────────────────────────────────┘│
│                                                                  │
└─────────────────────────────────────────────────────────────┘
```

### Containerized Deployment

```
┌─────────────────────────────────────────────────────────────┐
│                        Docker / Podman                           │
│                                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │ medassist-  │    │ postgres:14  │    │ ollama:latest   │  │
│  │ api         │───▶│             │    │                 │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                    Reverse Proxy                           ││
│  │  (Nginx, Traefik) - TLS, rate limiting, routing           ││
│  └─────────────────────────────────────────────────────────┘│
│                                                                  │
└─────────────────────────────────────────────────────────────┘
```

### Kubernetes Deployment (Future)

```
┌─────────────────────────────────────────────────────────────┐
│                        Kubernetes Cluster                        │
│                                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │ medassist-  │    │ postgres     │    │ ollama           │  │
│  │ api         │───▶│ (StatefulSet)│    │ (Deployment)     │  │
│  │ (Deployment)│    │             │    │                 │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│           │                                                 │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Ingress Controller (TLS termination, routing)            ││
│  └─────────────────────────────────────────────────────────┘│
│                                                                  │
└─────────────────────────────────────────────────────────────┘
```

---

## Configuration

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `DATABASE_URL` | PostgreSQL connection URL | - | Yes |
| `RUST_LOG` | Logging level | `info` | No |
| `SECRET_KEY` | Session signing key | - | Yes |
| `MAX_CONNECTIONS` | DB connection pool size | 10 | No |
| `OLLAMA_URL` | Ollama API URL | `http://localhost:11434` | No |
| `PORT` | API server port | 3000 | No |
| `HOST` | API server host | `0.0.0.0` | No |

### Configuration Files

- `.env` - Local development environment variables
- `.env.example` - Template for environment variables
- `rust-toolchain.toml` - Rust toolchain version
- `deny.toml` - License audit configuration

---

## Performance Considerations

### Database Performance
- **Indexes:** Justified indexes on frequently queried columns
- **Connection Pooling:** SQLx connection pool with configurable size
- **Query Optimization:** Use `sqlx::query!` for compile-time query validation
- **Batch Operations:** Use batch inserts for bulk operations

### API Performance
- **Rate Limiting:** Tower rate limiting middleware
- **Caching:** Future: Redis caching for frequently accessed data
- **Compression:** Gzip compression for responses
- **Pagination:** Paginated responses for large datasets

### AI Performance
- **Local Inference:** Ollama for local LLM inference
- **Batching:** Batch AI requests when possible
- **Caching:** Cache AI responses for similar prompts
- **Timeout:** Configurable timeout for AI requests

---

## Testing Strategy

### Unit Tests
- **Scope:** Domain and application layers
- **Framework:** Rust's built-in test framework
- **Coverage:** All domain validation and business logic

### Integration Tests
- **Scope:** Infrastructure and API layers
- **Framework:** SQLx test utilities, reqwest for HTTP tests
- **Environment:** Requires PostgreSQL database

### End-to-End Tests
- **Scope:** Full application flow
- **Framework:** Custom test harness
- **Environment:** Requires full deployment

### Test Coverage Goals
- **Domain Layer:** 100% of validation logic
- **Application Layer:** 100% of use cases
- **Infrastructure Layer:** 80% of repository methods
- **API Layer:** 80% of endpoints

---

## Monitoring and Observability

### Logging
- **Framework:** tracing + tracing-subscriber
- **Levels:** ERROR, WARN, INFO, DEBUG, TRACE
- **Format:** JSON for structured logging
- **Output:** stdout (containerized) or file

### Metrics
- **Framework:** Future: Prometheus metrics endpoint
- **Metrics:** Request count, error rate, latency, DB query time
- **Endpoint:** `/metrics` (future)

### Tracing
- **Framework:** OpenTelemetry (future)
- **Scope:** Request tracing across services
- **Export:** Jaeger, Zipkin, or console

### Health Checks
- **Liveness:** `GET /healthz` - Application is running
- **Readiness:** `GET /readyz` - Application is ready to serve requests (DB check)
- **Startup:** Application starts successfully

---

## Future Architecture Evolution

### 1. Web UI (Leptos)
- **Status:** Not started
- **Priority:** Medium
- **Dependencies:** Leptos, WebAssembly
- **Integration:** Share domain models with API

### 2. Desktop (Tauri)
- **Status:** Not started
- **Priority:** Low
- **Dependencies:** Tauri, Rust
- **Integration:** Package API + Web UI as desktop app

### 3. FHIR Integration
- **Status:** Not started
- **Priority:** Medium
- **Dependencies:** FHIR Rust crate
- **Integration:** Mapping between domain models and FHIR resources

### 4. RAG Retrieval
- **Status:** Not started
- **Priority:** Medium
- **Dependencies:** Embedding model, vector database
- **Integration:** Semantic search in documents and notes

### 5. OCR Adapters
- **Status:** Not started
- **Priority:** Medium
- **Dependencies:** Tesseract, EasyOCR, or commercial OCR
- **Integration:** Document scanning and text extraction

### 6. Audio Capture
- **Status:** Not started
- **Priority:** Medium
- **Dependencies:** Web Audio API, MediaRecorder
- **Integration:** Browser-based recording and upload

---

## References

- [Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html) - Robert C. Martin
- [Hexagonal Architecture](https://alistair.cockburn.us/Hexagonal+architecture) - Alistair Cockburn
- [Domain-Driven Design](https://domainlanguage.com/ddd/) - Eric Evans
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/) - Rust Community

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-16 | MedAssist OSS Contributors | Initial architecture documentation |
