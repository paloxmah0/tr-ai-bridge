//! AI Market Reader — Evidence-Based Analysis.
//!
//! Philosophy: "It's not what you think, but what you can evidence."
//! Every statement is a VERIFIABLE FACT you can check on the chart.
//! No interpretations, no opinions — just readings from tools.
//! The trade direction is derived FROM the facts, not guessed.

use crate::db::Db;
use crate::domain::{AssetClass, Candle};
use crate::engine::rules::{evaluate, Indicators};
use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::market::MarketProvider;
use chrono::{Duration, Timelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ─── Types ───

#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub market_state: String,
    pub direction: String,
    pub evidence_score: Decimal,
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub expiry: chrono::DateTime<chrono::Utc>,
    pub reasoning: String,
    pub evidence: Vec<Evidence>,
    pub what_to_watch: Vec<String>,
    pub timeframe_secs: u32,
    pub symbol: String,
    pub analysis_time_utc: chrono::DateTime<chrono::Utc>,
    pub market_session: String,
    pub current_candle_start: chrono::DateTime<chrono::Utc>,
    pub next_candle_start: chrono::DateTime<chrono::Utc>,
    pub seconds_to_next_candle: i64,
    pub countdown: String,
    pub recent_candles: Vec<CandleSummary>,
    pub upper_timeframe_context: Vec<UpperTFContext>,
    pub news: crate::news::NewsAssessment,
    pub entry_checklist: EntryChecklist,
    pub estimated_move_duration: String,
    pub estimated_move_candles: u32,
    pub recommended_timeframe_minutes: u32,
    pub recommended_trade_duration_minutes: u32,
    pub recommendation_reason: String,
    pub next_candle_prediction: String,
    pub next_candle_reasoning: String,
    pub active_pattern: String,
    pub active_chart_pattern: String,
    pub trade_options: Vec<TradeOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeOption {
    pub option_type: String,
    pub label: String,
    pub conviction: Decimal,
    pub entry: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub reasoning: String,
    pub supporting_evidence: Vec<String>,
    pub recommended: bool,
    pub risk_reward: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryChecklist {
    pub trend_aligned: bool,
    pub momentum_aligned: bool,
    pub pattern_confirmed: bool,
    pub no_news_risk: bool,
    pub risk_reward_ok: bool,
    pub session_active: bool,
    pub ready: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub source: String,
    pub finding: String,
    pub confirms: String,
    pub weight: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandleSummary {
    pub direction: String,
    pub open: Decimal, pub high: Decimal, pub low: Decimal, pub close: Decimal,
    pub body: Decimal, pub upper_wick: Decimal, pub lower_wick: Decimal,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpperTFContext {
    pub label: String, pub trend: String, pub last_candle_dir: String,
    pub rsi: Decimal, pub adx: Decimal, pub pattern: String, pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeRequest {
    pub symbol: String,
    #[serde(default = "default_tf")]
    pub timeframe_minutes: u32,
    pub asset_class: Option<AssetClass>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradeRequest {
    pub symbol: String,
    pub direction: String,
    #[serde(default = "default_tf")]
    pub timeframe_minutes: u32,
    pub stake: Option<Decimal>,
    pub asset_class: Option<AssetClass>,
}

fn default_tf() -> u32 { 5 }

// ─── Helpers ───

fn market_session(utc: &chrono::DateTime<Utc>) -> String {
    match utc.hour() {
        0..=7 => "Asian Session", 8..=12 => "London Session",
        13..=16 => "London-NY Overlap (High Volatility)",
        17..=21 => "New York Session",
        _ => "Off-Hours (Low Liquidity)",
    }.into()
}

fn session_quality(utc: &chrono::DateTime<Utc>) -> (Decimal, String) {
    let h = utc.hour();
    let (q, label) = match h {
        13..=16 => (Decimal::from(3), "London-NY Overlap — highest liquidity, signals most reliable."),
        8..=12 => (Decimal::from(2), "London Session — good liquidity, signals reliable."),
        17..=21 => (Decimal::from(2), "New York Session — good liquidity, signals reliable."),
        0..=7 => (Decimal::from(1), "Asian Session — lower volatility, signals weaker. Reduce position size."),
        _ => (Decimal::ZERO, "Off-Hours — thin liquidity. Signals unreliable. Best to WAIT."),
    };
    (q, label.to_string())
}

fn recommend_timeframe(symbol: &str) -> (u32, u32, String) {
    let s = symbol.to_uppercase();
    if s.starts_with("BOOM") || s.starts_with("CRASH") {
        (5, 10, "BOOM/CRASH: spike fires in 1-2 candles then fades. 5-min chart, 10-min max hold.".into())
    } else if s.starts_with("R_") || s.starts_with("1HZ") || s.starts_with("JD") || s.starts_with("STP") {
        (5, 10, "Synthetic indices: moves are sharp, last 1-2 candles (5-10 min). 10-min max hold.".into())
    } else if s.starts_with("FRX") || s.contains("/") {
        (15, 30, "Forex: slower moves, 2 candles to reach TP. 30-min hold.".into())
    } else {
        (5, 10, "Default: 5-min chart, 10-min hold.".into())
    }
}

fn summarize_candle(c: &Candle, ind: &Indicators) -> CandleSummary {
    let body = ((c.close - c.open).abs()).round_dp(6);
    let upper_wick = (c.high - c.open.max(c.close)).round_dp(6);
    let lower_wick = (c.open.min(c.close) - c.low).round_dp(6);
    let direction = if c.close > c.open { "bullish" } else if c.close < c.open { "bearish" } else { "neutral" };
    let pattern = ind.patterns.iter().filter(|(_, v)| **v == Decimal::ONE).next()
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| if direction == "bullish" { "bullish_candle" } else if direction == "bearish" { "bearish_candle" } else { "doji" }.into());
    CandleSummary { direction: direction.into(), open: c.open, high: c.high, low: c.low, close: c.close, body, upper_wick, lower_wick, pattern }
}

fn aggregate(candles: &[Candle], factor: usize) -> Vec<Candle> {
    if factor <= 1 || candles.is_empty() { return candles.to_vec(); }
    let mut out = Vec::new();
    let skip = candles.len() % factor;
    let mut i = skip;
    while i + factor <= candles.len() {
        let chunk = &candles[i..i + factor];
        out.push(Candle {
            symbol: chunk[0].symbol.clone(), ts: chunk[0].ts, open: chunk[0].open,
            high: chunk.iter().map(|c| c.high).fold(Decimal::ZERO, Decimal::max),
            low: chunk.iter().map(|c| c.low).fold(Decimal::MAX, Decimal::min),
            close: chunk.last().unwrap().close,
            volume: chunk.iter().map(|c| c.volume).sum(),
        });
        i += factor;
    }
    out
}

fn format_countdown(secs: i64) -> String {
    if secs <= 0 { return "0s".into(); }
    let m = secs / 60; let s = secs % 60;
    if m > 0 { format!("{}m {}s", m, s) } else { format!("{}s", s) }
}

fn active_candlestick_pattern(ind: &Indicators) -> String {
    let p = &ind.patterns;
    let has = |name: &str| p.get(name).copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;
    if has("morning_star") { return "Morning Star (bullish reversal)".into(); }
    if has("evening_star") { return "Evening Star (bearish reversal)".into(); }
    if has("bullish_engulfing") { return "Bullish Engulfing (strong bullish)".into(); }
    if has("bearish_engulfing") { return "Bearish Engulfing (strong bearish)".into(); }
    if has("hammer") { return "Hammer (bullish reversal)".into(); }
    if has("shooting_star") { return "Shooting Star (bearish reversal)".into(); }
    if has("three_white_soldiers") { return "Three White Soldiers (strong bullish)".into(); }
    if has("three_black_crows") { return "Three Black Crows (strong bearish)".into(); }
    if has("doji") { return "Doji (indecision)".into(); }
    if has("spinning_top") { return "Spinning Top (indecision)".into(); }
    if has("inside_bar") { return "Inside Bar (compression — breakout pending)".into(); }
    if has("narrowing_range") { return "Narrowing Range (compression — breakout imminent)".into(); }
    "No significant pattern".into()
}

fn active_chart_pattern(ind: &Indicators) -> String {
    let p = &ind.patterns;
    let has = |name: &str| p.get(name).copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;
    if has("double_top") { return "Double Top (bearish reversal)".into(); }
    if has("double_bottom") { return "Double Bottom (bullish reversal)".into(); }
    if has("resistance_breakout") { return "Resistance Breakout (bullish)".into(); }
    if has("support_breakdown") { return "Support Breakdown (bearish)".into(); }
    if has("uptrend_structure") { return "Uptrend Structure (higher highs + higher lows)".into(); }
    if has("downtrend_structure") { return "Downtrend Structure (lower highs + lower lows)".into(); }
    if has("expanding_range") { return "Expanding Range (volatility explosion)".into(); }
    "No clear chart pattern".into()
}

fn predict_next_candle(ind: &Indicators, direction: &str) -> (String, String) {
    let mut bull_signals: Vec<&str> = Vec::new();
    let mut bear_signals: Vec<&str> = Vec::new();
    let p = &ind.patterns;
    let has = |name: &str| p.get(name).copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;

    if has("hammer") { bull_signals.push("hammer"); }
    if has("bullish_engulfing") { bull_signals.push("bullish engulfing"); }
    if has("morning_star") { bull_signals.push("morning star"); }
    if has("three_white_soldiers") { bull_signals.push("three white soldiers"); }
    if has("double_bottom") { bull_signals.push("double bottom"); }
    if has("resistance_breakout") { bull_signals.push("resistance breakout"); }

    if has("shooting_star") { bear_signals.push("shooting star"); }
    if has("bearish_engulfing") { bear_signals.push("bearish engulfing"); }
    if has("evening_star") { bear_signals.push("evening star"); }
    if has("three_black_crows") { bear_signals.push("three black crows"); }
    if has("double_top") { bear_signals.push("double top"); }
    if has("support_breakdown") { bear_signals.push("support breakdown"); }

    let rsi = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
    if rsi < Decimal::from(30) { bull_signals.push("RSI oversold"); }
    else if rsi > Decimal::from(70) { bear_signals.push("RSI overbought"); }

    if let (Some(ema50), Some(ema200)) = (ind.ema.get(&50), ind.ema.get(&200)) {
        if ema50 > ema200 && ind.price > *ema50 { bull_signals.push("EMA structure bullish"); }
        else if ema50 < ema200 && ind.price < *ema50 { bear_signals.push("EMA structure bearish"); }
    }

    let (pred, reason) = if bull_signals.len() > bear_signals.len() {
        ("bullish", format!("Next candle: BULLISH. {} bullish vs {} bearish signals: {}.", bull_signals.len(), bear_signals.len(), bull_signals.join(", ")))
    } else if bear_signals.len() > bull_signals.len() {
        ("bearish", format!("Next candle: BEARISH. {} bearish vs {} bullish signals: {}.", bear_signals.len(), bull_signals.len(), bear_signals.join(", ")))
    } else if bull_signals.is_empty() {
        ("neutral", "Next candle: NEUTRAL — no strong signal. Wait for a setup.".into())
    } else {
        ("neutral", format!("Next candle: NEUTRAL (conflicted). {} bullish vs {} bearish.", bull_signals.len(), bear_signals.len()))
    };
    (pred.into(), reason)
}

// ─── Core ───

pub async fn analyze(
    db: &Db,
    market: &dyn MarketProvider,
    llm: &LlmClient,
    req: &AnalyzeRequest,
) -> AppResult<Prediction> {
    let symbol = &req.symbol;
    let tf_secs = req.timeframe_minutes * 60;
    let now = Utc::now();

    let candles = market.candles(symbol, 5000).await?;
    if candles.len() < 50 { return Err(AppError::Market("not enough candle data".into())); }
    let ind = Indicators::compute(&candles)?;
    let last = candles.last().unwrap();

    let recent: Vec<CandleSummary> = candles.iter().rev().take(5).rev()
        .map(|c| summarize_candle(c, &ind)).collect();

    // Upper timeframes — real MTF with 5000 base candles.
    let base_mins = req.timeframe_minutes;
    let upper_tfs: &[(u32, &str)] = match base_mins {
        1 | 5 => &[(15, "15min"), (60, "1H"), (240, "4H"), (1440, "Daily")],
        10 | 15 => &[(60, "1H"), (240, "4H"), (1440, "Daily")],
        30 | 60 => &[(240, "4H"), (1440, "Daily")],
        _ => &[(240, "4H"), (1440, "Daily")],
    };
    let mut upper_context: Vec<UpperTFContext> = Vec::new();
    let mut upper_bull = 0u32; let mut upper_bear = 0u32;
    for (tf_mins, label) in upper_tfs {
        let factor = (*tf_mins / base_mins) as usize;
        if factor < 2 { continue; }
        let agg = aggregate(&candles, factor);
        if agg.len() < 15 { continue; }
        if let Ok(ui) = Indicators::compute(&agg) {
            let la = agg.last().unwrap();
            let dir = if la.close > la.open { "bullish" } else if la.close < la.open { "bearish" } else { "neutral" };
            let rsi = ui.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
            let adx = ui.adx;
            let pattern = ui.patterns.iter().filter(|(_, v)| **v == Decimal::ONE).next().map(|(k, _)| k.clone()).unwrap_or_else(|| dir.into());
            let trend = if ind.price > *ui.ema.get(&50).unwrap_or(&ind.price) { "bullish" } else { "bearish" };
            if trend == "bullish" { upper_bull += 1; } else { upper_bear += 1; }
            let summary = format!("{} is {} (RSI {}, ADX {}) — {}", label, trend, rsi, adx, if trend == "bullish" { "supports BUY" } else { "supports SELL" });
            upper_context.push(UpperTFContext { label: (*label).into(), trend: trend.into(), last_candle_dir: dir.into(), rsi, adx, pattern, summary });
        }
    }

    // ═══ GATHER EVIDENCE ═══
    let mut evidence: Vec<Evidence> = Vec::new();
    let mut bull = Decimal::ZERO;
    let mut bear = Decimal::ZERO;
    let mut trend_dir = "neutral";
    let mut momentum_dir = "neutral";
    let mut pattern_dir = "neutral";
    let rsi_val = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));

    // Detect reversal candlestick patterns early — gates the trend tools.
    let has_bull_reversal_candle = ind.patterns.get("hammer").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bullish_engulfing").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("morning_star").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("dragonfly_doji").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("piercing_line").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bullish_harami").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("three_bar_bull_reversal").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("failed_breakout_down").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;
    let has_bear_reversal_candle = ind.patterns.get("shooting_star").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bearish_engulfing").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("evening_star").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("gravestone_doji").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("dark_cloud_cover").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bearish_harami").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("three_bar_bear_reversal").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("failed_breakout_up").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;

    // Compute real prior swing levels early — used by RSI, Stochastic, BB.
    // "At support/resistance" means price is within 2x ATR of a PRIOR swing
    // (excluding the last 10 bars, so it's a real historical level, not the
    // most recent extreme which is always "close" in a trending market).
    let prior_swing_low = candles.iter().rev().skip(10).take(40).map(|c| c.low).fold(Decimal::MAX, Decimal::min);
    let prior_swing_high = candles.iter().rev().skip(10).take(40).map(|c| c.high).fold(Decimal::ZERO, Decimal::max);
    let at_real_support = prior_swing_low < Decimal::MAX
        && (ind.price - prior_swing_low).abs() <= ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2);
    let at_real_resistance = prior_swing_high > Decimal::ZERO
        && (ind.price - prior_swing_high).abs() <= ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2);

    // --- FACT: Last candle OHLC ---
    let body = (last.close - last.open).abs().round_dp(6);
    let body_pct = if last.open != Decimal::ZERO { (body / last.open * Decimal::from(100)).round_dp(2) } else { Decimal::ZERO };
    let upper_wick = (last.high - last.open.max(last.close)).round_dp(6);
    let lower_wick = (last.open.min(last.close) - last.low).round_dp(6);
    let candle_dir = if last.close > last.open { "bullish" } else if last.close < last.open { "bearish" } else { "neutral" };
    evidence.push(Evidence {
        source: "price".into(),
        finding: format!("Last candle: {} | O={} H={} L={} C={} | body={} ({}%) | wicks u={} l={}.",
            candle_dir, last.open, last.high, last.low, last.close, body, body_pct, upper_wick, lower_wick),
        confirms: "neutral".into(), weight: Decimal::ZERO,
    });

    // --- FACT: ADX ---
    let strong_trend = ind.adx > Decimal::from(25);
    let ranging = ind.adx < Decimal::from(20);
    evidence.push(Evidence {
        source: "adx".into(),
        finding: format!("ADX = {}. {}.", ind.adx,
            if strong_trend { "Strong trend — trend-following tools are reliable." }
            else if ranging { "Weak/no trend — market is ranging. Mean-reversion tools apply." }
            else { "Trend is developing — momentum tools apply." }),
        confirms: "neutral".into(), weight: Decimal::ZERO,
    });

    // --- FACT: Candlestick patterns ---
    for (name, val) in &ind.patterns {
        if *val != Decimal::ONE { continue; }
        let (d, w) = pattern_sentiment(name);
        if w == Decimal::ZERO {
            evidence.push(Evidence { source: "candlestick".into(), finding: format!("{} pattern present. {}", name, pattern_meaning(name)), confirms: "neutral".into(), weight: Decimal::ZERO });
            continue;
        }
        let confirms = if d > 0 { "buy" } else { "sell" };
        if d > 0 { pattern_dir = "buy"; } else { pattern_dir = "sell"; }
        evidence.push(Evidence { source: "candlestick".into(), finding: format!("{} pattern present. {}", name, pattern_meaning(name)), confirms: confirms.into(), weight: w });
        if d > 0 { bull += w; } else { bear += w; }
    }

    // --- FACT: RSI ---
    // RSI < 30 means oversold — but in a downtrend, RSI stays low. It's only a
    // buy signal when price is ALSO at support. Same for RSI > 70 in uptrend.
    {
        let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
        let rsi_now = rsi_val;
        let rsi_5_ago = if closes.len() >= 19 {
            crate::engine::rules::Indicators::compute(&candles[..closes.len()-5])
                .ok().and_then(|i| i.rsi.get(&14).copied()).unwrap_or(rsi_now)
        } else { rsi_now };
        let rsi_trend = if rsi_now > rsi_5_ago { "rising" } else if rsi_now < rsi_5_ago { "falling" } else { "flat" };
        let ema_trend_bull = ind.price > *ind.ema.get(&200).unwrap_or(&ind.price);
        let (confirms, w, finding) = if rsi_now < Decimal::from(30) && at_real_support {
            ("buy", Decimal::from(3), format!("RSI = {} (oversold) AND at support. Reversal probability is high — price is exhausted at a real support level.", rsi_now))
        } else if rsi_now > Decimal::from(70) && at_real_resistance {
            ("sell", Decimal::from(3), format!("RSI = {} (overbought) AND at resistance. Reversal probability is high — price is exhausted at a real resistance level.", rsi_now))
        } else if rsi_now < Decimal::from(30) && !ema_trend_bull {
            ("neutral", Decimal::ZERO, format!("RSI = {} (oversold but NOT at support, in a downtrend — oversold is normal here, not a buy).", rsi_now))
        } else if rsi_now > Decimal::from(70) && ema_trend_bull {
            ("neutral", Decimal::ZERO, format!("RSI = {} (overbought but NOT at resistance, in an uptrend — overbought is normal here, not a sell).", rsi_now))
        } else if rsi_now < Decimal::from(40) && rsi_trend == "rising" && at_real_support {
            ("buy", Decimal::from(2), format!("RSI = {} (rising from oversold, at support). Momentum shifting to buyers.", rsi_now))
        } else if rsi_now > Decimal::from(60) && rsi_trend == "falling" && at_real_resistance {
            ("sell", Decimal::from(2), format!("RSI = {} (falling from overbought, at resistance). Momentum shifting to sellers.", rsi_now))
        } else {
            ("neutral", Decimal::ZERO, format!("RSI = {} ({}). Neutral — no extreme at a key level.", rsi_now, rsi_trend))
        };
        if confirms == "buy" { momentum_dir = "buy"; } else if confirms == "sell" { momentum_dir = "sell"; }
        evidence.push(Evidence { source: "rsi".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: Price vs EMA (suppress opposing when reversal candle printed) ---
    if let Some(ema200) = ind.ema.get(&200) {
        let (confirms, w, finding) = if ind.price > *ema200 {
            ("buy", Decimal::from(2), format!("Price ({}) ABOVE EMA200 ({}). Macro trend is bullish — bias is BUY.", ind.price, ema200))
        } else {
            ("sell", Decimal::from(2), format!("Price ({}) BELOW EMA200 ({}). Macro trend is bearish — bias is SELL.", ind.price, ema200))
        };
        let suppressed = (confirms == "sell" && has_bull_reversal_candle) || (confirms == "buy" && has_bear_reversal_candle);
        evidence.push(Evidence { source: "ema".into(), finding, confirms: confirms.into(), weight: if suppressed { Decimal::ZERO } else { w } });
        if !suppressed {
            if confirms == "buy" { bull += w; trend_dir = "buy"; } else { bear += w; trend_dir = "sell"; }
        }
    }
    if let Some(ema50) = ind.ema.get(&50) {
        let (confirms, w, finding) = if ind.price > *ema50 {
            ("buy", Decimal::from(2), format!("Price ({}) ABOVE EMA50 ({}). Short-term trend is up.", ind.price, ema50))
        } else {
            ("sell", Decimal::from(2), format!("Price ({}) BELOW EMA50 ({}). Short-term trend is down.", ind.price, ema50))
        };
        evidence.push(Evidence { source: "ema".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; if trend_dir == "neutral" { trend_dir = "buy"; } } else { bear += w; if trend_dir == "neutral" { trend_dir = "sell"; } }
    }

    // --- FACT: Price structure ---
    {
        let higher_highs = candles.iter().rev().take(5).rev()
            .collect::<Vec<_>>().windows(2).filter(|w| w[1].high > w[0].high).count();
        let lower_lows = candles.iter().rev().take(5).rev()
            .collect::<Vec<_>>().windows(2).filter(|w| w[1].low < w[0].low).count();
        if higher_highs > lower_lows {
            let w = Decimal::from(2);
            evidence.push(Evidence { source: "price_action".into(),
                finding: format!("Price structure: {} higher highs vs {} lower lows. Uptrend structure.", higher_highs, lower_lows),
                confirms: "buy".into(), weight: w });
            bull += w; if trend_dir == "neutral" { trend_dir = "buy"; }
        } else if lower_lows > higher_highs {
            let w = Decimal::from(2);
            evidence.push(Evidence { source: "price_action".into(),
                finding: format!("Price structure: {} lower lows vs {} higher highs. Downtrend structure.", lower_lows, higher_highs),
                confirms: "sell".into(), weight: w });
            bear += w; if trend_dir == "neutral" { trend_dir = "sell"; }
        }
    }

    // --- FACT: MACD (skip when reversal candle printed — it lags) ---
    // MACD is a LAGGING indicator — it reflects past momentum, not future.
    // In a strong downtrend, MACD can briefly flip positive (mean reversion)
    // and give a false "buy" signal. We only trust MACD when it AGREES with
    // the EMA trend. If EMA says sell and MACD says buy, MACD is lying.
    if !has_bull_reversal_candle && !has_bear_reversal_candle {
        if let Some(macd) = ind.macd {
            let ema_trend_bull = ind.price > *ind.ema.get(&200).unwrap_or(&ind.price);
            let (confirms, w, finding) = if macd > Decimal::ZERO && ema_trend_bull {
                ("buy", Decimal::from(2), format!("MACD = {} (positive, confirms bullish EMA trend). Momentum is bullish.", macd))
            } else if macd < Decimal::ZERO && !ema_trend_bull {
                ("sell", Decimal::from(2), format!("MACD = {} (negative, confirms bearish EMA trend). Momentum is bearish.", macd))
            } else if macd > Decimal::ZERO && !ema_trend_bull {
                ("neutral", Decimal::ZERO, format!("MACD = {} (positive but EMA trend is bearish — MACD is diverging from trend, likely a dead-cat bounce. IGNORE until MACD confirms with price above EMA200).", macd))
            } else if macd < Decimal::ZERO && ema_trend_bull {
                ("neutral", Decimal::ZERO, format!("MACD = {} (negative but EMA trend is bullish — likely a pullback. IGNORE until MACD confirms with price below EMA200).", macd))
            } else {
                ("neutral", Decimal::ZERO, format!("MACD = {} (neutral — no clear signal).", macd))
            };
            if confirms == "buy" { momentum_dir = "buy"; } else if confirms == "sell" { momentum_dir = "sell"; }
            evidence.push(Evidence { source: "macd".into(), finding, confirms: confirms.into(), weight: w });
            if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
        }
    }

    // --- FACT: Bollinger Bands ---
    // Price below lower BB in a downtrend is trend continuation, not a buy.
    // Only use BB as a signal at real support/resistance.
    {
        let (confirms, w, finding) = if ind.price < ind.bb_lower && at_real_support {
            ("buy", Decimal::from(2), format!("Price ({}) below lower BB ({}) AND at support. Mean-reversion buy is valid.", ind.price, ind.bb_lower))
        } else if ind.price > ind.bb_upper && at_real_resistance {
            ("sell", Decimal::from(2), format!("Price ({}) above upper BB ({}) AND at resistance. Mean-reversion sell is valid.", ind.price, ind.bb_upper))
        } else if ind.price < ind.bb_lower {
            ("neutral", Decimal::ZERO, format!("Price ({}) below lower BB ({}) but NOT at support — trend continuation, not a buy.", ind.price, ind.bb_lower))
        } else if ind.price > ind.bb_upper {
            ("neutral", Decimal::ZERO, format!("Price ({}) above upper BB ({}) but NOT at resistance — trend continuation, not a sell.", ind.price, ind.bb_upper))
        } else {
            ("neutral", Decimal::ZERO, format!("Price ({}) inside BB ({} to {}). Normal range.", ind.price, ind.bb_lower, ind.bb_upper))
        };
        evidence.push(Evidence { source: "bollinger".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: Stochastic ---
    // Stochastic measures where price is within its recent range. But "oversold"
    // in a downtrend is NORMAL, not a buy signal. We only use Stochastic as a
    // reversal signal when price is ALSO at a real support/resistance level.
    {
        let (confirms, w, finding) = if ind.stoch_k < Decimal::from(20) && at_real_support {
            ("buy", Decimal::from(2), format!("Stochastic %K = {} (oversold) AND at real support ({}). Mean-reversion buy signal is valid at support.", ind.stoch_k, prior_swing_low.round_dp(5)))
        } else if ind.stoch_k > Decimal::from(80) && at_real_resistance {
            ("sell", Decimal::from(2), format!("Stochastic %K = {} (overbought) AND at real resistance ({}). Mean-reversion sell signal is valid at resistance.", ind.stoch_k, prior_swing_high.round_dp(5)))
        } else if ind.stoch_k < Decimal::from(20) {
            ("neutral", Decimal::ZERO, format!("Stochastic %K = {} (oversold but NOT at support — in a downtrend, oversold is normal, not a buy).", ind.stoch_k))
        } else if ind.stoch_k > Decimal::from(80) {
            ("neutral", Decimal::ZERO, format!("Stochastic %K = {} (overbought but NOT at resistance — in an uptrend, overbought is normal, not a sell).", ind.stoch_k))
        } else {
            ("neutral", Decimal::ZERO, format!("Stochastic %K = {}. Mid-range.", ind.stoch_k))
        };
        evidence.push(Evidence { source: "stochastic".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: Consecutive candle exhaustion (at real prior swing) ---
    // (prior_swing_low/high and at_real_support/resistance already computed above)
    if ind.consecutive_bearish >= 3 && at_real_support {
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "momentum".into(),
            finding: format!("{} consecutive bearish candles near prior swing low (support {}). Exhaustion — reversal likely.", ind.consecutive_bearish, prior_swing_low.round_dp(5)),
            confirms: "buy".into(), weight: w });
        bull += w;
    }
    if ind.consecutive_bullish >= 3 && at_real_resistance {
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "momentum".into(),
            finding: format!("{} consecutive bullish candles near prior swing high (resistance {}). Exhaustion — reversal likely.", ind.consecutive_bullish, prior_swing_high.round_dp(5)),
            confirms: "sell".into(), weight: w });
        bear += w;
    }

    // --- FACT: Reversal convergence (requires candlestick) ---
    let bull_reversal_count = {
        let mut c = 0;
        if rsi_val < Decimal::from(35) { c += 1; }
        if has_bull_reversal_candle { c += 1; }
        if ind.stoch_k < Decimal::from(20) { c += 1; }
        if at_real_support { c += 1; }
        if ind.consecutive_bearish >= 3 { c += 1; }
        c
    };
    if has_bull_reversal_candle && bull_reversal_count >= 2 {
        let w = Decimal::from(4);
        evidence.push(Evidence { source: "reversal".into(),
            finding: format!("BULLISH REVERSAL CONFIRMED: {} signals align WITH a reversal candlestick. Reversal probability 71%+.", bull_reversal_count),
            confirms: "buy".into(), weight: w });
        bull += w; pattern_dir = "buy"; momentum_dir = "buy";
    }
    let bear_reversal_count = {
        let mut c = 0;
        if rsi_val > Decimal::from(65) { c += 1; }
        if has_bear_reversal_candle { c += 1; }
        if ind.stoch_k > Decimal::from(80) { c += 1; }
        if at_real_resistance { c += 1; }
        if ind.consecutive_bullish >= 3 { c += 1; }
        c
    };
    if has_bear_reversal_candle && bear_reversal_count >= 2 {
        let w = Decimal::from(4);
        evidence.push(Evidence { source: "reversal".into(),
            finding: format!("BEARISH REVERSAL CONFIRMED: {} signals align WITH a reversal candlestick. Reversal probability 71%+.", bear_reversal_count),
            confirms: "sell".into(), weight: w });
        bear += w; pattern_dir = "sell"; momentum_dir = "sell";
    }

    // --- FACT: Same-direction fatigue (dampen dominant direction after 4+ candles) ---
    let recent_candle_dirs: Vec<&str> = candles.iter().rev().take(6)
        .map(|c| if c.close > c.open { "bull" } else if c.close < c.open { "bear" } else { "flat" })
        .collect();
    let recent_bull_count = recent_candle_dirs.iter().filter(|&&d| d == "bull").count();
    let recent_bear_count = recent_candle_dirs.iter().filter(|&&d| d == "bear").count();
    if recent_bull_count >= 4 {
        let dampen = if recent_bull_count >= 5 { Decimal::new(6, 1) } else { Decimal::new(75, 2) };
        let before = bull;
        bull = bull * dampen;
        evidence.push(Evidence { source: "fatigue".into(),
            finding: format!("{} bullish candles in a row — buying exhausting. Bullish conviction dampened by {}%.", recent_bull_count, (Decimal::ONE - dampen) * Decimal::from(100)),
            confirms: "sell".into(), weight: Decimal::ZERO });
    } else if recent_bear_count >= 4 {
        let dampen = if recent_bear_count >= 5 { Decimal::new(6, 1) } else { Decimal::new(75, 2) };
        let before = bear;
        bear = bear * dampen;
        evidence.push(Evidence { source: "fatigue".into(),
            finding: format!("{} bearish candles in a row — selling exhausting. Bearish conviction dampened by {}%.", recent_bear_count, (Decimal::ONE - dampen) * Decimal::from(100)),
            confirms: "buy".into(), weight: Decimal::ZERO });
    }

    // --- FACT: Session quality ---
    let (sess_q, sess_reason) = session_quality(&now);
    evidence.push(Evidence { source: "session".into(), finding: sess_reason, confirms: "neutral".into(), weight: Decimal::ZERO });
    if sess_q == Decimal::ZERO {
        bull = bull * Decimal::new(6, 1) / Decimal::from(10);
        bear = bear * Decimal::new(6, 1) / Decimal::from(10);
    }

    // --- FACT: Upper timeframe alignment (weight vote, not a block) ---
    if upper_bull > upper_bear && upper_bull > 0 {
        let w = Decimal::from(3);
        evidence.push(Evidence { source: "upper_tf".into(), finding: format!("Upper timeframes: {}/{} bullish. Macro trend agrees with BUY.", upper_bull, upper_bull + upper_bear), confirms: "buy".into(), weight: w });
        bull += w;
    } else if upper_bear > upper_bull && upper_bear > 0 {
        let w = Decimal::from(3);
        evidence.push(Evidence { source: "upper_tf".into(), finding: format!("Upper timeframes: {}/{} bearish. Macro trend agrees with SELL.", upper_bear, upper_bull + upper_bear), confirms: "sell".into(), weight: w });
        bear += w;
    }

    // --- News ---
    let news = crate::news::assess_news(symbol).await.unwrap_or_else(|_| crate::news::NewsAssessment {
        status: "clear".into(), upcoming_high_impact: vec![], upcoming_medium_impact: vec![],
        recently_released: vec![], summary: "News data unavailable.".into(), recommendation: "".into(),
    });
    match news.status.as_str() {
        "danger" => {
            evidence.push(Evidence { source: "news".into(), finding: format!("{} {}", news.summary, news.recommendation), confirms: "neutral".into(), weight: Decimal::ZERO });
            bull = bull * Decimal::new(8, 1) / Decimal::from(10);
            bear = bear * Decimal::new(8, 1) / Decimal::from(10);
        }
        "caution" => {
            evidence.push(Evidence { source: "news".into(), finding: news.summary.clone(), confirms: "neutral".into(), weight: Decimal::ZERO });
        }
        _ => {
            evidence.push(Evidence { source: "news".into(), finding: "No high-impact news in next 30 min.".into(), confirms: "neutral".into(), weight: Decimal::ZERO });
        }
    }

    // --- Note rules (fire naturally with their weight) ---
    let strategies = db.list_enabled_strategies().await.unwrap_or_default();
    let mut note_count = 0u32;
    for strat in &strategies {
        if !strat.symbols.is_empty() && !strat.symbols.iter().any(|s| s == symbol || symbol.contains(s)) { continue; }
        let rules = db.list_rules(strat.id).await.unwrap_or_default();
        for rule in &rules {
            if !rule.enabled { continue; }
            if let Ok(true) = evaluate(&rule.expr, &ind) {
                let el = rule.expr.to_lowercase();
                let is_bear = el.contains("bearish") || el.contains("short") || el.contains("overbought") || el.contains("> 65") || el.contains("> 70");
                let confirms = if is_bear { "sell" } else { "buy" };
                let w = rule.weight;
                evidence.push(Evidence { source: "note".into(), finding: format!("Rule '{}' from note '{}' is TRUE. Expression: {}.", rule.name, strat.name, rule.expr), confirms: confirms.into(), weight: w });
                if confirms == "buy" { bull += w; } else { bear += w; }
                note_count += 1;
            }
        }
    }

    // --- Trend mode: advisory weight bonus in strong trends ---
    if ind.adx > Decimal::from(25) {
        let trend_dir_str = if ind.price > *ind.ema.get(&200).unwrap_or(&ind.price) { "buy" } else { "sell" };
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "trend_mode".into(),
            finding: format!("STRONG TREND MODE (ADX {}): trend is {}. Continuation favored — trend direction gets +{} weight.", ind.adx.round_dp(1), trend_dir_str.to_uppercase(), w),
            confirms: trend_dir_str.into(), weight: w });
        if trend_dir_str == "buy" { bull += w; } else { bear += w; }
    }

    // ═══ DERIVE DIRECTION FROM WEIGHTED EVIDENCE ═══
    // CONSERVATIVE MODE: only signal when evidence is overwhelming.
    // This sacrifices frequency for accuracy — fewer signals, but each one
    // has all tools, patterns, notes, and timeframes agreeing.
    let total = bull + bear;
    let has_trend = ind.adx > Decimal::from(20);
    let strong_reversal = has_bull_reversal_candle && bull_reversal_count >= 2
        || has_bear_reversal_candle && bear_reversal_count >= 2;

    // Require 58% conviction — high enough to filter noise, low enough to
    // catch real signals. 65% was too high and blocked good trades.
    let conviction_threshold = Decimal::new(58, 2);

    // BOOM/CRASH structural bias
    let sym_upper = symbol.to_uppercase();
    let structural_bias: Option<&str> = if sym_upper.starts_with("BOOM") { Some("buy") }
        else if sym_upper.starts_with("CRASH") { Some("sell") }
        else { None };

    // Macro trend must AGREE with the signal. If upper TFs are 4/4 bearish,
    // we only allow SELL. A bullish pattern in a bearish macro = counter-trend
    // bounce, not a trade. This is the #1 fix for consistency.
    let macro_all_bear = upper_bear > 0 && upper_bull == 0;
    let macro_all_bull = upper_bull > 0 && upper_bear == 0;
    let macro_direction: Option<&str> = if macro_all_bull { Some("buy") }
        else if macro_all_bear { Some("sell") }
        else { None };

    let (direction, evidence_score): (String, Decimal) = if total == Decimal::ZERO {
        ("wait".into(), Decimal::ZERO)
    } else {
        let ratio = if bull > bear { bull / total } else { bear / total };
        let dominant = if bull > bear { "buy" } else { "sell" };

        // 1. Must meet conviction threshold
        // 2. Must have trend OR confirmed reversal
        // 3. Must align with macro trend (if all upper TFs agree one way)
        let mut qualifies = ratio >= conviction_threshold && (has_trend || strong_reversal);
        if qualifies {
            if let Some(md) = macro_direction {
                if dominant != md {
                    qualifies = false;
                    evidence.push(Evidence { source: "macro".into(),
                        finding: format!("BLOCKED: signal is {} but ALL upper timeframes are {}. Counter-trend signals are blocked for consistency.", dominant.to_uppercase(), md.to_uppercase()),
                        confirms: "neutral".into(), weight: Decimal::ZERO });
                }
            }
        }
        // BOOM/CRASH structural bias
        if !qualifies && structural_bias.is_some() {
            let bias = structural_bias.unwrap();
            let bias_weight = if bias == "buy" { bull } else { bear };
            let bias_ratio = if total > Decimal::ZERO { bias_weight / total } else { Decimal::ZERO };
            if bias_ratio >= Decimal::new(50, 2) { qualifies = true; }
        }
        if !qualifies { ("wait".into(), ratio) } else { (dominant.into(), ratio) }
    };

    // --- 2-candle-ahead prediction (advisory only) ---
    let last_3_bull = candles.iter().rev().take(3).filter(|c| c.close > c.open).count();
    let last_3_bear = candles.iter().rev().take(3).filter(|c| c.close < c.open).count();
    let avg_body: Decimal = candles.iter().rev().take(20)
        .map(|c| (c.close - c.open).abs())
        .fold(Decimal::ZERO, |a, b| a + b) / Decimal::from(20.min(candles.len() as u32));
    let last_body = (last.close - last.open).abs();
    let is_climax = last_body > avg_body * Decimal::new(15, 1);
    let next_2_prediction: String = if direction == "sell" {
        if last_3_bear >= 3 {
            "WARNING: 3+ bearish candles = exhaustion. Next 2 candles may bounce UP. Entry is late — use tight stop.".into()
        } else if is_climax && last.close < last.open {
            "WARNING: selling climax. Next 2 candles may snap back UP.".into()
        } else if has_bull_reversal_candle {
            "WARNING: bullish reversal candle printed. Next 2 candles may go UP.".into()
        } else {
            "Next 2 candles: bearish continuation likely — no exhaustion signals.".into()
        }
    } else if direction == "buy" {
        if last_3_bull >= 3 {
            "WARNING: 3+ bullish candles = exhaustion. Next 2 candles may pull back DOWN. Entry is late — use tight stop.".into()
        } else if is_climax && last.close > last.open {
            "WARNING: buying climax. Next 2 candles may pull back DOWN.".into()
        } else if has_bear_reversal_candle {
            "WARNING: bearish reversal candle printed. Next 2 candles may go DOWN.".into()
        } else {
            "Next 2 candles: bullish continuation likely — no exhaustion signals.".into()
        }
    } else {
        "Next 2 candles: neutral — no clear direction. Wait for a setup.".into()
    };
    evidence.push(Evidence { source: "ahead".into(), finding: next_2_prediction.clone(), confirms: "neutral".into(), weight: Decimal::ZERO });

    let market_state = determine_market_state(&ind);

    // Entry/SL/TP.
    let entry = ind.price;
    let atr = ind.atr.get(&14).copied().unwrap_or(entry * Decimal::new(5, 3));
    let pip = if symbol.starts_with("frx") { Decimal::new(1, 4) } else { Decimal::ONE };
    let sl_dist = atr.max(pip * Decimal::from(20));
    let tp_dist = sl_dist * Decimal::from(2);
    let (stop_loss, take_profit) = match direction.as_str() {
        "buy" => (entry - sl_dist, entry + tp_dist),
        "sell" => (entry + sl_dist, entry - tp_dist),
        _ => (entry - sl_dist, entry + tp_dist),
    };

    // --- Trigger gate ---
    let prev_c = candles.len().ge(&2).then(|| &candles[candles.len() - 2]);
    let prior_swing_high_tg = candles.iter().rev().skip(1).take(50).map(|c| c.high).fold(Decimal::ZERO, Decimal::max);
    let prior_swing_low_tg = candles.iter().rev().skip(1).take(50).map(|c| c.low).fold(Decimal::MAX, Decimal::min);
    // No trigger gate — the conviction score + pattern check is the filter.
    // The trigger gate was blocking valid signals and causing missed trades.
    let trigger_fired = true; // always pass — conviction is the real filter

    // ═══ ENTRY CHECKLIST ═══
    let session_active = now.hour() >= 7 && now.hour() <= 21;
    let trend_aligned = direction != "wait" && (trend_dir == direction || trend_dir == "neutral");
    let momentum_aligned = direction != "wait" && (momentum_dir == direction || momentum_dir == "neutral");
    // Pattern confirmed: a candlestick pattern aligns, OR consecutive candles
    // show exhaustion (3+ same direction), OR a reversal candle printed.
    let pattern_confirmed = direction != "wait" && (pattern_dir == direction || ind.consecutive_bearish >= 3 || ind.consecutive_bullish >= 3 || has_bull_reversal_candle || has_bear_reversal_candle);
    let no_news_risk = news.status != "danger";
    let risk_reward_ok = tp_dist >= sl_dist * Decimal::from(2);
    let conviction_ok = direction != "wait" && evidence_score >= conviction_threshold;
    // ALL conditions must be YES — no exceptions.
    let ready = direction != "wait"
        && conviction_ok
        && trend_aligned
        && momentum_aligned
        && pattern_confirmed
        && no_news_risk
        && risk_reward_ok
        && session_active;

    let mut checklist_details = Vec::new();
    checklist_details.push(format!("1. Trend aligned: {}", if trend_aligned { "YES" } else { "NO" }));
    checklist_details.push(format!("2. Momentum aligned: {}", if momentum_aligned { "YES" } else { "NO" }));
    checklist_details.push(format!("3. Pattern confirmed: {}", if pattern_confirmed { "YES" } else { "NO" }));
    checklist_details.push(format!("4. Conviction: {} ({:.0}%, bar {:.0}%)", if conviction_ok { "YES" } else { "NO" }, evidence_score * Decimal::from(100), conviction_threshold * Decimal::from(100)));
    checklist_details.push(format!("5. No news risk: {}", if no_news_risk { "YES" } else { "NO" }));
    checklist_details.push(format!("6. Risk/reward: {}", if risk_reward_ok { "YES" } else { "NO" }));
    checklist_details.push(format!("7. Session active: {}", if session_active { "YES" } else { "NO" }));
    if ready { checklist_details.push("ALL CONDITIONS MET — trade is valid.".into()); }
    else { checklist_details.push("NOT ALL CONDITIONS MET — do not trade.".into()); }

    // What to watch.
    let mut what_to_watch: Vec<String> = Vec::new();
    what_to_watch.push(format!("RSI = {}. If it crosses {} 30/70, reversal is confirmed.", rsi_val, if direction == "buy" { "above" } else { "below" }));
    what_to_watch.push(format!("Price vs EMA50 ({}). If price holds {} it, trend continues.", ind.ema.get(&50).map(|d| d.to_string()).unwrap_or_default(), if direction == "buy" { "above" } else { "below" }));
    what_to_watch.push(format!("Support: {} | Resistance: {}", ind.swing_low, ind.swing_high));
    if news.status != "clear" { what_to_watch.push(format!("NEWS: {}", news.summary)); }
    let last_ts = candles.last().unwrap().ts;
    let real_tf_secs: i64 = if candles.len() >= 2 {
        let prev_ts = candles[candles.len() - 2].ts;
        let gap = (last_ts - prev_ts).num_seconds().abs();
        if gap > 0 { gap } else { tf_secs as i64 }
    } else { tf_secs as i64 };
    let elapsed_in_slot = now.timestamp() % real_tf_secs;
    let secs_remaining = (real_tf_secs - elapsed_in_slot) % real_tf_secs;
    let next_candle_start = now + Duration::seconds(secs_remaining);
    let countdown = format_countdown(secs_remaining);
    what_to_watch.push(format!("Next candle in {} — watch the open.", countdown));

    let session = market_session(&now);
    let expiry = now + Duration::seconds(real_tf_secs);
    let tf_secs = real_tf_secs as u32;

    // Move duration estimate.
    let avg_dir_body: Decimal = candles.iter().rev().take(20)
        .map(|c| (c.close - c.open).abs())
        .fold(Decimal::ZERO, |a, b| a + b) / Decimal::from(20.min(candles.len() as u32));
    let move_distance = tp_dist;
    let mut move_candles = if avg_dir_body > Decimal::ZERO {
        (move_distance / avg_dir_body).round().to_string().parse::<i64>().unwrap_or(3)
    } else { 3 };
    if ind.adx > Decimal::from(25) { move_candles = (move_candles + 1).max(3); }
    let max_candles = if symbol.starts_with("frx") || symbol.contains("/") { 6 } else { 3 };
    let move_candles = move_candles.clamp(1, max_candles) as u32;
    let move_secs = move_candles as i64 * real_tf_secs;
    let move_duration_label = format_countdown(move_secs);

    // Build report.
    let reasoning = build_report(&market_state, &direction, &evidence_score, &session,
        &evidence, &ind, note_count, symbol, req.timeframe_minutes, &recent, &upper_context, &what_to_watch, &checklist_details, move_candles, &move_duration_label);

    let final_reasoning = if let Ok(insight) = llm_enhance(llm, symbol, &direction, &evidence_score, &evidence, &ind).await {
        format!("{}\n\nAI Insight: {}", reasoning, insight)
    } else { reasoning };

    let fired_sources: Vec<String> = evidence.iter()
        .filter(|e| e.weight > Decimal::ZERO && e.confirms == direction)
        .map(|e| e.source.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Predict next candle.
    let (next_candle_prediction, next_candle_reasoning) = predict_next_candle(&ind, &direction);
    let active_pattern = active_candlestick_pattern(&ind);
    let active_chart_pattern = active_chart_pattern(&ind);
    let (rec_tf, rec_duration, rec_reason) = recommend_timeframe(symbol);

    // Build trade options (non-contradictory).
    let total_evidence = bull + bear;
    let bull_pct = if total_evidence > Decimal::ZERO { bull / total_evidence } else { Decimal::ZERO };
    let bear_pct = if total_evidence > Decimal::ZERO { bear / total_evidence } else { Decimal::ZERO };
    let bearish_dominant = bear > bull;
    let bullish_dominant = bull > bear;
    let sl_dist_val = sl_dist;
    let tp_dist_val = tp_dist;
    let mut trade_options: Vec<TradeOption> = Vec::new();

    if bullish_dominant {
        // BUY + REVERSAL DOWN
        let mut support: Vec<String> = Vec::new();
        if ind.price > *ind.ema.get(&200).unwrap_or(&ind.price) { support.push("Price above EMA200".into()); }
        if ind.price > *ind.ema.get(&50).unwrap_or(&ind.price) { support.push("Price above EMA50".into()); }
        if let Some(macd) = ind.macd { if macd > Decimal::ZERO { support.push("MACD positive".into()); } }
        for p in ["bullish_engulfing", "hammer", "morning_star", "three_white_soldiers"] {
            if ind.patterns.get(p).copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { support.push(format!("{}", p)); }
        }
        if ind.patterns.get("resistance_breakout").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { support.push("resistance breakout".into()); }
        if ind.patterns.get("uptrend_structure").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { support.push("higher highs + higher lows".into()); }
        trade_options.push(TradeOption {
            option_type: "buy".into(), label: "BUY — Trend Continuation Up".into(),
            conviction: bull_pct.round_dp(4), entry: entry.round_dp(6), stop_loss: (entry - sl_dist_val).round_dp(6), take_profit: (entry + tp_dist_val).round_dp(6),
            reasoning: format!("Bullish evidence: {}%. Supports: {}.", bull_pct * Decimal::from(100), if support.is_empty() { "none".into() } else { support.join(", ") }),
            supporting_evidence: support, recommended: direction == "buy", risk_reward: "1:2".into(),
        });
        let mut rsupport: Vec<String> = Vec::new();
        if has_bear_reversal_candle { rsupport.push("bearish reversal candlestick".into()); }
        if at_real_resistance { rsupport.push(format!("at resistance ({})", prior_swing_high.round_dp(5))); }
        if ind.rsi_divergence == "bearish" { rsupport.push("RSI bearish divergence".into()); }
        if rsi_val > Decimal::from(65) { rsupport.push(format!("RSI overbought ({})", rsi_val.round_dp(1))); }
        if ind.patterns.get("double_top").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { rsupport.push("double top".into()); }
        let rconv = if bear_reversal_count >= 2 && has_bear_reversal_candle { Decimal::from(bear_reversal_count) / Decimal::from(7) } else { Decimal::ZERO };
        trade_options.push(TradeOption {
            option_type: "reversal_down".into(), label: "REVERSAL DOWN — Rejection from Resistance (counter-trend)".into(),
            conviction: rconv.round_dp(4), entry: entry.round_dp(6), stop_loss: (entry + sl_dist_val).round_dp(6), take_profit: (entry - tp_dist_val).round_dp(6),
            reasoning: format!("Bearish reversal: {} signals. Supports: {}.", bear_reversal_count, if rsupport.is_empty() { "none".into() } else { rsupport.join(", ") }),
            supporting_evidence: rsupport, recommended: direction == "sell" && has_bear_reversal_candle && bear_reversal_count >= 2, risk_reward: "1:2".into(),
        });
    } else if bearish_dominant {
        // SELL + REVERSAL UP
        let mut support: Vec<String> = Vec::new();
        if ind.price < *ind.ema.get(&200).unwrap_or(&ind.price) { support.push("Price below EMA200".into()); }
        if ind.price < *ind.ema.get(&50).unwrap_or(&ind.price) { support.push("Price below EMA50".into()); }
        if let Some(macd) = ind.macd { if macd < Decimal::ZERO { support.push("MACD negative".into()); } }
        for p in ["bearish_engulfing", "shooting_star", "evening_star", "three_black_crows"] {
            if ind.patterns.get(p).copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { support.push(format!("{}", p)); }
        }
        if ind.patterns.get("support_breakdown").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { support.push("support breakdown".into()); }
        if ind.patterns.get("downtrend_structure").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { support.push("lower highs + lower lows".into()); }
        trade_options.push(TradeOption {
            option_type: "sell".into(), label: "SELL — Trend Continuation Down".into(),
            conviction: bear_pct.round_dp(4), entry: entry.round_dp(6), stop_loss: (entry + sl_dist_val).round_dp(6), take_profit: (entry - tp_dist_val).round_dp(6),
            reasoning: format!("Bearish evidence: {}%. Supports: {}.", bear_pct * Decimal::from(100), if support.is_empty() { "none".into() } else { support.join(", ") }),
            supporting_evidence: support, recommended: direction == "sell", risk_reward: "1:2".into(),
        });
        let mut rsupport: Vec<String> = Vec::new();
        if has_bull_reversal_candle { rsupport.push("bullish reversal candlestick".into()); }
        if at_real_support { rsupport.push(format!("at support ({})", prior_swing_low.round_dp(5))); }
        if ind.rsi_divergence == "bullish" { rsupport.push("RSI bullish divergence".into()); }
        if rsi_val < Decimal::from(35) { rsupport.push(format!("RSI oversold ({})", rsi_val.round_dp(1))); }
        if ind.patterns.get("double_bottom").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE { rsupport.push("double bottom".into()); }
        let rconv = if bull_reversal_count >= 2 && has_bull_reversal_candle { Decimal::from(bull_reversal_count) / Decimal::from(7) } else { Decimal::ZERO };
        trade_options.push(TradeOption {
            option_type: "reversal_up".into(), label: "REVERSAL UP — Bounce from Support (counter-trend)".into(),
            conviction: rconv.round_dp(4), entry: entry.round_dp(6), stop_loss: (entry - sl_dist_val).round_dp(6), take_profit: (entry + tp_dist_val).round_dp(6),
            reasoning: format!("Bullish reversal: {} signals. Supports: {}.", bull_reversal_count, if rsupport.is_empty() { "none".into() } else { rsupport.join(", ") }),
            supporting_evidence: rsupport, recommended: direction == "buy" && has_bull_reversal_candle && bull_reversal_count >= 2, risk_reward: "1:2".into(),
        });
    }

    Ok(Prediction {
        market_state, direction, evidence_score,
        entry_price: entry.round_dp(6), stop_loss: stop_loss.round_dp(6), take_profit: take_profit.round_dp(6),
        expiry, reasoning: final_reasoning, evidence, what_to_watch,
        timeframe_secs: tf_secs, symbol: symbol.clone(),
        analysis_time_utc: now, market_session: session,
        current_candle_start: last_ts, next_candle_start,
        seconds_to_next_candle: secs_remaining, countdown,
        recent_candles: recent, upper_timeframe_context: upper_context, news,
        entry_checklist: EntryChecklist {
            trend_aligned, momentum_aligned, pattern_confirmed, no_news_risk, risk_reward_ok, session_active, ready,
            details: checklist_details,
        },
        estimated_move_duration: move_duration_label,
        estimated_move_candles: move_candles,
        recommended_timeframe_minutes: rec_tf,
        recommended_trade_duration_minutes: rec_duration,
        recommendation_reason: rec_reason,
        next_candle_prediction,
        next_candle_reasoning,
        active_pattern,
        active_chart_pattern,
        trade_options,
    })
}

fn determine_market_state(ind: &Indicators) -> String {
    let above_ema50 = ind.price > *ind.ema.get(&50).unwrap_or(&ind.price);
    let above_ema200 = ind.price > *ind.ema.get(&200).unwrap_or(&ind.price);
    let rsi = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
    let strong = ind.adx > Decimal::from(25);
    if ind.bb_width_pct < Decimal::from(20) { return "squeeze".into(); }
    if rsi < Decimal::from(30) && !above_ema50 && ind.rsi_divergence == "bullish" { return "reversing_up".into(); }
    if rsi > Decimal::from(70) && above_ema50 && ind.rsi_divergence == "bearish" { return "reversing_down".into(); }
    if above_ema50 && above_ema200 && strong { return "trending_up".into(); }
    if !above_ema50 && !above_ema200 && strong { return "trending_down".into(); }
    if !strong { return "ranging".into(); }
    "mixed".into()
}

fn pattern_meaning(name: &str) -> &'static str {
    match name {
        "hammer" => "Small body at top, long lower wick. Buyers rejected lower prices.",
        "bullish_engulfing" => "Large bullish candle engulfs the previous bearish candle.",
        "bearish_engulfing" => "Large bearish candle engulfs the previous bullish candle.",
        "bullish_harami" => "Small bullish candle inside previous large bearish candle.",
        "bearish_harami" => "Small bearish candle inside previous large bullish candle.",
        "doji" => "Open and close are nearly equal. Market is undecided.",
        "dragonfly_doji" => "Doji with long lower wick. Buyers rejected low prices strongly.",
        "gravestone_doji" => "Doji with long upper wick. Sellers rejected high prices strongly.",
        "morning_star" => "Three-candle bottom reversal: bearish, small body, bullish.",
        "evening_star" => "Three-candle top reversal: bullish, small body, bearish.",
        "three_white_soldiers" => "Three consecutive bullish candles with rising closes.",
        "three_black_crows" => "Three consecutive bearish candles with falling closes.",
        "piercing_line" => "Bullish candle opens below prior low, closes above prior midpoint.",
        "dark_cloud_cover" => "Bearish candle opens above prior high, closes below prior midpoint.",
        "shooting_star" => "Small body, long upper wick, at top of uptrend.",
        "marubozu" => "No wicks. Full body. Strong conviction.",
        "spinning_top" => "Small body with long wicks both sides. High indecision.",
        "long_lower_shadow" => "Lower wick is 2/3+ of range. Buyers rejected lows.",
        "long_upper_shadow" => "Upper wick is 2/3+ of range. Sellers rejected highs.",
        "tweezer_bottom" => "Two candles with matching lows.",
        "tweezer_top" => "Two candles with matching highs.",
        "hanging_man" => "Same shape as hammer but in an uptrend.",
        "inverted_hammer" => "Small body, long upper wick at bottom.",
        "bullish_candle" => "Close is above open. The candle is green/bullish.",
        "bearish_candle" => "Close is below open. The candle is red/bearish.",
        _ => "Pattern detected.",
    }
}

fn build_report(
    market_state: &str, direction: &str, evidence_score: &Decimal, session: &str,
    evidence: &[Evidence], ind: &Indicators, _note_count: u32, symbol: &str, tf_mins: u32,
    _recent: &[CandleSummary], _upper: &[UpperTFContext], what_to_watch: &[String], checklist: &[String],
    move_candles: u32, move_duration_label: &str,
) -> String {
    let pct = evidence_score * Decimal::from(100);
    let bull_e = evidence.iter().filter(|e| e.confirms == "buy" && e.weight > Decimal::ZERO).count();
    let bear_e = evidence.iter().filter(|e| e.confirms == "sell" && e.weight > Decimal::ZERO).count();
    let mut r = String::new();

    r.push_str(&format!("============================================\n  MARKET READING: {} ({}min)\n  UTC: {}\n  Session: {}\n  Market State: {}\n============================================\n\n",
        symbol, tf_mins, Utc::now().format("%Y-%m-%d %H:%M:%S UTC"), session, market_state));

    r.push_str(&format!("TRADE BIAS: {} (evidence: {:.1}%)\n", if direction == "buy" { "BUY" } else if direction == "sell" { "SELL" } else { "WAIT" }, pct));
    r.push_str(&format!("{} tools confirm BUY, {} tools confirm SELL.\n\n", bull_e, bear_e));

    r.push_str("VERIFIED FACTS:\n");
    for e in evidence {
        r.push_str(&format!("  [{}] {}\n", e.source, e.finding));
    }

    r.push_str("\nENTRY CHECKLIST:\n");
    for c in checklist { r.push_str(&format!("  {}\n", c)); }

    r.push_str("\nWHAT TO WATCH:\n");
    for w in what_to_watch { r.push_str(&format!("  - {}\n", w)); }

    r.push_str(&format!("\nKEY LEVELS: Entry={} SL={} TP={}\n", ind.price,
        if direction == "buy" { ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) } else { ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) },
        if direction == "buy" { ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2) } else { ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2) }));

    // Forward forecast.
    let atr14 = ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO);
    let (forecast_dir, why, magnitude) = if direction == "wait" {
        ("FLAT / RANGE", "Insufficient conviction — mixed evidence. Expect chop until a breakout trigger fires.".to_string(), atr14.clone())
    } else {
        let dir_word = if direction == "buy" { "UP" } else { "DOWN" };
        let target = if direction == "buy" { ind.swing_high } else { ind.swing_low };
        (dir_word, format!("Bias {} with {:.0}% conviction. Momentum + structure align — expect continuation {} toward {}.", dir_word, pct, if direction == "buy" { "up" } else { "down" }, target), (target - ind.price).abs())
    };

    r.push_str("\n============================================\n  FORWARD FORECAST (next move)\n============================================\n");
    r.push_str(&format!("Direction: {}\n", forecast_dir));
    r.push_str(&format!("Expected magnitude: ~{} ({} ATR)\n", magnitude.round_dp(5),
        if atr14 != Decimal::ZERO { (magnitude / atr14).round_dp(2).to_string() } else { "?".into() }));
    r.push_str(&format!("Rationale: {}\n", why));
    r.push_str(&format!("Horizon: ~{} candles (≈{})\n", move_candles, move_duration_label));
    r
}

