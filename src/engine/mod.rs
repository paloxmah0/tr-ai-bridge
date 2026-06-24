pub mod rules;

use crate::domain::strategy::Rule;
use crate::domain::signal::Signal;
use crate::domain::{Side, TradingMode};
use crate::error::AppResult;
use crate::market::MarketProvider;
use chrono::Utc;
use rust_decimal::Decimal;
use rules::{evaluate, Indicators};
use uuid::Uuid;

use crate::db::Db;
use crate::domain::strategy::Strategy;

/// Result of evaluating rules over a single candle window. Pure (no I/O) so
/// the backtester can reuse it without a DB or market provider.
pub struct WindowSignal {
    pub side: Side,
    pub price: Decimal,
    pub strength: Decimal,
    pub rationale: String,
}

/// Evaluate rules against precomputed indicators for one bar. Returns a signal
/// when at least one rule fires.
pub fn evaluate_at(rules: &[Rule], ind: &Indicators) -> AppResult<Option<WindowSignal>> {
    let enabled: Vec<&Rule> = rules.iter().filter(|r| r.enabled).collect();
    if enabled.is_empty() {
        return Ok(None);
    }
    let mut fired_weight = Decimal::ZERO;
    let mut total_weight = Decimal::ZERO;
    let mut fired_names: Vec<String> = Vec::new();
    for r in &enabled {
        total_weight += r.weight;
        match evaluate(&r.expr, ind) {
            Ok(true) => {
                fired_weight += r.weight;
                fired_names.push(r.name.clone());
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(rule = %r.name, error = %e, "rule eval failed");
            }
        }
    }
    if total_weight == Decimal::ZERO || fired_weight == Decimal::ZERO {
        return Ok(None);
    }
    let strength = (fired_weight / total_weight).round_dp(6);
    let side = if ind.pct_change >= Decimal::ZERO { Side::Buy } else { Side::Sell };
    let rationale = format!(
        "Fired rules: {} (strength {:.2}%)",
        fired_names.join(", "),
        strength * Decimal::from(100)
    );
    Ok(Some(WindowSignal { side, price: ind.price, strength, rationale }))
}

/// Evaluate a strategy's rules against the latest market data. Returns a Signal
/// only when at least one rule fires; `strength` is the weighted fraction of firing rules.
pub async fn evaluate_strategy(
    db: &Db,
    market: &dyn MarketProvider,
    strategy: &Strategy,
    account_mode: TradingMode,
) -> AppResult<Option<Signal>> {
    let rules = db.list_rules(strategy.id).await?;

    let symbol = strategy.symbols.first().cloned().unwrap_or_default();
    if symbol.is_empty() {
        return Ok(None);
    }

    let candles = market.candles(&symbol, 250).await?;
    if candles.is_empty() {
        return Ok(None);
    }
    let ind = Indicators::compute(&candles)?;
    let window = evaluate_at(&rules, &ind)?;

    let Some(w) = window else { return Ok(None) };

    Ok(Some(Signal {
        id: Uuid::new_v4(),
        strategy_id: strategy.id,
        account_id: strategy.account_id,
        symbol,
        side: w.side,
        price: w.price,
        strength: w.strength,
        rationale: w.rationale,
        mode: account_mode,
        created_at: Utc::now(),
    }))
}
