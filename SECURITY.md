# Security Policy — MedAssist OSS

## Reporting a vulnerability

MedAssist OSS handles sensitive clinical data. If you believe you found a
security issue, please report it responsibly.

**Current limitation (honest disclosure):** this project does not yet operate a
private issue tracker or a verified security contact address. Until one is
established in the public repository, report vulnerabilities through the
repository's private security advisory mechanism (GitHub: *Security → Report a
vulnerability*) if available, or by opening an issue **without** disclosing
exploitable details publicly until a fix is released.

Do **not** post full exploit code, real patient data, or reproduction steps
containing protected health information in public channels.

## Scope and design commitments

- Clinical information is treated as sensitive personal data (LGPD art. 11).
- Passwords are hashed with Argon2id; plaintext passwords are never stored.
- Session tokens are opaque, expiring, and revocable; cookies use
  `HttpOnly`, `Secure`, `SameSite=Strict` in production.
- All SQL goes through parameterized SQLx queries; no string interpolation.
- Multi-tenant isolation is enforced server-side via `org_id`; a client-supplied
  identifier is never treated as proof of authorization.
- Audit events record actor, action, resource, timestamp and outcome — but not
  full clinical content by default.
- AI output is always a *draft* requiring human review before finalization;
  prompts, transcriptions and model responses are treated as untrusted input
  (prompt-injection defense).
- No clinical data is sent to external providers unless explicitly configured
  and authorized by the operator.
- Logs exclude credentials, tokens and full transcriptions by default.

## What is NOT yet implemented (as of this writing)

- TLS termination configuration examples are documented but not enforced by the
  application itself (use a reverse proxy or enable Postgres/TLS at deploy time).
- Automated penetration testing and third-party security audit have not been
  performed. Do not assume the software is certified secure for production use.

## Data protection notes

- Retention policies for audio, transcripts, documents and backups must be
  configured per organization; see `docs/privacy.md`.
- Never commit real `.env` files; `.env.example` ships placeholders only.
