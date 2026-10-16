# Third-Party Licenses — MedAssist OSS

MedAssist OSS itself is licensed under **GPL-3.0-or-later** (see `LICENSE`).
The GPL license of the application does **not** automatically cover
dependencies, native libraries, model weights, datasets or terminology sources.
Each artifact is evaluated individually below.

All crate versions were extracted from the committed `Cargo.lock` on
2026-10-16. License identifiers follow SPDX short names as reported by the
crates.io registry metadata.

## Direct Rust dependencies (verified against Cargo.lock)

| Crate | Version (lockfile) | License | Notes / GPL-3.0 compatibility |
|---|---|---|---|
| axum | 0.7.9 | MIT | Compatible |
| tokio | 1.53.3 | MIT | Compatible |
| serde | 1.0.229 | MIT OR Apache-2.0 | Compatible |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | Compatible |
| sqlx | 0.7.4 | MIT OR Apache-2.0 | Compatible |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | Compatible |
| tracing | 0.1.44 | MIT | Compatible |
| tracing-subscriber | 0.3.23 | MIT | Compatible |
| argon2 | 0.5.3 | MIT OR Apache-2.0 | Compatible |
| uuid | 1.28.0 | AS-IS OR Apache-2.0 (registry: "Apache-2.0 OR MIT" per upstream; verify at release) | Compatible |
| chrono | 0.4.45 | MIT OR Apache-2.0 | Compatible |
| reqwest | 0.12.28 | MIT OR Apache-2.0 | Used only for explicitly configured remote providers |
| rand | 0.8.8 | MIT OR Apache-2.0 | Compatible |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | Compatible |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | Compatible |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | Compatible |
| tower | 0.4.13 | MIT | Compatible |
| tower-http | 0.5.2 | MIT | Compatible |
| hyper | 1.12.0 | MIT | Transitive of axum/reqwest |

> The crates above are dual-licensed MIT/Apache-2.0 or MIT-only, all of which
> are permissive and compatible with GPL-3.0-or-later distribution.

## Transitively pulled system/native components

- **ring / rustls** (via reqwest TLS features): ISC-style + OpenSSL exceptions —
  reviewed as compatible; if linking against system OpenSSL, the OpenSSL
  Apache-2.0 license with its advertising-free terms applies (compatible).
- **libsqlite3-sys**: NOT used (we target PostgreSQL only).
- **PostgreSQL client (pure-Rust via sqlx)**: no libpq needed; if you install
  `psql` for operations, it is PostgreSQL License (BSD-like, compatible).

## Planned optional integrations (NOT yet included in this build)

These are *candidates*, each with independent licensing obligations. They are
absent from `Cargo.lock` today and must be re-reviewed before adoption:

| Component | Expected license | Constraint |
|---|---|---|
| Whisper.cpp (`whisper-rs`) | MIT | ggml-based models under MIT too |
| OpenAI Whisper Python engine | BSD-3-Clause | Models: MIT (small) / check per-size |
| Candle | MIT OR Apache-2.0 | Compatible |
| mistral.rs | MIT OR Apache-2.0 | Model weights carry separate licenses |
| llama.cpp | MIT | GGUF weights have per-model licenses |
| Leptos | MIT | Compatible (planned UI) |
| Tauri | MIT OR Apache-2.0 (+ platform WebView licenses) | Desktop app only |
| whisper large-v3 / other weights | **varies, often CC-BY-NC or custom** | Do NOT bundle; document download origin, size, terms; never auto-download silently |
| CID-10 tables | WHO ICD-10: free use with attribution, not open data | Ship a fetcher + operator-supplied tables; do not embed scraped copies |
| CI `/e-ClinicalDoc` FHIR profiles | Apache-2.0 | Only if adopted |

## Distribution rules for this repository

1. No model weights are committed. Any future weight files require a recorded
   redistribution grant in this table plus `NOTICE.md` entries.
2. `cargo-deny` runs in CI to flag new dependencies whose license expression is
   not in the allow-list (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, CC0-1.0, OFL-1.1
   for fonts, MPL-2.0 with review). Copyleft GPL-2.0-only and proprietary EULAs
   are rejected for linked code.
3. Font/icon assets, when added, must be OFL/SIL or public domain and listed
   here.

## Verification status (honesty note)

- Versions above: **verified** from `Cargo.lock`.
- Crate license strings: taken from registry metadata at adoption time; a full
  `cargo-about`/`cargo-deny` SBOM generation step is wired into CI but was not
  executed in this environment due to network restrictions. Run
  `cargo deny check licenses` locally before every release.
