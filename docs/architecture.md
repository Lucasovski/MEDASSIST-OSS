# MedAssist OSS - Architecture Documentation

**License:** GPL-3.0-or-later  
**Version:** 0.1.0  
**Last Updated:** 2026-10-16

---

## Overview

MedAssist OSS follows a **clean/hexagonal architecture** pattern with strict layer separation. The system is designed for:

- **Privacy-by-design**: Multi-tenant isolation, provenance tracking, human-in-the-loop for AI
- **Modularity**: Each crate has a single responsibility with clear boundaries
- **Testability**: Framework-free domain, dependency injection via traits
- **Self-hosting**: Local-first execution, no external dependencies required for core functionality

---

## Crate Layering

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI (medassist-cli)                         │
│              Command-line administration tool                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      API (medassist-api)                           │
│           HTTP transport layer (Axum) + Auth + Middleware         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Application (medassist-application)                 │
│  Use cases + Business logic + Ports (traits) + Service orchestration│
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────────────┼───────────────────────┐
              ▼                       ▼                       ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│   Domain         │ │ Infrastructure   │ │   External        │
│ (medassist-domain)│ │ (medassist-infra) │ │   Providers      │
│  - Models        │ │  - SQLx Repos    │ │   - Ollama       │
│  - Validation    │ │  - Blob Store    │ │   - Test Double  │
│  - Invariants    │ │  - AI Adapters   │ │   (for CI)       │
└──────────────────┘ └──────────────────┘ └──────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    PostgreSQL + Filesystem                         │
│              Persistence layer (SQLx + PostgreSQL)                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Dependency Rules

### 1. Domain Crate (`medassist-domain`)

**Purpose:** Pure business domain models and invariants.

**Dependencies:**
- ✅ `chrono` - Date/time handling
- ✅ `serde` - Serialization
- ✅ `thiserror` - Error types
- ✅ `uuid` - Unique identifiers
- ❌ NO `axum`, `sqlx`, `tokio`, `reqwest`, or any framework/IO types

**Responsibilities:**
- Clinical entities (Patient, Encounter, Note, Medication, Allergy, VitalSigns, etc.)
- Value objects and validation
- Domain errors (`DomainError`)
- Provenance tracking
- State machines (Note status, Encounter status, etc.)

**Key Types:**
```rust
// Entities
Patient, Encounter, Note, Medication, Allergy, VitalSigns
Transcription, Job, Consent, ClinicalDocument

// Value Objects
ExternalIdentifier, Provenance, SourceRef, ProvenanceKind
NoteStatus (Draft, InReview, Finalised, Amended)
EncounterStatus

// Errors
DomainError (Validation, NotFound, Conflict, etc.)
```

---

### 2. Application Crate (`medassist-application`)

**Purpose:** Use cases, business logic, and ports (interfaces/traits).

**Dependencies:**
- ✅ `medassist-domain` - Domain models
- ✅ `async-trait` - Async trait support
- ✅ `argon2` - Password hashing
- ✅ `chrono` - Date/time
- ✅ `serde` / `serde_json` - Serialization
- ✅ `sha2` - Hashing for document integrity
- ✅ `tracing` - Observability
- ✅ `uuid` - Identifiers
- ❌ NO `axum`, `sqlx`, `tokio` (except in dev-dependencies for tests)

**Responsibilities:**
- **Ports (Traits):** Define interfaces for external dependencies
  - `PatientRepository`, `EncounterRepository`, `NoteRepository`
  - `DocumentRepository`, `JobRepository`, `UserRepository`
  - `SessionRepository`, `AuditSink`, `BlobStore`
  - `ClinicalDocumentationEngine`, `LanguageModelProvider`
  - `TranscriptionEngine`

- **Services:** Implement use cases using domain models
  - `PatientService` - CRUD, search, duplicate detection
  - `EncounterService` - Lifecycle management
  - `NoteService` - Create, edit, finalize, amend with conflict detection
  - `AuthService` - Authentication, sessions, password policies
  - `DocumentService` - Upload, validation, hashing, metadata
  - `TranscriptionService` - Audio transcription orchestration
  - `AuditService` - Event recording
  - `AiService` - AI-assisted documentation with guardrails

