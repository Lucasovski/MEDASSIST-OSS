//! MedAssist OSS — domain core.
//!
//! This crate is intentionally free of framework dependencies (no Axum, no
//! SQLx, no HTTP, no AI provider). It holds the clinical domain model and the
//! invariants that must hold regardless of persistence or transport.
//!
//! License: GPL-3.0-or-later.

pub mod allergy;
pub mod consent;
pub mod document;
pub mod encounter;
pub mod error;
pub mod identifier;
pub mod job;
pub mod medication;
pub mod note;
pub mod patient;
pub mod prov;
pub mod transcription;
pub mod vital_sign;

pub use error::DomainError;

/// Small helper trait for entities that carry a stable internal id.
pub trait Entity {
    fn id(&self) -> uuid::Uuid;
}
