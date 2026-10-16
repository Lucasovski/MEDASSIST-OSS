//! Infrastructure adapters: PostgreSQL (SQLx) repositories, filesystem blob
//! store and AI provider adapters. All implement the application ports.
//!
//! License: GPL-3.0-or-later.

pub mod audit_pg;
pub mod blob_fs;
pub mod db;
pub mod pg_repos;
pub mod ai_ollama;
pub mod ai_double;

/// Re-export of common types for wiring in `medassist-api`.
pub use db::Db;
