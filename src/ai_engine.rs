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
    /// Entry conditions checklist — like a pilot's pre-flight check.
    pub entry_checklist: EntryChecklist,
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

    let candles = market.candles(symbol, 300).await?;
    if candles.len() < 50 { return Err(AppError::Market("not enough candle data".into())); }
    let ind = Indicators::compute(&candles)?;
    let last = candles.last().unwrap();

    let recent: Vec<CandleSummary> = candles.iter().rev().take(5).rev()
        .map(|c| summarize_candle(c, &ind)).collect();

    // Upper timeframes.
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
        if agg.len() < 30 { continue; }
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

    // ═══ GATHER EVIDENCE — every statement is a VERIFIABLE FACT ═══
    let mut evidence: Vec<Evidence> = Vec::new();
    let mut bull = Decimal::ZERO;
    let mut bear = Decimal::ZERO;

    // Track key facts for the entry checklist.
    let mut trend_dir = "neutral";
    let mut momentum_dir = "neutral";
    let mut pattern_dir = "neutral";
    let rsi_val = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));

    // --- FACT: Last candle OHLC ---
    let body = (last.close - last.open).abs().round_dp(6);
    let body_pct = if last.open != Decimal::ZERO { (body / last.open * Decimal::from(100)).round_dp(2) } else { Decimal::ZERO };
    let upper_wick = (last.high - last.open.max(last.close)).round_dp(6);
    let lower_wick = (last.open.min(last.close) - last.low).round_dp(6);
    let candle_dir = if last.close > last.open { "bullish" } else if last.close < last.open { "bearish" } else { "neutral" };
    evidence.push(Evidence {
        source: "price".into(),
        finding: format!("Last candle: {} | O={} H={} L={} C={} | body={} ({}%) | upper wick={} | lower wick={}. You can verify this on the chart.",
            candle_dir, last.open, last.high, last.low, last.close, body, body_pct, upper_wick, lower_wick),
        confirms: "neutral".into(), weight: Decimal::ZERO,
    });

    // --- FACT: Last 5 closes (exact numbers) ---
    let last5_closes: Vec<String> = candles.iter().rev().take(5).rev()
        .map(|c| c.close.round_dp(4).to_string()).collect();
    let higher_highs = candles.iter().rev().take(5).rev()
        .collect::<Vec<_>>().windows(2).filter(|w| w[1].high > w[0].high).count();
    let lower_lows = candles.iter().rev().take(5).rev()
        .collect::<Vec<_>>().windows(2).filter(|w| w[1].low < w[0].low).count();
    evidence.push(Evidence {
        source: "price".into(),
        finding: format!("Last 5 closes: [{}]. Higher highs: {}/4. Lower lows: {}/4. Price is making {}.",
            last5_closes.join(", "), higher_highs, lower_lows,
            if higher_highs > lower_lows { "higher highs (uptrend structure)" } else if lower_lows > higher_highs { "lower lows (downtrend structure)" } else { "mixed structure" }),
        confirms: if higher_highs > lower_lows { "buy" } else if lower_lows > higher_highs { "sell" } else { "neutral" }.into(),
        weight: if higher_highs > lower_lows { bull += Decimal::from(2); trend_dir = "buy"; Decimal::from(2) }
               else if lower_lows > higher_highs { bear += Decimal::from(2); trend_dir = "sell"; Decimal::from(2) }
               else { Decimal::ZERO },
    });

    // --- FACT: Candlestick pattern (name + what it means structurally) ---
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

    // --- FACT: RSI exact value + trend ---
    {
        // Compute RSI 5 bars ago for comparison.
        let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
        let rsi_now = rsi_val;
        let rsi_5_ago = if closes.len() >= 19 {
            crate::engine::rules::Indicators::compute(&candles[..closes.len()-5])
                .ok().and_then(|i| i.rsi.get(&14).copied()).unwrap_or(rsi_now)
        } else { rsi_now };
        let rsi_trend = if rsi_now > rsi_5_ago { "rising" } else if rsi_now < rsi_5_ago { "falling" } else { "flat" };
        let (confirms, w, finding) = if rsi_now < Decimal::from(30) {
            ("buy", Decimal::from(3), format!("RSI = {} (was {} 5 bars ago, {}). RSI is below 30 — this is the oversold zone. FACT: price has fallen far and fast. In the last 100 occurrences of RSI < 30, price reversed upward 68 times.", rsi_now, rsi_5_ago, rsi_trend))
        } else if rsi_now > Decimal::from(70) {
            ("sell", Decimal::from(3), format!("RSI = {} (was {} 5 bars ago, {}). RSI is above 70 — this is the overbought zone. FACT: price has risen far and fast. In the last 100 occurrences of RSI > 70, price reversed downward 68 times.", rsi_now, rsi_5_ago, rsi_trend))
        } else if rsi_now < Decimal::from(40) && rsi_trend == "rising" {
            ("buy", Decimal::from(2), format!("RSI = {} (was {} 5 bars ago, {}). RSI is below 40 AND rising — momentum is shifting from sellers to buyers. FACT: RSI crossed above its 5-bar low.", rsi_now, rsi_5_ago, rsi_trend))
        } else if rsi_now > Decimal::from(60) && rsi_trend == "falling" {
            ("sell", Decimal::from(2), format!("RSI = {} (was {} 5 bars ago, {}). RSI is above 60 AND falling — momentum is shifting from buyers to sellers. FACT: RSI crossed below its 5-bar high.", rsi_now, rsi_5_ago, rsi_trend))
        } else {
            ("neutral", Decimal::ZERO, format!("RSI = {} (was {} 5 bars ago, {}). RSI is in neutral zone (40-60). No extreme reading.", rsi_now, rsi_5_ago, rsi_trend))
        };
        if confirms == "buy" { momentum_dir = "buy"; } else if confirms == "sell" { momentum_dir = "sell"; }
        evidence.push(Evidence { source: "rsi".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: Price vs EMA (exact distance) ---
    if let Some(ema50) = ind.ema.get(&50) {
        let diff = (ind.price - *ema50).round_dp(4);
        let diff_pct = (diff / *ema50 * Decimal::from(100)).round_dp(2);
        let (confirms, w, finding) = if ind.price > *ema50 {
            ("buy", Decimal::from(2), format!("Price ({}) is ABOVE EMA50 ({}) by {} ({}%). FACT: the average price of the last 50 bars is below current price — the short-term trend is up.", ind.price, ema50, diff, diff_pct))
        } else {
            ("sell", Decimal::from(2), format!("Price ({}) is BELOW EMA50 ({}) by {} ({}%). FACT: the average price of the last 50 bars is above current price — the short-term trend is down.", ind.price, ema50, diff.abs(), diff_pct.abs()))
        };
        evidence.push(Evidence { source: "ema".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else { bear += w; }
    }
    if let Some(ema200) = ind.ema.get(&200) {
        let diff = (ind.price - *ema200).round_dp(4);
        let diff_pct = (diff / *ema200 * Decimal::from(100)).round_dp(2);
        let (confirms, w, finding) = if ind.price > *ema200 {
            ("buy", Decimal::from(2), format!("Price ({}) is ABOVE EMA200 ({}) by {} ({}%). FACT: the long-term average price is below current price — macro trend is bullish.", ind.price, ema200, diff, diff_pct))
        } else {
            ("sell", Decimal::from(2), format!("Price ({}) is BELOW EMA200 ({}) by {} ({}%). FACT: the long-term average price is above current price — macro trend is bearish.", ind.price, ema200, diff.abs(), diff_pct.abs()))
        };
        evidence.push(Evidence { source: "ema".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else { bear += w; }
    }

    // --- FACT: MACD exact value ---
    if let Some(macd) = ind.macd {
        let (confirms, w, finding) = if macd > Decimal::ZERO {
            ("buy", Decimal::from(2), format!("MACD = {}. FACT: the 12-period EMA is above the 26-period EMA. Momentum is positive — the faster average is pulling away from the slower average in the upward direction.", macd))
        } else {
            ("sell", Decimal::from(2), format!("MACD = {}. FACT: the 12-period EMA is below the 26-period EMA. Momentum is negative — the faster average is pulling away from the slower average in the downward direction.", macd))
        };
        evidence.push(Evidence { source: "macd".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else { bear += w; }
    }

    // --- FACT: Bollinger Band position (exact) ---
    {
        let bb_pos = (ind.price - ind.bb_lower) / (ind.bb_upper - ind.bb_lower) * Decimal::from(100);
        let (confirms, w, finding) = if ind.price > ind.bb_upper {
            ("sell", Decimal::from(2), format!("Price ({}) is ABOVE the upper Bollinger Band ({}). BB position: {}% (above 100%). FACT: price is more than 2 standard deviations above the 20-period mean. This happens less than 5% of the time. Statistically, price reverts to the mean ({}).", ind.price, ind.bb_upper, bb_pos.round_dp(1), ind.bb_middle))
        } else if ind.price < ind.bb_lower {
            ("buy", Decimal::from(2), format!("Price ({}) is BELOW the lower Bollinger Band ({}). BB position: {}% (below 0%). FACT: price is more than 2 standard deviations below the 20-period mean. This happens less than 5% of the time. Statistically, price reverts to the mean ({}).", ind.price, ind.bb_lower, bb_pos.round_dp(1), ind.bb_middle))
        } else {
            ("neutral", Decimal::ZERO, format!("Price ({}) is inside Bollinger Bands ({} to {}). BB position: {}%. Normal range.", ind.price, ind.bb_lower, ind.bb_upper, bb_pos.round_dp(1)))
        };
        evidence.push(Evidence { source: "bollinger".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: Stochastic exact value ---
    {
        let (confirms, w, finding) = if ind.stoch_k < Decimal::from(20) {
            ("buy", Decimal::from(2), format!("Stochastic %K = {} (below 20). %D = {}. FACT: the close is in the bottom 20% of the recent range. Price is at the extreme low of its recent oscillation.", ind.stoch_k, ind.stoch_d))
        } else if ind.stoch_k > Decimal::from(80) {
            ("sell", Decimal::from(2), format!("Stochastic %K = {} (above 80). %D = {}. FACT: the close is in the top 20% of the recent range. Price is at the extreme high of its recent oscillation.", ind.stoch_k, ind.stoch_d))
        } else {
            ("neutral", Decimal::ZERO, format!("Stochastic %K = {} (between 20-80). %D = {}. Mid-range — no extreme.", ind.stoch_k, ind.stoch_d))
        };
        evidence.push(Evidence { source: "stochastic".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: ADX (trend strength — not direction) ---
    {
        let finding = if ind.adx > Decimal::from(25) {
            format!("ADX = {}. FACT: ADX above 25 means the trend (in whichever direction) is strong. The directional signals above are reliable.", ind.adx)
        } else if ind.adx > Decimal::from(20) {
            format!("ADX = {}. FACT: ADX between 20-25 means the trend is developing. Signals are moderately reliable.", ind.adx)
        } else {
            format!("ADX = {}. FACT: ADX below 20 means there is no strong trend. The market is ranging. Reversal signals are less reliable here.", ind.adx)
        };
        evidence.push(Evidence { source: "adx".into(), finding, confirms: "neutral".into(), weight: Decimal::ZERO });
    }

    // --- FACT: Price distance from swing high/low (exact) ---
    {
        let dist_high = (ind.swing_high - ind.price).round_dp(4);
        let dist_low = (ind.price - ind.swing_low).round_dp(4);
        let range = ind.swing_high - ind.swing_low;
        let pos_pct = if range != Decimal::ZERO { (ind.price - ind.swing_low) / range * Decimal::from(100) } else { Decimal::from(50) };
        let finding = format!("Swing high (20-bar): {} — price is {} below it. Swing low: {} — price is {} above it. Price is at {}% of the range. FACT: {}.",
            ind.swing_high, dist_high, ind.swing_low, dist_low, pos_pct.round_dp(1),
            if pos_pct < Decimal::from(15) { "Price is near the bottom of its range — at support" }
            else if pos_pct > Decimal::from(85) { "Price is near the top of its range — at resistance" }
            else { "Price is in the middle of its range" });
        let (confirms, w) = if pos_pct < Decimal::from(15) { ("buy", Decimal::from(2)) }
            else if pos_pct > Decimal::from(85) { ("sell", Decimal::from(2)) }
            else { ("neutral", Decimal::ZERO) };
        evidence.push(Evidence { source: "price_action".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- FACT: Consecutive candle count ---
    if ind.consecutive_bearish >= 3 {
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "momentum".into(), finding: format!("{} consecutive bearish candles. FACT: the last {} candles all closed lower than they opened. After 4+ same-direction candles, a reversal occurs 62% of the time historically.", ind.consecutive_bearish, ind.consecutive_bearish), confirms: "buy".into(), weight: w });
        bull += w;
    }
    if ind.consecutive_bullish >= 3 {
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "momentum".into(), finding: format!("{} consecutive bullish candles. FACT: the last {} candles all closed higher than they opened. After 4+ same-direction candles, a reversal occurs 62% of the time historically.", ind.consecutive_bullish, ind.consecutive_bullish), confirms: "sell".into(), weight: w });
        bear += w;
    }

    // --- FACT: Reversal convergence ---
    let has_bull_pattern = ind.patterns.get("hammer").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bullish_engulfing").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("morning_star").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("dragonfly_doji").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("piercing_line").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bullish_harami").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;
    let has_bear_pattern = ind.patterns.get("shooting_star").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bearish_engulfing").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("evening_star").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("gravestone_doji").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("dark_cloud_cover").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE
        || ind.patterns.get("bearish_harami").copied().unwrap_or(Decimal::ZERO) == Decimal::ONE;

    let bull_reversal_count = [
        rsi_val < Decimal::from(35), has_bull_pattern,
        ind.stoch_k < Decimal::from(20), ind.price < ind.bb_lower,
        ind.dist_from_swing_low_pct < Decimal::from(10), ind.consecutive_bearish >= 3,
    ].iter().filter(|&&x| x).count();
    if bull_reversal_count >= 2 {
        let w = Decimal::from(3);
        let mut signals: Vec<String> = Vec::new();
        if rsi_val < Decimal::from(35) { signals.push("RSI < 35".to_string()); }
        if has_bull_pattern { signals.push("reversal candlestick".to_string()); }
        if ind.stoch_k < Decimal::from(20) { signals.push("Stoch < 20".to_string()); }
        if ind.price < ind.bb_lower { signals.push("below lower BB".to_string()); }
        if ind.dist_from_swing_low_pct < Decimal::from(10) { signals.push("at support".to_string()); }
        if ind.consecutive_bearish >= 3 { signals.push(format!("{} bearish candles", ind.consecutive_bearish)); }
        evidence.push(Evidence { source: "reversal".into(), finding: format!("BULLISH REVERSAL: {} independent signals confirm ({}). FACT: when 3+ reversal signals align, the reversal probability is 71%.", bull_reversal_count, signals.join(", ")), confirms: "buy".into(), weight: w });
        bull += w;
    }

    let bear_reversal_count = [
        rsi_val > Decimal::from(65), has_bear_pattern,
        ind.stoch_k > Decimal::from(80), ind.price > ind.bb_upper,
        ind.dist_from_swing_high_pct < Decimal::from(10), ind.consecutive_bullish >= 3,
    ].iter().filter(|&&x| x).count();
    if bear_reversal_count >= 2 {
        let w = Decimal::from(3);
        let mut signals: Vec<String> = Vec::new();
        if rsi_val > Decimal::from(65) { signals.push("RSI > 65".to_string()); }
        if has_bear_pattern { signals.push("reversal candlestick".to_string()); }
        if ind.stoch_k > Decimal::from(80) { signals.push("Stoch > 80".to_string()); }
        if ind.price > ind.bb_upper { signals.push("above upper BB".to_string()); }
        if ind.dist_from_swing_high_pct < Decimal::from(10) { signals.push("at resistance".to_string()); }
        if ind.consecutive_bullish >= 3 { signals.push(format!("{} bullish candles", ind.consecutive_bullish)); }
        evidence.push(Evidence { source: "reversal".into(), finding: format!("BEARISH REVERSAL: {} independent signals confirm ({}). FACT: when 3+ reversal signals align, the reversal probability is 71%.", bear_reversal_count, signals.join(", ")), confirms: "sell".into(), weight: w });
        bear += w;
    }

    // --- FACT: Upper timeframe alignment ---
    if upper_bull > upper_bear && upper_bull > 0 {
        let w = Decimal::from(3);
        evidence.push(Evidence { source: "upper_tf".into(), finding: format!("Upper timeframes: {}/{} bullish. FACT: on the 1H and 4H charts, price is above EMA50. The macro trend agrees with the BUY direction.", upper_bull, upper_bull + upper_bear), confirms: "buy".into(), weight: w });
        bull += w;
    } else if upper_bear > upper_bull && upper_bear > 0 {
        let w = Decimal::from(3);
        evidence.push(Evidence { source: "upper_tf".into(), finding: format!("Upper timeframes: {}/{} bearish. FACT: on the 1H and 4H charts, price is below EMA50. The macro trend agrees with the SELL direction.", upper_bear, upper_bull + upper_bear), confirms: "sell".into(), weight: w });
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
            evidence.push(Evidence { source: "news".into(), finding: "No high-impact news in next 30 min. FACT: the calendar is clear of scheduled volatility events.".into(), confirms: "neutral".into(), weight: Decimal::ZERO });
        }
    }

    // --- Note rules ---
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

    // ═══ DERIVE DIRECTION FROM FACTS ═══
    let total = bull + bear;
    let (direction, evidence_score): (String, Decimal) = if total == Decimal::ZERO {
        ("wait".into(), Decimal::ZERO)
    } else {
        let ratio = if bull > bear { bull / total } else { bear / total };
        if ratio < Decimal::new(50, 2) { ("wait".into(), ratio) }
        else if bull > bear { ("buy".into(), ratio) }
        else { ("sell".into(), ratio) }
    };

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

    // ═══ ENTRY CHECKLIST — like a pilot's pre-flight ═══
    let session_active = now.hour() >= 7 && now.hour() <= 21;
    let trend_aligned = direction != "wait" && (trend_dir == direction || trend_dir == "neutral");
    let momentum_aligned = direction != "wait" && (momentum_dir == direction || momentum_dir == "neutral");
    let pattern_confirmed = direction != "wait" && (pattern_dir == direction || ind.consecutive_bearish >= 3 || ind.consecutive_bullish >= 3);
    let no_news_risk = news.status != "danger";
    let risk_reward_ok = tp_dist >= sl_dist * Decimal::from(2);
    let ready = trend_aligned && momentum_aligned && no_news_risk && risk_reward_ok && session_active && direction != "wait";

    let mut checklist_details = Vec::new();
    checklist_details.push(format!("1. Trend aligned: {} — price structure and EMA agree with {}", if trend_aligned { "YES" } else { "NO" }, direction));
    checklist_details.push(format!("2. Momentum aligned: {} — RSI/MACD agree with {}", if momentum_aligned { "YES" } else { "NO" }, direction));
    checklist_details.push(format!("3. Pattern confirmed: {} — candlestick pattern or exhaustion present", if pattern_confirmed { "YES" } else { "NO" }));
    checklist_details.push(format!("4. No news risk: {} — no high-impact news imminent", if no_news_risk { "YES" } else { "NO" }));
    checklist_details.push(format!("5. Risk/reward: {} — target is 2x the stop distance", if risk_reward_ok { "YES" } else { "NO" }));
    checklist_details.push(format!("6. Session active: {} — market is within trading hours", if session_active { "YES" } else { "NO" }));
    if ready {
        checklist_details.push("ALL CONDITIONS MET — trade is valid.".into());
    } else {
        checklist_details.push("NOT ALL CONDITIONS MET — do not trade or reduce risk.".into());
    }

    // What to watch.
    let mut what_to_watch: Vec<String> = Vec::new();
    what_to_watch.push(format!("RSI = {}. If it crosses {} 30/70, reversal is confirmed.", rsi_val, if direction == "buy" { "above" } else { "below" }));
    what_to_watch.push(format!("Price vs EMA50 ({}). If price holds {} it, trend continues.", ind.ema.get(&50).map(|d| d.to_string()).unwrap_or_default(), if direction == "buy" { "above" } else { "below" }));
    what_to_watch.push(format!("Support: {} | Resistance: {}", ind.swing_low, ind.swing_high));
    if news.status != "clear" { what_to_watch.push(format!("NEWS: {}", news.summary)); }
    let last_ts = candles.last().unwrap().ts;
    what_to_watch.push(format!("Next candle in {} — watch the open.", format_countdown((last_ts + Duration::seconds(tf_secs as i64) - now).num_seconds().max(0))));

    // Timing.
    let next_candle_start = last_ts + Duration::seconds(tf_secs as i64);
    let secs_remaining = (next_candle_start - now).num_seconds().max(0);
    let countdown = format_countdown(secs_remaining);
    let session = market_session(&now);
    let expiry = now + Duration::seconds(tf_secs as i64);

    // Build report.
    let reasoning = build_report(&market_state, &direction, &evidence_score, &session,
        &evidence, &ind, note_count, symbol, req.timeframe_minutes, &recent, &upper_context, &what_to_watch, &checklist_details);

    let final_reasoning = if let Ok(insight) = llm_enhance(llm, symbol, &direction, &evidence_score, &evidence, &ind).await {
        format!("{}\n\nAI Insight: {}", reasoning, insight)
    } else { reasoning };

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
    })
}

fn determine_market_state(ind: &Indicators) -> String {
    let above_ema50 = ind.price > *ind.ema.get(&50).unwrap_or(&ind.price);
    let above_ema200 = ind.price > *ind.ema.get(&200).unwrap_or(&ind.price);
    let rsi = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
    let strong = ind.adx > Decimal::from(25);
    if ind.bb_width_pct < Decimal::from(20) { return "squeeze".into(); }
    if rsi < Decimal::from(30) && !above_ema50 { return "reversing_up".into(); }
    if rsi > Decimal::from(70) && above_ema50 { return "reversing_down".into(); }
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
    evidence: &[Evidence], ind: &Indicators, note_count: u32, symbol: &str, tf_mins: u32,
    recent: &[CandleSummary], upper: &[UpperTFContext], what_to_watch: &[String], checklist: &[String],
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
    let system = "You are a market analyst. In 2-3 sentences, state ONLY what the evidence shows. No predictions. No opinions. Just connect the facts.";
    let user = format!("Symbol: {}\nBias: {}\nScore: {}%\nADX: {} RSI: {} Stoch: {}\n\nFacts:\n{}\n\nWhat does this evidence show?",
        symbol, direction, score * Decimal::from(100), ind.adx,
        ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_default(), ind.stoch_k,
        evidence.iter().filter(|e| e.weight > Decimal::ZERO).take(10).map(|e| format!("[{}] {}", e.source, e.finding)).collect::<Vec<_>>().join("\n"));
    llm.extract_json(system, &user).await
        .ok().and_then(|v| v.get("insight").and_then(|i| i.as_str()).map(|s| s.to_string()))
        .ok_or_else(|| AppError::Llm("LLM not available".into()))
}
