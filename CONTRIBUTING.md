# Contributing to MedAssist OSS

Thank you for your interest in contributing. This project is licensed under
**GNU GPL v3.0 or later**; contributions are accepted under the same terms.

## Licensing conditions (inbound = outbound)

1. Every contribution you submit must be your own original work, or properly
   attributed material that is compatible with GPL-3.0-or-later.
2. By opening a pull request you automatically license your contribution under
   GPL-3.0-or-later. No separate CLA signature step exists yet; if one is added
   later, it will be documented here and PRs will state it explicitly.
3. Do not introduce dependencies whose licenses are incompatible with GPL-3.0
   (e.g., GPL-1.0/2.0-only without "or later", proprietary SDKs). Check
   `THIRD_PARTY_LICENSES.md` and CI (`cargo-deny`) before adding crates.
4. Model weights, datasets and clinical terminology sources have their own
   licenses — never commit large model weights or restricted data without an
   explicit redistribution grant recorded in `THIRD_PARTY_LICENSES.md`.

## Development workflow

```bash
rustup show                      # toolchain pinned in rust-toolchain.toml
cp .env.example .env             # fill local values; never commit .env
sqlx database setup              # applies migrations from ./migrations
cargo run -p medassist-cli       # or cargo run -p medassist-api
```

Before opening a PR, all of these must pass:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Integration tests requiring PostgreSQL are gated behind the `DATABASE_URL`
environment variable and are skipped when it is absent; CI provisions a real
database so they actually run there.

## Code and clinical-safety rules

- Rust is the primary language; keep domain logic free of Axum/SQLx/UI types.
- Never mark an AI-generated note as final without human review paths being
  exercised by tests.
- New endpoints require authorization checks tested against cross-organization
  access attempts.
- Use only synthetic/pseudonymous data in tests, fixtures and docs.
- Log statements must not emit credentials, tokens or full clinical content.

## Reporting issues

- Security vulnerabilities: see `SECURITY.md`, do not open public issues.
- Bugs: include exact commands, Rust version (`rustc --version`), OS, and
  failure output. Redact any real patient data.

## Style

- `cargo fmt` output is authoritative; clippy warnings are errors.
- Public items need doc comments explaining *why*, especially safety-relevant
  invariants (provenance, draft vs. finalized state, org isolation).
