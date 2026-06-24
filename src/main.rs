mod analytics;
mod api;
mod backtest;
mod config;
mod db;
mod domain;
mod dynamic_config;
mod engine;
mod engine_loop;
mod error;
mod execution;
mod ingest;
mod insights;
mod llm;
mod market;
mod state;

use std::collections::HashMap;
use std::sync::Arc;

use config::Settings;
use dynamic_config::{DynamicConfig, keys};
use llm::LlmClient;
use market::{Broker, DerivClient, MarketProvider, MarketRegistry, OandaClient};
use sqlx::sqlite::SqlitePoolOptions;
use state::AppState;

use crate::db::Db;
use crate::ingest::Ingestor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn,hyper=warn".into()),
        )
        .init();

    let settings = Arc::new(Settings::load()?);

    // Lazy pool — no connection attempt, returns instantly.
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_lazy(&settings.database.url)?;

    let db = Db::new(pool);

    // Build initial config from env defaults only — instant, no DB hit.
    let mut env_defaults = HashMap::new();
    env_defaults.insert(keys::LLM_BASE_URL.to_string(), settings.llm.base_url.clone());
    env_defaults.insert(keys::LLM_API_KEY.to_string(), settings.llm.api_key.clone());
    env_defaults.insert(keys::LLM_MODEL.to_string(), settings.llm.model.clone());
    env_defaults.insert(keys::DERIV_APP_ID.to_string(), settings.deriv.app_id.clone());
    env_defaults.insert(keys::DERIV_API_TOKEN.to_string(), settings.deriv.api_key.clone());
    env_defaults.insert(keys::DERIV_ACCOUNT_ID.to_string(), settings.deriv.account_id.clone());
    env_defaults.insert(keys::OANDA_BASE_URL.to_string(), settings.oanda.base_url.clone());
    env_defaults.insert(keys::OANDA_API_TOKEN.to_string(), settings.oanda.api_key.clone());
    env_defaults.insert(keys::OANDA_ACCOUNT_ID.to_string(), settings.oanda.account_id.clone());

    // Start with env-only config; overlay DB values in the background once
    // the database is reachable. This keeps startup instant.
    let mut config = DynamicConfig::from_defaults(env_defaults);
    config.with_db(db.clone());
    let config = Arc::new(config);

    // Clients read credentials from the shared config, so tokens set via the
    // Settings page take effect immediately.
    let llm = Arc::new(LlmClient::new(config.shared(), settings.llm.timeout_secs));
    let ingest = Arc::new(Ingestor::new(db.clone(), llm.clone()));

    let deriv_client = Arc::new(DerivClient::new(config.shared(), settings.deriv_granularity_secs));
    let oanda_client = OandaClient::new(config.shared(), settings.deriv_granularity_secs);

    // Use Deriv for ALL market data when OANDA token isn't set — Deriv provides
    // both synthetic indices AND forex pairs (frxEURUSD etc.) without a token.
    let oanda_token = config.get(crate::dynamic_config::keys::OANDA_API_TOKEN).await;
    let (forex_provider, forex_broker): (Arc<dyn MarketProvider>, Option<Arc<dyn Broker>>) =
        if oanda_token.is_empty() {
            tracing::info!("no OANDA token — using Deriv for forex data too");
            (deriv_client.clone() as Arc<dyn MarketProvider>, None)
        } else {
            let ob: Arc<dyn Broker> = oanda_client.clone();
            (oanda_client.clone() as Arc<dyn MarketProvider>, Some(ob))
        };

    let deriv: Arc<dyn MarketProvider> = deriv_client.clone();
    let deriv_broker: Arc<dyn Broker> = deriv_client.clone();

    let markets = Arc::new(MarketRegistry::new(forex_provider, deriv, forex_broker, Some(deriv_broker)));

    let state = AppState {
        settings: settings.clone(),
        db: db.clone(),
        config: config.clone(),
        llm: llm.clone(),
        ingest: ingest.clone(),
        markets: markets.clone(),
    };

    // Background: run DB migrations + overlay DB-persisted settings.
    // Non-blocking — the server starts immediately and these complete in
    // the background once PostgreSQL is reachable.
    {
        let db = db.clone();
        let config = config.clone();
        tokio::spawn(async move {
            match db.run_migrations().await {
                Ok(()) => tracing::info!("database migrations applied"),
                Err(e) => tracing::warn!(error = %e, "database unavailable — run in degraded mode until DB is up"),
            }
            config.overlay_db().await;
        });
    }

    // Background engine loop.
    {
        let db = db.clone();
        let markets = markets.clone();
        let tick = settings.engine_tick_secs;
        tokio::spawn(async move { engine_loop::run(db, markets, tick).await });
    }

    let app = api::router(state);
    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