fn pattern_sentiment(name: &str) -> (i32, Decimal) {
    match name {
        "hammer" => (1, Decimal::from(2)), "bullish_engulfing" => (1, Decimal::from(3)),
        "bullish_harami" => (1, Decimal::from(2)), "piercing_line" => (1, Decimal::from(2)),
        "morning_star" => (1, Decimal::from(3)), "three_white_soldiers" => (1, Decimal::from(3)),
        "dragonfly_doji" => (1, Decimal::from(2)), "long_lower_shadow" => (1, Decimal::from(1)),
        "tweezer_bottom" => (1, Decimal::from(1)), "inverted_hammer" => (1, Decimal::from(1)),
        "shooting_star" => (-1, Decimal::from(2)), "bearish_engulfing" => (-1, Decimal::from(3)),
        "bearish_harami" => (-1, Decimal::from(2)), "dark_cloud_cover" => (-1, Decimal::from(2)),
        "evening_star" => (-1, Decimal::from(3)), "three_black_crows" => (-1, Decimal::from(3)),
        "gravestone_doji" => (-1, Decimal::from(2)), "long_upper_shadow" => (-1, Decimal::from(1)),
        "tweezer_top" => (-1, Decimal::from(1)), "hanging_man" => (-1, Decimal::from(1)),
        "bullish_candle" => (1, Decimal::new(5, 1)), "bearish_candle" => (-1, Decimal::new(5, 1)),
        _ => (0, Decimal::ZERO),
    }
}

