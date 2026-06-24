use axum::{
    routing::{get, post, put},
    Router,
};

use crate::state::AppState;

pub mod accounts;
pub mod ai_trade;
pub mod analytics;
pub mod backtest;
pub mod notes;
pub mod settings;
pub mod signals;
pub mod strategies;
pub mod trades;

/// Build the full application router: API under `/api`, frontend SPA as fallback.
pub fn router(state: AppState) -> Router {
    let api = api_router(state);

    Router::new()
        .nest_service("/api", api)
        .fallback(spa_fallback)
}

/// SPA fallback: try to serve a static file from frontend/dist; if the file
/// doesn't exist (e.g. /settings, /strategies), serve index.html so the React
/// router can handle the path client-side.
async fn spa_fallback(req: axum::extract::Request) -> axum::response::Response {
    let path = req.uri().path().trim_start_matches('/');
    let file_path = if path.is_empty() {
        "frontend/dist/index.html".to_string()
    } else {
        format!("frontend/dist/{path}")
    };

    // Try the exact file first (JS, CSS, images, etc).
    if let Ok(body) = tokio::fs::read(&file_path).await {
        return axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", mime_for(&file_path))
            .body(axum::body::Body::from(body))
            .unwrap();
    }

    // Fallback to index.html for SPA routing.
    match tokio::fs::read("frontend/dist/index.html").await {
        Ok(body) => axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "text/html")
            .body(axum::body::Body::from(body))
            .unwrap(),
        Err(_) => axum::response::Response::builder()
            .status(axum::http::StatusCode::NOT_FOUND)
            .body(axum::body::Body::from(
                "Frontend not built. Run: cd frontend && npm run build",
            ))
            .unwrap(),
    }
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".js") { "application/javascript" }
    else if path.ends_with(".css") { "text/css" }
    else if path.ends_with(".html") { "text/html" }
    else if path.ends_with(".svg") { "image/svg+xml" }
    else if path.ends_with(".png") { "image/png" }
    else if path.ends_with(".ico") { "image/x-icon" }
    else if path.ends_with(".json") { "application/json" }
    else if path.ends_with(".woff2") { "font/woff2" }
    else { "application/octet-stream" }
}

/// All REST API routes, mounted under /api.
fn api_router(state: AppState) -> Router<()> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        // AI trade — user picks market + timeframe, app predicts and trades
        .route("/analyze", post(ai_trade::analyze))
        .route("/trade", post(ai_trade::place_trade))
        // settings
        .route("/settings", get(settings::get).put(settings::update))
        .route("/settings/test", post(settings::test))
        // accounts
        .route("/accounts", post(accounts::create).get(accounts::list))
        .route("/accounts/:id", get(accounts::get))
        .route("/accounts/:id/mode", post(accounts::set_mode))
        // strategies
        .route("/accounts/:id/strategies", post(strategies::create).get(strategies::list))
        .route("/strategies/:id", get(strategies::get).put(strategies::update).delete(strategies::delete))
        .route("/strategies/:id/backtest", post(backtest::run))
        // notes
        .route("/accounts/:id/notes", post(notes::create).get(notes::list))
        .route("/notes/:id", get(notes::get).post(notes::process))
        // signals & trades
        .route("/accounts/:id/signals", get(signals::list))
        .route("/accounts/:id/trades", get(trades::list))
        .route("/trades/:id/close", post(trades::close))
        // analytics
        .route("/accounts/:id/analytics", get(analytics::summary))
        .route("/accounts/:id/insights", get(analytics::insights))
        .with_state(state)
}
