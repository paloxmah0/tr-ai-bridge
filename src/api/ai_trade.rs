use crate::ai_engine::{AnalyzeRequest, Prediction, TradeRequest};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use rust_decimal::Decimal;

/// POST /api/analyze — AI analyzes the market and returns a BUY/SELL prediction.
/// User provides: symbol (market) + timeframe. The app does the rest.
pub async fn analyze(
    State(state): State<AppState>,
    Json(req): Json<AnalyzeRequest>,
) -> AppResult<Json<Prediction>> {
    // Determine asset class from symbol prefix.
    let class = req.asset_class.unwrap_or_else(|| {
        if req.symbol.starts_with("frx") {
            crate::domain::AssetClass::Forex
        } else {
            crate::domain::AssetClass::DerivIndex
        }
    });
    let provider = state.markets.select(class).clone();

    let pred = crate::ai_engine::analyze(
        &state.db,
        provider.as_ref(),
        &state.llm,
        &req,
    )
    .await?;
    Ok(Json(pred))
}

/// POST /api/trade — User confirms direction and places a trade.
/// The app handles everything else (entry, SL, TP, execution).
pub async fn place_trade(
    State(state): State<AppState>,
    Json(req): Json<TradeRequest>,
) -> AppResult<Json<serde_json::Value>> {
    use crate::domain::{OrderType, Side, TradingMode};
    use crate::domain::trade::TradeStatus;
    use chrono::Utc;
    use uuid::Uuid;

    let class = req.asset_class.unwrap_or_else(|| {
        if req.symbol.starts_with("frx") {
            crate::domain::AssetClass::Forex
        } else {
            crate::domain::AssetClass::DerivIndex
        }
    });
    let provider = state.markets.select(class).clone();

    // Get current price.
    let quote = provider.quote(&req.symbol).await?;
    let side = match req.direction.as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => return Err(AppError::BadRequest("direction must be 'buy' or 'sell'".into())),
    };
    let entry = match side {
        Side::Buy => quote.ask,
        Side::Sell => quote.bid,
    };

    // Get the first account (demo account).
    let accounts = state.db.list_accounts().await?;
    let account = accounts.first().ok_or_else(|| AppError::NotFound("no account".into()))?;

    // Compute SL/TP using ATR.
    let candles = provider.candles(&req.symbol, 100).await?;
    let ind = crate::engine::rules::Indicators::compute(&candles)?;
    let atr = ind.atr.get(&14).copied().unwrap_or(entry * Decimal::new(5, 1000));
    let pip = if req.symbol.starts_with("frx") { Decimal::new(1, 4) } else { Decimal::ONE };
    let sl_dist = atr.max(pip * Decimal::from(20));
    let tp_dist = sl_dist * Decimal::from(2);
    let (stop, tp) = match side {
        Side::Buy => (entry - sl_dist, entry + tp_dist),
        Side::Sell => (entry + sl_dist, entry - tp_dist),
    };

    let stake = req.stake.unwrap_or(account.balance * Decimal::new(1, 2)); // 10% default

    // Create the trade.
    let trade = crate::domain::trade::Trade {
        id: Uuid::new_v4(),
        account_id: account.id,
        strategy_id: Uuid::nil(), // AI-driven, no specific strategy
        signal_id: None,
        symbol: req.symbol.clone(),
        side,
        order_type: OrderType::Market,
        mode: account.mode,
        size: stake,
        entry_price: entry,
        exit_price: None,
        stop_loss: Some(stop),
        take_profit: Some(tp),
        pnl: None,
        status: TradeStatus::Open,
        opened_at: Utc::now(),
        closed_at: None,
    };
    state.db.insert_trade(&trade).await?;

    Ok(Json(serde_json::json!({
        "trade_id": trade.id,
        "direction": req.direction,
        "symbol": req.symbol,
        "entry_price": entry,
        "stop_loss": stop,
        "take_profit": tp,
        "stake": stake,
        "mode": format!("{:?}", account.mode).to_lowercase(),
        "expiry_minutes": req.timeframe_minutes,
        "message": format!("Trade placed: {} {} at {}", req.direction, req.symbol, entry)
    })))
}