- **Guardrails:**
  - Anti-hallucination validation for AI output
  - Provenance enforcement (AI can never overwrite human edits)
  - Human review mandatory before finalization

**Key Patterns:**
```rust
// Port definition
pub trait PatientRepository: Send + Sync {
    async fn create(&self, actor: &Actor, input: NewPatient, now: DateTime<Utc>) 
        -> Result<Patient, AppError>;
    async fn get(&self, actor: &Actor, id: Uuid) 
        -> Result<Patient, AppError>;
    // ...
}

// Service using port
pub struct PatientService<R: PatientRepository> {
    repo: R,
    // ...
}
```

---

### 3. Infrastructure Crate (`medassist-infrastructure`)

**Purpose:** Concrete implementations of ports (adapters).

**Dependencies:**
- ✅ `medassist-domain` - Domain models
- ✅ `medassist-application` - Ports and services
- ✅ `sqlx` - PostgreSQL access
- ✅ `tokio` - Async runtime
- ✅ `reqwest` - HTTP client for external AI providers
- ✅ All other concrete dependencies

**Responsibilities:**
- **SQLx Repositories:** PostgreSQL implementations of all repository traits
  - `PgPatientRepository`, `PgNoteRepository`, etc.
  - Parameterized queries, no raw SQL string interpolation

- **Blob Storage:**
  - `FsBlobStore` - Filesystem-based immutable blob storage
  - Path sanitization to prevent directory traversal
  - SHA-256 integrity verification

- **AI Engines:**
  - `AiTestDouble` - Deterministic test implementation for CI
  - `OllamaEngine` - HTTP adapter for local Ollama inference
  - Timeout and cancellation support

**Key Implementations:**
```rust
// PostgreSQL repository
pub struct PgPatientRepository(sqlx::PgPool);

#[async_trait]
impl PatientRepository for PgPatientRepository {
    async fn create(&self, actor: &Actor, input: NewPatient, now: DateTime<Utc>) 
        -> Result<Patient, AppError> {
        // Parameterized SQLx query
        let row = sqlx::query!(
            r#"INSERT INTO patients (...) VALUES (...) RETURNING *"#,
            // ... parameters
        ).fetch_one(&self.0).await?;
        // ... map to domain model
    }
}

// Filesystem blob store
pub struct FsBlobStore { base_path: PathBuf };

impl BlobStore for FsBlobStore {
    async fn store(&self, id: Uuid, bytes: Vec<u8>) -> Result<PathBuf, AppError> {
        // Sanitize path, write immutably
    }
}
```

---

### 4. API Crate (`medassist-api`)

**Purpose:** HTTP transport layer (thin adapter over application services).

**Dependencies:**
- ✅ `medassist-domain` - Domain models (for DTO conversion)
- ✅ `medassist-application` - Services and ports
- ✅ `medassist-infrastructure` - Repository implementations
- ✅ `axum` - Web framework
- ✅ `tower` / `tower-http` - Middleware
- ✅ `serde` / `serde_json` - JSON serialization
- ✅ `multer` - Multipart upload handling

**Responsibilities:**
- **Routing:** Define HTTP routes and map to service methods
- **Request/Response Handling:**
  - Parse requests into DTOs
  - Convert domain models to JSON responses
  - Error handling with typed `ApiError` envelope
- **Authentication:**
  - `Authed` extractor for Bearer token / cookie auth
  - Session validation via `FromRequestParts`
- **Middleware:**
  - Request tracing
  - CORS configuration
  - Rate limiting (configured)
- **State Management:**
  - `AppState` containing all service instances
  - Dependency injection via Axum's `State` extractor

**Key Components:**
```rust
// Router definition
pub fn api_v1(st: AppState) -> Router<AppState> {
    Router::new()
        .route("/patients", get(list_patients).post(create_patient))
        .route("/patients/:id", get(get_patient).patch(update_patient))
        // ... more routes
        .with_state(st)
}

// Handler example
async fn create_patient(
    Authed(actor): Authed,      // Extracted from request
    State(st): State<AppState>,  // Injected state
    Json(dto): Json<NewPatientDto>,
) -> Result<Json<Patient>, ApiError> {
    let p = st.patients.create(&actor, dto.into(), now()).await?;
    Ok(Json(p))
}
```

