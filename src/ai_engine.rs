//! AI Prediction Engine.
//!
//! Combines ALL accumulated knowledge — extracted notes, technical indicators,
//! and candlestick patterns — into a single BUY/SELL prediction for a given
//! market and timeframe. The user doesn't pick strategies or rules; the app
//! uses everything it knows to analyze the market and suggest a direction.

use crate::db::Db;
use crate::domain::strategy::Rule;
use crate::domain::{AssetClass, Candle, Side};
use crate::engine::rules::{evaluate, Indicators};
use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::market::MarketProvider;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single AI prediction for a market + timeframe.
#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub direction: String, // "buy" | "sell" | "hold"
    pub confidence: Decimal, // 0.0 - 1.0
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub expiry: chrono::DateTime<chrono::Utc>,
    pub reasoning: String,
    /// Individual signals that contributed to the prediction.
    pub signals: Vec<SignalFactor>,
    /// The timeframe in seconds.
    pub timeframe_secs: u32,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalFactor {
    pub source: String, // "candlestick" | "indicator" | "note" | "momentum"
    pub name: String,
    pub direction: String, // "bullish" | "bearish" | "neutral"
    pub weight: Decimal,
    pub detail: String,
}

/// Request from the user: pick a market + timeframe, get a prediction.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeRequest {
    pub symbol: String,
    /// Timeframe in minutes (e.g. 1, 5, 15, 60).
    #[serde(default = "default_tf")]
    pub timeframe_minutes: u32,
    pub asset_class: Option<AssetClass>,
}

/// Trade request: user confirms the prediction and places a trade.
#[derive(Debug, Clone, Deserialize)]
pub struct TradeRequest {
    pub symbol: String,
    pub direction: String, // "buy" | "sell"
    #[serde(default = "default_tf")]
    pub timeframe_minutes: u32,
    pub stake: Option<Decimal>,
    pub asset_class: Option<AssetClass>,
}

fn default_tf() -> u32 { 5 }

