//! Application layer: use-cases orchestrate domain invariants through *ports*
//! (traits) implemented by infrastructure. No Axum/SQLx here.
//!
//! License: GPL-3.0-or-later.

pub mod ai;
pub mod audit;
pub mod auth;
pub mod documents;
pub mod encounters;
pub mod errors;
pub mod notes;
pub mod patients;
pub mod ports;
pub mod templates;
pub mod transcription;

pub use errors::AppError;

/// Pagination defaults shared across use-cases.
pub const DEFAULT_PAGE_SIZE: i64 = 25;
pub const MAX_PAGE_SIZE: i64 = 100;

pub fn clamp_page(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let offset = offset.unwrap_or(0).max(0);
    (limit, offset)
}
