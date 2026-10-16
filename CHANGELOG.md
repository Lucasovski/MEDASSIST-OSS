# CHANGELOG — MedAssist OSS

Project format: Keep a Changelog; this software follows Semantic Versioning.
All notable changes are documented here. Status labels distinguish what is
**implemented**, **compiled**, **tested** and **validated in execution**.

## [Unreleased] — development snapshot (2026-10-16)

### Added
- Cargo workspace with 5 crates: `medassist-domain`, `medassist-application`,
  `medassist-infrastructure`, `medassist-api`, `medassist-cli`.
- **Domain crate** (compiled + 36/36 unit tests passing): Patient (stable UUID
  identity, external identifiers, never name-only matching), Encounter, SOAP
  Note model with draft/review/finalize/amend state machine, version chain with
  optimistic-concurrency conflict detection, Provenance tagging (human vs AI vs
  imported), Medication, Allergy, VitalSigns, Transcription (segments +
  timestamps + consent), Job state machine, Consent record, Identifier types.
- **Application crate** (compiled + 21/21 unit tests passing incl. cross-org
  isolation for documents): ports (traits) for repositories, auth, AI engines;
  use-cases for patients, encounters, notes (save/draft/finalize with human
  edits never silently overwritten by async AI), auth service (Argon2id, lockout
  after failed attempts, opaque expiring session tokens, revocation), audit
  service, transcription orchestration, document import with size/type
  validation and SHA-256 hashing, template registry, AI anti-hallucination
  guardrails (unknown fields become flagged gaps, never invented facts).
- **Infrastructure crate** (compiles): SQLx/PostgreSQL repositories (patients,
  notes, documents, jobs, users, sessions, audit), filesystem blob store with
  path sanitization + immutable writes, deterministic test-double AI engine
  (clearly labeled as a double, not a real model), Ollama HTTP adapter skeleton
  with timeouts/cancellation.
- PostgreSQL migration `migrations/20261017000001_schema.sql`: multi-tenant
  schema with `org_id` on every clinical table, FK constraints, justified
  indexes, UTC timestamptz columns.
- Axum API skeleton: typed error envelope (`ApiError`), health/readiness
  endpoints, request tracing middleware, state wiring.
- CLI crate skeleton.
- Licensing & governance: full GPL-3.0 text (`LICENSE`), `README.md`,
  `NOTICE.md`, `THIRD_PARTY_LICENSES.md`, `SECURITY.md`, `CONTRIBUTING.md`,
  `.env.example`, `CHANGELOG.md`.
- CI: GitHub Actions workflow (fmt/check/clippy -D warnings/tests with real
  Postgres service) + `cargo-deny` config (`deny.toml`).

### Fixed
- Workspace manifest errors (missing api/cli crate manifests, sqlx feature
  misconfiguration) that prevented compilation.

### Known limitations / not yet done
- API routes beyond health checks return 501-style typed "not implemented"
  errors; CRUD endpoints over the application services are pending.
- Integration tests against live PostgreSQL written but gated on DATABASE_URL;
  executed locally in dev environment only at time of writing.
- Leptos web UI, Tauri desktop app, FHIR mapping, RAG retrieval, OCR adapters,
  CID-10 tables, audio capture frontend: not started.
- No third-party security audit performed. Clinical safety validation not
  performed — this tool supports documentation and does not replace
  professional judgment.
