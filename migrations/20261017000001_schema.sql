-- MedAssist OSS initial schema (GPL-3.0-or-later).
-- Multi-tenant: every clinical row carries org_id; repositories filter by it.

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE organisations (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (name <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    email text NOT NULL,
    password_hash text NOT NULL,
    roles text[] NOT NULL DEFAULT '{}',
    failed_logins int NOT NULL DEFAULT 0,
    locked_until timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_lower ON users (lower(email));

CREATE TABLE sessions (
    token_hash text PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    org_id uuid NOT NULL,
    roles text[] NOT NULL DEFAULT '{}',
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_idx ON sessions(user_id);

CREATE TABLE patients (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    family_name text NOT NULL,
    given_name text NOT NULL,
    birth_date date,
    sex text CHECK (sex IN ('female','male','intersex','unknown','undisclosed')),
    contact text,
    external_ids jsonb NOT NULL DEFAULT '[]',
    notes text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz
);
CREATE INDEX patients_org_idx ON patients(org_id) WHERE deleted_at IS NULL;
CREATE INDEX patients_name_trgm_idx ON patients USING gin ((lower(family_name) || ' ' || lower(given_name)) gin_trgm_ops);

CREATE TABLE encounters (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    patient_id uuid NOT NULL REFERENCES patients(id),
    practitioner_id uuid NOT NULL,
    status text NOT NULL CHECK (status IN ('open','in_progress','closed','finalised','cancelled')),
    chief_complaint text,
    started_at timestamptz,
    ended_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX encounters_patient_idx ON encounters(patient_id, created_at DESC);
CREATE INDEX encounters_org_idx ON encounters(org_id);

CREATE TABLE notes (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    encounter_id uuid NOT NULL REFERENCES encounters(id),
    patient_id uuid NOT NULL REFERENCES patients(id),
    author_id uuid NOT NULL,
    template_id uuid,
    status text NOT NULL CHECK (status IN ('draft','in_review','finalised','amended')),
    version int NOT NULL DEFAULT 1,
    body jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    finalised_at timestamptz,
    finalised_by uuid,
    amends_note_id uuid REFERENCES notes(id)
);
CREATE INDEX notes_encounter_idx ON notes(encounter_id, created_at DESC);
CREATE INDEX notes_org_idx ON notes(org_id);

CREATE TABLE note_versions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    note_id uuid NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    version int NOT NULL,
    body jsonb NOT NULL,
    edited_by uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (note_id, version)
);

CREATE TABLE documents (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    patient_id uuid NOT NULL REFERENCES patients(id),
    encounter_id uuid REFERENCES encounters(id),
    kind text NOT NULL CHECK (kind IN ('pdf','plain_text','csv','image','other')),
    filename text NOT NULL,
    content_type text NOT NULL,
    size_bytes bigint NOT NULL,
    sha256 text NOT NULL,
    blob_key text NOT NULL,
    received_at timestamptz,
    state text NOT NULL CHECK (state IN ('pending','processing','extracted','failed','reviewed')),
    error text,
    uploaded_by uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX documents_patient_idx ON documents(patient_id, created_at DESC);
CREATE INDEX documents_org_idx ON documents(org_id);

CREATE TABLE consents (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    patient_id uuid NOT NULL REFERENCES patients(id),
    granted_by text NOT NULL,
    scope text NOT NULL,
    retention_policy text NOT NULL,
    granted_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);
CREATE INDEX consents_patient_idx ON consents(patient_id);

CREATE TABLE transcriptions (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    patient_id uuid NOT NULL REFERENCES patients(id),
    encounter_id uuid REFERENCES encounters(id),
    state text NOT NULL CHECK (state IN ('pending','processing','completed','failed','cancelled')),
    engine text NOT NULL,
    engine_version text,
    language text,
    segments jsonb NOT NULL DEFAULT '[]',
    reviewed boolean NOT NULL DEFAULT false,
    consent_id uuid NOT NULL REFERENCES consents(id),
    audio_retention_policy text NOT NULL,
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX transcriptions_org_idx ON transcriptions(org_id);

CREATE TABLE jobs (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organisations(id),
    kind text NOT NULL CHECK (kind IN ('transcription','document_extraction','ai_documentation_draft')),
    state text NOT NULL CHECK (state IN ('pending','running','completed','failed','cancelled')),
    attempts int NOT NULL DEFAULT 0,
    max_attempts int NOT NULL DEFAULT 3,
    error text,
    result_ref text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX jobs_claim_idx ON jobs(kind, state, created_at);

CREATE TABLE templates (
    id uuid PRIMARY KEY,
    org_id uuid REFERENCES organisations(id),
    name text NOT NULL,
    version int NOT NULL DEFAULT 1,
    sections text[] NOT NULL,
    required_sections text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX templates_scope_name_ver ON templates (COALESCE(org_id, '00000000-0000-0000-0000-000000000000'::uuid), name, version);

CREATE TABLE audit_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    time timestamptz NOT NULL,
    actor_user_id uuid,
    org_id uuid,
    action text NOT NULL,
    resource_type text NOT NULL,
    resource_id uuid,
    outcome text NOT NULL CHECK (outcome IN ('success','failure','denied')),
    metadata jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_time_idx ON audit_events(time DESC);
CREATE INDEX audit_org_idx ON audit_events(org_id, time DESC);
CREATE INDEX audit_resource_idx ON audit_events(resource_type, resource_id);

CREATE TABLE codes (
    system text NOT NULL,
    version text NOT NULL,
    code text NOT NULL,
    display text NOT NULL,
    PRIMARY KEY (system, version, code)
);
CREATE INDEX codes_display_trgm ON codes USING gin (display gin_trgm_ops);
