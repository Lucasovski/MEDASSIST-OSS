# MedAssist OSS

**Open-source, self-hostable, AI-assisted clinical documentation.**
Rust-first backend (Axum + SQLx + PostgreSQL), privacy-by-design, human-in-the-loop
for every AI suggestion. Licensed **GPL-3.0-or-later**.

> ⚠️ Clinical-use disclaimer: MedAssist OSS is a documentation *support* tool.
> It does not diagnose, does not prescribe, and does not replace professional
> judgment. AI output is always an unreviewed draft until a qualified human
> finalizes it. No clinical-safety validation has been performed.

## Status (honest labels)

| Area | Implemented | Compiled | Unit-tested | Integration/execution-validated |
|---|---|---|---|---|
| Domain model (patient, encounter, SOAP note, versions, provenance, meds, allergies, vitals, jobs, consent) | OK | OK | 36/36 passing | pending live DB run |
| Application services (auth Argon2id+lockout, sessions, notes w/ conflict detection, docs w/ hash+limits, audit, AI guardrails) | OK | OK | 21/21 passing | pending live DB run |
| PostgreSQL schema + migrations | OK | OK | -- | gated on `DATABASE_URL` |
| SQLx repositories / blob store / AI test-double / Ollama adapter skeleton | OK | OK | partial | pending live DB run |
| Axum API (health/readiness, typed errors, tracing) | OK | OK | OK | running binary |
| CRUD/clinical endpoints over services | OK | OK | -- | pending live DB run |
| Web UI (Leptos), desktop (Tauri), FHIR, RAG, OCR, audio capture | NOT STARTED | -- | -- | -- |

## Features

- **Patients**: stable UUID identity, external identifiers, duplicate-control
  that never merges on name alone; search with pagination.
- **Encounters & SOAP notes**: draft -> review -> finalize -> amend state
  machine; full version history; optimistic-concurrency edit-conflict detection.
- **Provenance everywhere**: every field tagged human / AI-draft / imported,
  with source references. AI suggestions can never silently overwrite human
  edits and cannot finalize notes autonomously.
- **AI engine abstraction** (`ClinicalDocumentationEngine`,
  `LanguageModelProvider`, `TranscriptionEngine`): local-first (Ollama adapter),
  deterministic test double for CI; anti-hallucination guardrails turn missing
  information into explicit flagged gaps instead of invented facts.
- **Documents/labs**: upload validation (size/type), SHA-256 integrity hashing,
  immutable originals, extracted values linked back to their source document.
- **Multi-tenant isolation** by `org_id` enforced server-side; cross-org access
  attempts are covered by tests.
- **Audit trail**: actor/action/resource/timestamp/outcome without embedding
  full clinical content.
- **Auth**: Argon2id password hashes, failed-attempt lockout, opaque expiring
  revocable session tokens.

## Requirements

- Rust stable (pinned in `rust-toolchain.toml`)
- PostgreSQL >= 14
- Linux/macOS (Windows via WSL supported for development)

## Quick start (development)

```bash
git clone <repo-url> && cd medassist-oss
cp .env.example .env          # edit DATABASE_URL etc.; never commit .env

# Start PostgreSQL (e.g., docker/podman):
docker run -d --name medassist-pg -e POSTGRES_USER=medassist \
  -e POSTGRES_PASSWORD=<choose-a-secret> -e POSTGRES_DB=medassist \
  -p 127.0.0.1:5432:5432 postgres:15-alpine

cargo run -p medassist-cli -- migrate   # applies ./migrations
cargo run -p medassist-api              # http://127.0.0.1:8080
curl http://127.0.0.1:8080/healthz      # liveness
curl http://127.0.0.1:8080/readyz       # readiness incl. DB check
```

### First administrator

There is **no default admin account or universal password**. Bootstrap the
first admin via the CLI once the `create-admin` command is wired; until then
use the documented SQL bootstrap in `docs/deployment.md`. Never ship secrets
in code or logs.

## Tests

```bash
cargo test --workspace                 # unit tests, no DB needed
DATABASE_URL=postgres://... cargo test --workspace -- --include-ignored   # integration
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check licenses              # license allow-list audit
```

## Configuration

All configuration is environment-driven (see `.env.example`):
`DATABASE_URL`, `MEDASSIST_BIND`, `MEDASSIST_ENV` (`development|production`),
`MEDASSIST_BLOB_DIR`. Production refuses to start with development defaults.
AI providers are opt-in: nothing leaves the machine unless an operator
explicitly configures a remote endpoint.

## Documentation

- `docs/architecture.md` — crate layering and dependency rules
- `SECURITY.md` — security posture & vulnerability reporting
- `docs/privacy.md` — LGPD-oriented data flows & retention policies
- `docs/ai.md` — engines, guardrails, model licensing rules
- `docs/deployment.md` — containers, backups, restore, TLS
- `CONTRIBUTING.md`, `CHANGELOG.md`, `NOTICE.md`, `THIRD_PARTY_LICENSES.md`

## Limitations (current release)

- CRUD endpoints are implemented in routes.rs but require live PostgreSQL for full integration testing.
  Services layer is complete and tested.
- Transcription/AI real-model execution requires an operator-provided local
  provider (e.g., Ollama); none is bundled. No weights ship with the repo.
- Accessibility (WCAG 2.2 AA) work begins with the web UI.
- Not production-hardened: no pen-test or regulatory certification performed.

## License

Copyright (C) 2026 MedAssist OSS contributors.

This program is free software licensed under the **GNU General Public License
v3.0 or later** (full text in `LICENSE`). You may redistribute and/or modify it
under those terms. Third-party component licenses are inventoried individually
in `THIRD_PARTY_LICENSES.md` — the application's GPL does not automatically
cover dependencies, model weights or terminology sources. When distributing
modified versions, keep `LICENSE`, `NOTICE.md` and
`THIRD_PARTY_LICENSES.md` intact and provide source availability as required
by GPL section 6.
