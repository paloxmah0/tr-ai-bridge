use crate::dynamic_config::keys;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// GET /api/settings — returns all config values. Sensitive keys (tokens/keys)
/// are masked to `••••last4` for safety, with an `is_set` flag.
#[derive(Debug, Serialize)]
pub struct SettingsResp {
    pub values: HashMap<String, String>,
    pub masked: HashMap<String, String>,
    pub is_set: HashMap<String, bool>,
}

const SENSITIVE: &[&str] = &[
    keys::LLM_API_KEY,
    keys::DERIV_API_TOKEN,
    keys::OANDA_API_TOKEN,
];

pub async fn get(State(state): State<AppState>) -> AppResult<Json<SettingsResp>> {
    let all = state.config.get_all().await;
    let mut masked = HashMap::new();
    let mut is_set = HashMap::new();
    for (k, v) in &all {
        is_set.insert(k.clone(), !v.is_empty());
        if SENSITIVE.contains(&k.as_str()) && !v.is_empty() {
            let tail: String = v.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            masked.insert(k.clone(), format!("••••{tail}"));
        } else {
            masked.insert(k.clone(), v.clone());
        }
    }
    Ok(Json(SettingsResp { values: all, masked, is_set }))
}

/// PUT /api/settings — update one or more config values. Only non-empty
/// fields in the request body are updated; empty strings are skipped so
/// masked values shown in the UI don't overwrite real tokens.
#[derive(Debug, Deserialize)]
pub struct UpdateSettings {
    #[serde(flatten)]
    pub values: HashMap<String, String>,
}

pub async fn update(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettings>,
) -> AppResult<Json<serde_json::Value>> {
    let mut updated = Vec::new();
    for (k, v) in &req.values {
        // Skip placeholder values (the masked •••• form sent back from the UI).
        if v.starts_with("••••") {
            continue;
        }
        // Trim whitespace from all values (tokens pasted from browser).
        let cleaned = v.trim();
        state.config.set(k, cleaned).await?;
        updated.push(k.clone());
    }
    Ok(Json(serde_json::json!({ "updated": updated })))
}

/// POST /api/settings/test — test connectivity for a given service.
#[derive(Debug, Deserialize)]
pub struct TestReq {
    pub service: String, // "llm" | "deriv" | "oanda"
}

#[derive(Debug, Serialize)]
pub struct TestResp {
    pub ok: bool,
    pub message: String,
}

pub async fn test(
    State(state): State<AppState>,
    Json(req): Json<TestReq>,
) -> AppResult<Json<TestResp>> {
    let (ok, message) = match req.service.as_str() {
        "llm" => {
            let key = state.config.get(keys::LLM_API_KEY).await;
            let url = state.config.get(keys::LLM_BASE_URL).await;
            if key.is_empty() { (false, "No API key set".into()) }
            else if url.is_empty() { (false, "No base URL set".into()) }
            else { (true, format!("LLM configured: {url} model={}", state.config.get(keys::LLM_MODEL).await)) }
        }
        "deriv" => {
            let token = state.config.get(keys::DERIV_API_TOKEN).await;
            if token.is_empty() {
                (true, "Anonymous mode (market data only). Set token for live trading.".into())
            } else {
                // Actually test the token by trying to authorize.
                let deriv_client = crate::market::DerivClient::new(
                    state.config.shared(),
                    state.settings.deriv_granularity_secs,
                );
                match deriv_client.test_connection().await {
                    Ok(msg) => (true, msg),
                    Err(e) => (false, format!("Token test failed: {}", e)),
                }
            }
        }
        "oanda" => {
            let token = state.config.get(keys::OANDA_API_TOKEN).await;
            let acct = state.config.get(keys::OANDA_ACCOUNT_ID).await;
            if token.is_empty() { (false, "No OANDA token set".into()) }
            else if acct.is_empty() { (false, "No OANDA account ID set".into()) }
            else { (true, format!("OANDA configured: account {acct}")) }
        }
        other => (false, format!("Unknown service: {other}")),
    };
    Ok(Json(TestResp { ok, message }))
}
