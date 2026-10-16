//! Axum HTTP API for MedAssist OSS. Transport only: request parsing, auth
//! extraction, error mapping. All behaviour lives in the application layer.
//!
//! License: GPL-3.0-or-later.

pub mod error;
pub mod routes;
pub mod state;

use axum::routing::get;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use state::AppState;

pub const MAX_BODY_BYTES: usize = 26 * 1024 * 1024; // 25 MiB docs + headroom

/// Build the router. `ui` optionally serves the static web app at `/`.
pub fn build_router(state: AppState, ui: Option<axum::Router>) -> axum::Router {
    let api = axum::Router::new()
        .route("/healthz", get(routes::health_live))
        .route("/readyz", get(routes::health_ready))
        .nest("/api/v1", routes::api_v1(state.clone()))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .with_state(state);

    let mut app = api;
    if let Some(ui) = ui {
        app = app.merge(ui);
    }
    app.layer(TraceLayer::new_for_http())
}
