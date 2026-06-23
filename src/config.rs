use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub server: ServerSettings,
    pub database: DatabaseSettings,
    pub llm: LlmSettings,
    pub forex: ProviderSettings,
    pub deriv: ProviderSettings,
    pub oanda: ProviderSettings,
    #[serde(default)]
    pub default_trading_mode: String,
    #[serde(default = "default_tick")]
    pub engine_tick_secs: u64,
    #[serde(default = "default_upload_bytes")]
    pub max_note_upload_bytes: usize,
    #[serde(default = "default_granularity")]
    pub deriv_granularity_secs: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSettings {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
}

impl LlmSettings {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSettings {
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub account_id: String,
    /// Deriv application id, used to build the WebSocket URL.
    #[serde(default)]
    pub app_id: String,
}

fn default_tick() -> u64 { 10 }
fn default_upload_bytes() -> usize { 5 * 1024 * 1024 }
fn default_granularity() -> u32 { 60 }

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();

        // Auto-create .env from .env.example if missing.
        if !std::path::Path::new(".env").exists() && std::path::Path::new(".env.example").exists() {
            let _ = std::fs::copy(".env.example", ".env");
            let _ = dotenvy::dotenv();
        }

        let table = serde_json::json!({
            "server": {
                "host": env("SERVER_HOST").parse().unwrap_or_else(|_| "0.0.0.0".to_string()),
                "port": env("SERVER_PORT").parse::<u16>().unwrap_or(8080),
            },
            "database": {
                "url": std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "sqlite://trading.db?mode=rwc".into()),
            },
            "llm": {
                "base_url": std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into()),
                "api_key": std::env::var("LLM_API_KEY").unwrap_or_default(),
                "model": std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
                "timeout_secs": env("LLM_TIMEOUT_SECS").parse::<u64>().unwrap_or(60),
            },
            "forex": {
                "base_url": env("FOREX_PROVIDER_BASE_URL"),
                "api_key": env("FOREX_PROVIDER_API_KEY"),
                "account_id": env("FOREX_PROVIDER_ACCOUNT_ID"),
            },
            "deriv": {
                "base_url": env("DERIV_PROVIDER_BASE_URL"),
                "api_key": env("DERIV_PROVIDER_API_TOKEN"),
                "account_id": env("DERIV_PROVIDER_ACCOUNT_ID"),
                "app_id": std::env::var("DERIV_APP_ID").unwrap_or_else(|_| "1089".into()),
            },
            "oanda": {
                "base_url": std::env::var("OANDA_PROVIDER_BASE_URL").unwrap_or_else(|_| "https://api-fxpractice.oanda.com".into()),
                "api_key": env("OANDA_PROVIDER_API_TOKEN"),
                "account_id": env("OANDA_PROVIDER_ACCOUNT_ID"),
            },
            "default_trading_mode": std::env::var("DEFAULT_TRADING_MODE").unwrap_or_else(|_| "paper".into()),
            "engine_tick_secs": env("ENGINE_TICK_SECS").parse::<u64>().unwrap_or(default_tick()),
            "max_note_upload_bytes": env("MAX_NOTE_UPLOAD_BYTES").parse::<usize>().unwrap_or(default_upload_bytes()),
            "deriv_granularity_secs": env("DERIV_GRANULARITY_SECS").parse::<u32>().unwrap_or(default_granularity()),
        });

        let settings: Settings = serde_json::from_value(table)?;
        Ok(settings)
    }
}