/// Run a full AI analysis on a market. Combines:
/// 1. All candlestick patterns (20+ patterns detected)
/// 2. All technical indicators (RSI, EMA, MACD, ATR, momentum)
/// 3. All extracted note knowledge (rules from LLM-extracted strategies)
/// 4. LLM reasoning over the combined data (if API key configured)
///
/// Returns a single prediction: BUY or SELL with confidence, SL/TP, and reasoning.
pub async fn analyze(
    db: &Db,
    market: &dyn MarketProvider,
    llm: &LlmClient,
    req: &AnalyzeRequest,
) -> AppResult<Prediction> {
    let symbol = &req.symbol;
    let tf_secs = req.timeframe_minutes * 60;

    // 1. Fetch real market data.
    let candles = market.candles(symbol, 300).await?;
    if candles.len() < 50 {
        return Err(AppError::Market("not enough candle data for analysis".into()));
    }
    let ind = Indicators::compute(&candles)?;
    let last = candles.last().unwrap();

    // 2. Gather ALL signal factors.
    let mut factors: Vec<SignalFactor> = Vec::new();
    let mut bull_weight = Decimal::ZERO;
    let mut bear_weight = Decimal::ZERO;

    // --- Candlestick patterns ---
    for (name, val) in &ind.patterns {
        if *val == Decimal::ONE {
            let (dir, w) = pattern_sentiment(name);
            let dir_str = if dir > 0 { "bullish" } else if dir < 0 { "bearish" } else { "neutral" };
            factors.push(SignalFactor {
                source: "candlestick".into(),
                name: name.clone(),
                direction: dir_str.into(),
                weight: w,
                detail: format!("{} pattern detected", name),
            });
            if dir > 0 { bull_weight += w; }
            else if dir < 0 { bear_weight += w; }
        }
    }

    // --- Technical indicators ---
    // RSI
    if let Some(rsi) = ind.rsi.get(&14) {
        let (dir, w, detail) = if *rsi < Decimal::from(30) {
            (1, Decimal::from(2), format!("RSI {} — oversold, bullish reversal likely", rsi))
        } else if *rsi > Decimal::from(70) {
            (-1, Decimal::from(2), format!("RSI {} — overbought, bearish reversal likely", rsi))
        } else if *rsi < Decimal::from(45) {
            (1, Decimal::from(1), format!("RSI {} — approaching oversold", rsi))
        } else if *rsi > Decimal::from(55) {
            (-1, Decimal::from(1), format!("RSI {} — approaching overbought", rsi))
        } else {
            (0, Decimal::ZERO, format!("RSI {} — neutral", rsi))
        };
        let dir_str = if dir > 0 { "bullish" } else if dir < 0 { "bearish" } else { "neutral" };
        factors.push(SignalFactor { source: "indicator".into(), name: "RSI(14)".into(), direction: dir_str.into(), weight: w, detail });
        if dir > 0 { bull_weight += w; } else if dir < 0 { bear_weight += w; }
    }

    // EMA trend
    if let Some(ema50) = ind.ema.get(&50) {
        let (dir, w, detail) = if ind.price > *ema50 {
            (1, Decimal::from(1), format!("Price {} above EMA50 {} — uptrend", ind.price, ema50))
        } else {
            (-1, Decimal::from(1), format!("Price {} below EMA50 {} — downtrend", ind.price, ema50))
        };
        let dir_str = if dir > 0 { "bullish" } else { "bearish" };
        factors.push(SignalFactor { source: "indicator".into(), name: "EMA(50)".into(), direction: dir_str.into(), weight: w, detail });
        if dir > 0 { bull_weight += w; } else if dir < 0 { bear_weight += w; }
    }

    // MACD
    if let Some(macd) = ind.macd {
        let (dir, w, detail) = if macd > Decimal::ZERO {
            (1, Decimal::from(1), format!("MACD {} — bullish momentum", macd))
        } else {
            (-1, Decimal::from(1), format!("MACD {} — bearish momentum", macd))
        };
        let dir_str = if dir > 0 { "bullish" } else { "bearish" };
        factors.push(SignalFactor { source: "indicator".into(), name: "MACD".into(), direction: dir_str.into(), weight: w, detail });
        if dir > 0 { bull_weight += w; } else { bear_weight += w; }
    }

    // Momentum (pct_change)
    {
        let (dir, w, detail) = if ind.pct_change > Decimal::ZERO {
            (1, Decimal::from(1), format!("Recent change +{}% — upward momentum", ind.pct_change))
        } else {
            (-1, Decimal::from(1), format!("Recent change {}% — downward momentum", ind.pct_change))
        };
        let dir_str = if dir > 0 { "bullish" } else { "bearish" };
        factors.push(SignalFactor { source: "momentum".into(), name: "Price Change".into(), direction: dir_str.into(), weight: w, detail });
        if dir > 0 { bull_weight += w; } else { bear_weight += w; }
    }

    // --- Note-derived knowledge: run all extracted rules ---
    let strategies = db.list_enabled_strategies().await.unwrap_or_default();
    let mut note_factors_count = 0;
    for strat in &strategies {
        // Only use strategies whose symbol matches or is generic.
        if !strat.symbols.is_empty() && !strat.symbols.iter().any(|s| s == symbol || symbol.contains(s)) {
            continue;
        }
        let rules = db.list_rules(strat.id).await.unwrap_or_default();
        for rule in &rules {
            if !rule.enabled { continue; }
            match evaluate(&rule.expr, &ind) {
                Ok(true) => {
                    // Determine direction from the rule name/expression.
                    let expr_lower = rule.expr.to_lowercase();
                    let is_bearish = expr_lower.contains("bearish") || expr_lower.contains("short")
                        || expr_lower.contains("overbought") || expr_lower.contains("> 65") || expr_lower.contains("> 70");
                    let dir = if is_bearish { -1 } else { 1 };
                    let w = rule.weight;
                    let dir_str = if dir > 0 { "bullish" } else { "bearish" };
                    factors.push(SignalFactor {
                        source: "note".into(),
                        name: format!("{} ({})", rule.name, strat.name),
                        direction: dir_str.into(),
                        weight: w,
                        detail: format!("Rule from note '{}' fired: {}", strat.name, rule.expr),
                    });
                    if dir > 0 { bull_weight += w; } else { bear_weight += w; }
                    note_factors_count += 1;
                }
                _ => {}
            }
        }
    }

    // 3. Compute final direction + confidence.
    let total = bull_weight + bear_weight;
    let (direction, confidence): (String, Decimal) = if total == Decimal::ZERO {
        ("hold".into(), Decimal::ZERO)
    } else if bull_weight > bear_weight {
        let conf = (bull_weight / total).round_dp(2);
        ("buy".into(), conf)
    } else {
        let conf = (bear_weight / total).round_dp(2);
        ("sell".into(), conf)
    };

    // 4. Compute entry, SL, TP based on ATR.
    let entry = ind.price;
    let atr = ind.atr.get(&14).copied().unwrap_or_else(|| {
        // Fallback: use a fraction of price.
        entry * Decimal::new(5, 1000)
    });
    let pip = if symbol.starts_with("frx") { Decimal::new(1, 4) } else { Decimal::ONE };
    let sl_dist = atr.max(pip * Decimal::from(20));
    let tp_dist = sl_dist * Decimal::from(2);

    let (stop_loss, take_profit) = match direction.as_str() {
        "buy" => (entry - sl_dist, entry + tp_dist),
        "sell" => (entry + sl_dist, entry - tp_dist),
        _ => (entry - sl_dist, entry + tp_dist),
    };

    let expiry = Utc::now() + Duration::seconds(tf_secs as i64);

    // 5. Build reasoning summary.
    let bull_count = factors.iter().filter(|f| f.direction == "bullish").count();
    let bear_count = factors.iter().filter(|f| f.direction == "bearish").count();
    let mut reasoning = format!(
        "AI Analysis: {} bullish signals vs {} bearish signals. ",
        bull_count, bear_count
    );
    if direction == "buy" {
        reasoning.push_str(&format!("Bullish bias with {:.0}% confidence. ", confidence * Decimal::from(100)));
    } else if direction == "sell" {
        reasoning.push_str(&format!("Bearish bias with {:.0}% confidence. ", confidence * Decimal::from(100)));
    } else {
        reasoning.push_str("Market is neutral — no clear direction. ");
    }
    if note_factors_count > 0 {
        reasoning.push_str(&format!("Used {} rules from learned notes. ", note_factors_count));
    }
    reasoning.push_str(&format!("Key factors: "));
    let top_factors: Vec<&SignalFactor> = factors.iter()
        .filter(|f| f.weight > Decimal::ZERO)
        .take(5)
        .collect();
    for f in &top_factors {
        reasoning.push_str(&format!("{} ({}), ", f.name, f.direction));
    }
    if reasoning.ends_with(", ") { reasoning.pop(); reasoning.pop(); }

    // 6. If LLM is configured, enhance reasoning with a natural language summary.
    if let Ok(enhanced) = llm_enhance(llm, symbol, &direction, &confidence, &factors, &ind).await {
        reasoning = format!("{}\n\nAI Insight: {}", reasoning, enhanced);
    }

    Ok(Prediction {
        direction,
        confidence,
        entry_price: entry.round_dp(6),
        stop_loss: stop_loss.round_dp(6),
        take_profit: take_profit.round_dp(6),
        expiry,
        reasoning,
        signals: factors,
        timeframe_secs: tf_secs,
        symbol: symbol.clone(),
    })
}

