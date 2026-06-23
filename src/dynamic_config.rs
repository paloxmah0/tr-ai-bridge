use crate::db::Db;
use crate::error::AppResult;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A shared, runtime-mutable key-value config. Initialized from env defaults,
/// overlaid with DB-persisted values. Updated via the settings API.
/// Clients (LLM, brokers) read from this on each call so token changes take
/// effect immediately without restart.
pub type SharedConfig = Arc<RwLock<HashMap<String, String>>>;

pub struct DynamicConfig {
    values: SharedConfig,
    db: Db,
}

impl DynamicConfig {
    pub async fn new(db: Db, env_defaults: HashMap<String, String>) -> Self {
        let mut values = env_defaults;
        if let Ok(db_vals) = db.load_settings().await {
            for (k, v) in db_vals {
                values.insert(k, v);
            }
        }
        Self { values: Arc::new(RwLock::new(values)), db }
    }

    /// Create from env defaults only — instant, no DB hit. The `db` field
    /// should be set via `with_db` before calling `overlay_db`.
    pub fn from_defaults(env_defaults: HashMap<String, String>) -> Self {
        Self {
            values: Arc::new(RwLock::new(env_defaults)),
            db: Db::new(
                sqlx::postgres::PgPoolOptions::new()
                    .connect_lazy("postgres://localhost")
                    .unwrap_or_else(|_| panic!("failed to create dummy pool")),
            ),
        }
    }

    /// Attach a real DB pool after construction (used with `from_defaults`).
    pub fn with_db(&mut self, db: Db) {
        self.db = db;
    }

    /// Try to load DB-persisted settings and overlay them on top of env
    /// defaults. Non-blocking if the DB is unreachable (just logs a warning).
    pub async fn overlay_db(&self) {
        if let Ok(db_vals) = self.db.load_settings().await {
            let mut guard = self.values.write().await;
            for (k, v) in db_vals {
                guard.insert(k, v);
            }
            tracing::info!("loaded persisted settings from DB");
        } else {
            tracing::warn!("could not load settings from DB (using env defaults)");
        }
    }

    pub fn shared(&self) -> SharedConfig {
        self.values.clone()
    }

    pub async fn get(&self, key: &str) -> String {
        self.values.read().await.get(key).cloned().unwrap_or_default()
    }

    pub async fn set(&self, key: &str, value: &str) -> AppResult<()> {
        self.values.write().await.insert(key.to_string(), value.to_string());
        self.db.save_setting(key, value).await
    }

    pub async fn get_all(&self) -> HashMap<String, String> {
        self.values.read().await.clone()
    }
}

/// Well-known setting keys.
pub mod keys {
    pub const LLM_BASE_URL: &str = "llm_base_url";
    pub const LLM_API_KEY: &str = "llm_api_key";
    pub const LLM_MODEL: &str = "llm_model";

    pub const DERIV_APP_ID: &str = "deriv_app_id";
    pub const DERIV_API_TOKEN: &str = "deriv_api_token";
    pub const DERIV_ACCOUNT_ID: &str = "deriv_account_id";

    pub const OANDA_BASE_URL: &str = "oanda_base_url";
    pub const OANDA_API_TOKEN: &str = "oanda_api_token";
    pub const OANDA_ACCOUNT_ID: &str = "oanda_account_id";
}
