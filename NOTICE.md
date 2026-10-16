# Notice — MedAssist OSS

Attributions and notices required by third-party components or applicable
licenses:

1. **GNU GPL v3 text** in `LICENSE` is reproduced verbatim from the system
   canonical copy (`/usr/share/common-licenses/GPL-3`, Debian's
   `common-licenses` package, FSF-authoritative text). Copyright © 2007 Free
   Software Foundation, Inc. The license text itself is not subject to GPL but
   must be distributed unmodified.

2. **Argon2** reference implementation by the Password Hashing Competition
   winners (A. Biryukov, D. Khovratovich, F. P. S. de Sousa); Rust wrapper
   `argon2` crate (RustCrypto), MIT/Apache-2.0. We use Argon2id parameters as
   recommended by OWASP ASVS.

3. **PostgreSQL** — this software interoperates with PostgreSQL; "PostgreSQL"
   is a registered trademark of the PostgreSQL Global Development Group.

4. **HL7®, FHIR®** — FHIR® is the registered trademark of HL7 and is used for
   interoperability documentation only. No claim of HL7 conformance is made.

5. **WHO ICD-10** — when operators import ICD-10 tables, the WHO terms of use
   and attribution apply to that data, not to this codebase. This repository
   does not embed ICD-10 content.

6. **Doctor Assistant (doctorassistant.ai)** was consulted only as a public
   functional reference at the product-category level. No code, branding,
   visual identity or proprietary content from that service is present in this
   project. All implementations are original work under GPL-3.0-or-later.

7. Individual model weights (when configured by an operator, e.g., Whisper or
   Mistral models) carry their own licenses and notices which must accompany
   any redistribution of derived artifacts. See `THIRD_PARTY_LICENSES.md`.