---

### 5. CLI Crate (`medassist-cli`)

**Purpose:** Command-line administration tool.

**Dependencies:**
- ✅ `medassist-domain` - Domain models
- ✅ `medassist-application` - Services
- ✅ `medassist-infrastructure` - Repositories
- ✅ `tokio` - Async runtime
- ✅ `sqlx` - Database migrations

**Responsibilities:**
- **Database Management:**
  - `cargo run -p medassist-cli -- migrate` - Apply migrations
  - `cargo run -p medassist-cli -- create-admin` - Bootstrap first admin (future)
- **Maintenance:**
  - Backup/export utilities (future)
  - Configuration validation (future)

---

## Data Flow Patterns

### 1. Request Flow (API)

```
HTTP Request
     │
     ▼
┌─────────────────┐
│  Middleware      │  ← Tracing, CORS, Rate Limiting
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  Extractor       │  ← Authed (from token/cookie)
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  Handler         │  ← Route handler in routes.rs
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  Service         │  ← Application layer use case
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  Repository      │  ← Infrastructure (PostgreSQL)
└─────────────────┘
     │
     ▼
  Database
```

### 2. Provenance Flow

```
User Input / Upload
     │
     ▼
┌─────────────────┐
│  Provenance      │  ← Tagged as ProvenanceKind::Human
│  SourceRef       │  ← SourceRef::Human { user_id }
└─────────────────┘
     │
     ▼
  Stored in Domain Model
     │
     ▼
  Every field carries its provenance
     │
     ▼
  AI suggestions carry ProvenanceKind::AiSuggestion
     │
     ▼
  Human review required before finalization
```

---

## Multi-Tenant Isolation

### Implementation

Every clinical table in PostgreSQL has:
- `org_id UUID NOT NULL` - Organization identifier
- Foreign key constraints enforcing org membership
- Indexes on `(org_id, ...)` for efficient querying

### Enforcement

1. **Repository Layer:** All queries include `WHERE org_id = ?`
2. **Service Layer:** Actor's `org_id` is passed to all operations
3. **API Layer:** `Authed` extractor ensures authenticated user has valid org
4. **Cross-Org Tests:** Application services include tests verifying cross-org access is denied

```rust
// Example: Patient search always filtered by org
async fn search(&self, actor: &Actor, q: PatientQuery) -> Result<Page<Patient>, AppError> {
    let rows = sqlx::query!(
        "SELECT * FROM patients WHERE org_id = $1 AND (...) LIMIT $2 OFFSET $3",
        actor.org_id,  // Always filtered by actor's org
        q.limit,
        q.offset
    ).fetch_all(&self.0).await?;
    // ...
}
```

---

## Security Architecture

### 1. Authentication

- **Password Hashing:** Argon2id with configurable parameters
- **Session Tokens:** Opaque, expiring, revocable UUIDs
- **Lockout:** Failed attempt counter with configurable threshold
- **Token Storage:** Bearer token in Authorization header or HttpOnly cookie

### 2. Authorization

- **Multi-Tenant:** Org-based isolation enforced server-side
- **Role-Based:** Actor carries roles for future RBAC
- **Resource-Level:** Ownership checked at service layer

### 3. Data Protection

- **No Plaintext Secrets:** Passwords never stored in plaintext
- **No Clinical Data in Logs:** Audit events exclude full clinical content
- **Provenance Tracking:** Every field tagged with source and kind
- **Immutable Originals:** Documents and blobs are write-once

### 4. Input Validation

- **Domain Layer:** All models validate on construction
- **Service Layer:** Business rules enforced before persistence
- **API Layer:** DTOs validated before processing

---

## AI Safety Architecture

### 1. Guardrails

- **Anti-Hallucination:** Numeric quantities and dates in AI output must be grounded in input
- **Provenance:** All AI output tagged as `ProvenanceKind::AiSuggestion`
- **Review Required:** `reviewed: false` flag prevents auto-finalization
- **Gap Flagging:** Missing information becomes explicit gaps, never invented facts

### 2. Validation Pipeline

```
DocumentationRequest (authorized context + transcript)
     │
     ▼
┌─────────────────┐
│  Language Model  │  ← Generate raw draft text
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  validate_draft() │  ← Check numbers/dates against input
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  NoteBody::Soap  │  ← With AI provenance, reviewed=false
└─────────────────┘
     │
     ▼
  Human Review Required
```