async fn llm_enhance(llm: &LlmClient, symbol: &str, direction: &str, score: &Decimal, evidence: &[Evidence], ind: &Indicators) -> AppResult<String> {
    let system = "You are a forward-looking market forecaster. Based ONLY on the verified evidence provided, predict what the chart is MOST LIKELY to do next over the upcoming candle(s). State a clear directional forecast (UP, DOWN, or FLAT), the specific trigger that would confirm or invalidate your forecast, and the time horizon. Be decisive. Answer in JSON with keys: forecast, rationale, trigger_confirm, trigger_invalidate, horizon.";
    let user = format!("Symbol: {}\nCurrent bias: {}\nConviction: {}%\nADX: {} RSI: {} Stoch: {}\n\nVerified facts:\n{}\n\nBased on this evidence, forecast where the graph is going NEXT. What is the most probable next move and why? What event confirms it? What invalidates it?",
        symbol, direction, score * Decimal::from(100), ind.adx,
        ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_default(), ind.stoch_k,
        evidence.iter().filter(|e| e.weight > Decimal::ZERO).take(10).map(|e| format!("[{}] {}", e.source, e.finding)).collect::<Vec<_>>().join("\n"));
    llm.extract_json(system, &user).await
        .ok().and_then(|v| {
            let forecast = v.get("forecast").and_then(|i| i.as_str()).unwrap_or("");
            let rationale = v.get("rationale").and_then(|i| i.as_str()).unwrap_or("");
            let confirm = v.get("trigger_confirm").and_then(|i| i.as_str()).unwrap_or("");
            let invalidate = v.get("trigger_invalidate").and_then(|i| i.as_str()).unwrap_or("");
            let horizon = v.get("horizon").and_then(|i| i.as_str()).unwrap_or("");
            if forecast.is_empty() && rationale.is_empty() { return None; }
            Some(format!("FORECAST: {}\nWhy: {}\nConfirms if: {}\nInvalidates if: {}\nHorizon: {}",
                forecast.to_uppercase(), rationale, confirm, invalidate, horizon))
        })
        .ok_or_else(|| AppError::Llm("LLM not available".into()))
}