/// Map candlestick pattern name to sentiment: (direction, weight).
/// +1 = bullish, -1 = bearish, 0 = neutral.
fn pattern_sentiment(name: &str) -> (i32, Decimal) {
    match name {
        // Bullish patterns
        "hammer" => (1, Decimal::from(2)),
        "bullish_engulfing" => (1, Decimal::from(3)),
        "bullish_harami" => (1, Decimal::from(2)),
        "piercing_line" => (1, Decimal::from(2)),
        "morning_star" => (1, Decimal::from(3)),
        "three_white_soldiers" => (1, Decimal::from(3)),
        "dragonfly_doji" => (1, Decimal::from(2)),
        "long_lower_shadow" => (1, Decimal::from(1)),
        "tweezer_bottom" => (1, Decimal::from(1)),
        "inverted_hammer" => (1, Decimal::from(1)),
        // Bearish patterns
        "shooting_star" => (-1, Decimal::from(2)),
        "bearish_engulfing" => (-1, Decimal::from(3)),
        "bearish_harami" => (-1, Decimal::from(2)),
        "dark_cloud_cover" => (-1, Decimal::from(2)),
        "evening_star" => (-1, Decimal::from(3)),
        "three_black_crows" => (-1, Decimal::from(3)),
        "gravestone_doji" => (-1, Decimal::from(2)),
        "long_upper_shadow" => (-1, Decimal::from(1)),
        "tweezer_top" => (-1, Decimal::from(1)),
        "hanging_man" => (-1, Decimal::from(1)),
        // Neutral
        "doji" => (0, Decimal::ZERO),
        "spinning_top" => (0, Decimal::ZERO),
        "marubozu" => (0, Decimal::ZERO),
        "bullish_candle" => (1, Decimal::new(5, 1)), // 0.5
        "bearish_candle" => (-1, Decimal::new(5, 1)),
        _ => (0, Decimal::ZERO),
    }
}

/// Ask the LLM to provide a natural-language insight on the prediction.
async fn llm_enhance(
    llm: &LlmClient,
    symbol: &str,
    direction: &str,
    confidence: &Decimal,
    factors: &[SignalFactor],
    ind: &Indicators,
) -> AppResult<String> {
    let rsi = ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_else(|| "N/A".into());
    let ema50 = ind.ema.get(&50).map(|d| d.to_string()).unwrap_or_else(|| "N/A".into());
    let macd = ind.macd.map(|d| d.to_string()).unwrap_or_else(|| "N/A".into());

    let factor_summary: Vec<String> = factors.iter()
        .filter(|f| f.weight > Decimal::ZERO)
        .map(|f| format!("- {} ({}): {}", f.name, f.direction, f.detail))
        .collect();

    let system = "You are an expert forex and derivatives trader. Analyze the market data and signals provided, and give a concise (2-3 sentence) insight on why this trade direction makes sense. Be direct and specific. Do not give financial advice disclaimers.";
    let user = format!(
        "Market: {}\nDirection: {}\nConfidence: {}%\nRSI(14): {}\nEMA(50): {}\nMACD: {}\nPrice: {}\n\nSignals:\n{}\n\nGive a brief insight on this prediction:",
        symbol, direction, confidence * Decimal::from(100), rsi, ema50, macd, ind.price,
        factor_summary.join("\n")
    );

    let resp = llm.extract_json(system, &user).await;
    match resp {
        Ok(v) => {
            v.get("insight")
                .and_then(|i| i.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| AppError::Llm("no insight in response".into()))
        }
        Err(_) => Err(AppError::Llm("LLM not available".into())),
    }
}