### 3. Engine Abstraction

```rust
pub trait ClinicalDocumentationEngine: Send + Sync {
    fn id(&self) -> String;
    async fn generate(&self, req: &DocumentationRequest) -> Result<NoteBody, AppError>;
}

// Implementations
pub struct AiTestDouble;      // Deterministic for CI
pub struct OllamaEngine;       // HTTP to local Ollama
```

---

## Error Handling

### Error Types

| Layer | Error Type | Purpose |
|-------|-----------|---------|
| Domain | `DomainError` | Validation, business rule violations |
| Application | `AppError` | Service-level errors, wrapping domain errors |
| API | `ApiError` | HTTP error envelope with status codes |

### Error Flow

```rust
// Domain error
DomainError::Validation { field: "family_name", message: "must not be empty" }
     │
     ▼
// Wrapped in AppError
AppError::Domain(DomainError::Validation { ... })
     │
     ▼
// Converted to ApiError
ApiError::from(AppError::Domain(...))
     │
     ▼
// HTTP Response
StatusCode::BAD_REQUEST, Json(ApiError { code: "validation", message: "..." })
```

---

## Testing Strategy

### 1. Unit Tests (No DB Required)

- **Domain:** 36 tests - Model validation, state machines, provenance
- **Application:** 21 tests - Service logic, guardrails, cross-org isolation
- **Infrastructure:** 3 tests - Repository helpers, AI double, blob store

### 2. Integration Tests (Require PostgreSQL)

- Gated on `DATABASE_URL` environment variable
- CI provisions real PostgreSQL service
- Test full request flow: API → Service → Repository → DB

### 3. Test Coverage Goals

- Domain: 100% of validation logic
- Application: All use cases with happy path + error cases
- API: All routes with auth + error handling
- Cross-org: Every write operation tested against wrong-org actor

---

## Configuration

### Environment Variables

| Variable | Purpose | Required |
|----------|---------|----------|
| `DATABASE_URL` | PostgreSQL connection string | Yes (for runtime) |
| `MEDASSIST_BIND` | HTTP server address | Yes |
| `MEDASSIST_ENV` | Environment (development/production) | Yes |
| `MEDASSIST_BLOB_DIR` | Blob storage directory | Yes |
| `MEDASSIST_LLM_ID` | AI model identifier | No (optional) |
| `MEDASSIST_LLM_EXTERNAL` | Whether AI sends data externally | No (default: false) |

### Security Defaults

- **Development mode:** Allows insecure defaults for local development
- **Production mode:** Refuses to start without proper configuration

---

## Deployment Considerations

### 1. Database

- PostgreSQL >= 14
- Migrations applied via `sqlx database setup` or CLI
- Recommended: Separate database for each environment

### 2. Storage

- Blob store: Local filesystem (configurable path)
- Future: S3-compatible storage support

### 3. AI Providers

- **Local-first:** Ollama adapter for local inference
- **Optional:** Can configure remote providers (not recommended for clinical data)
- **Default:** Test double for CI, no external calls

### 4. TLS

- Not handled by application (use reverse proxy)
- PostgreSQL connections should use TLS in production

---

## Future Architecture Evolution

### Planned Additions

1. **Web UI (Leptos):**
   - Separate crate or workspace member
   - Communicates with API via HTTP
   - Real-time updates via WebSockets (future)

2. **Desktop (Tauri):**
   - Optional packaging for desktop distribution
   - Embeds API server or connects to remote

3. **FHIR Integration:**
   - Mapping layer between domain models and FHIR resources
   - Export/import capabilities

4. **RAG Retrieval:**
   - Vector database integration (local or external)
   - Embedding models for semantic search
   - Clinical knowledge base integration

5. **OCR Adapters:**
   - Document scanning and text extraction
   - Integration with transcription pipeline

6. **Audio Capture:**
   - Browser-based recording
   - Streaming to transcription service

---

## References

- [Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html)
- [Hexagonal Architecture](https://alistair.cockburn.us/Hexagonal-architecture/)
- [Domain-Driven Design](https://domainlanguage.com/ddd/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
