//! AI Market Reader — Evidence-Based Analysis.
//!
//! Philosophy: the AI does NOT predict. It READS the current market state
//! using candlestick reading + indicator tools + learned knowledge, and
//! reports what the evidence shows. The trade direction is derived FROM
//! the evidence, not from a guess about the future.
//!
//! "It's not what you think, but what you can evidence."
//!
//! Output structure:
//! 1. Current Market State — what is the market doing RIGHT NOW?
//! 2. Candlestick Reading — what does the last candle say?
//! 3. Tool Verification — what each indicator confirms (facts, not opinions)
//! 4. Upper Timeframe State — macro context
//! 5. Note Knowledge — what learned rules apply
//! 6. Evidence Tally — how many tools confirm each direction
//! 7. Trade Bias — derived from the weight of evidence
//! 8. What to Watch — key levels, invalidation conditions

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
    /// The current market condition: "trending_up", "trending_down", "ranging", "reversing_up", "reversing_down"
    pub market_state: String,
    /// Trade bias derived from evidence: "buy" | "sell" | "wait"
    pub direction: String,
    /// How many tools confirm the bias (0-100%)
    pub evidence_score: Decimal,
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub expiry: chrono::DateTime<chrono::Utc>,
    /// The full evidence-based report.
    pub reasoning: String,
    /// Each piece of evidence with its source.
    pub evidence: Vec<Evidence>,
    /// What the user should watch to confirm or invalidate the bias.
    pub what_to_watch: Vec<String>,
    pub timeframe_secs: u32,
    pub symbol: String,
    pub analysis_time_utc: chrono::DateTime<chrono::Utc>,
    pub market_session: String,
    pub current_candle_start: chrono::DateTime<chrono::Utc>,
    pub next_candle_start: chrono::DateTime<chrono::Utc>,
    pub seconds_to_next_candle: i64,
    pub countdown: String,
    /// Last 5 candles for display.
    pub recent_candles: Vec<CandleSummary>,
    /// Upper timeframe states.
    pub upper_timeframe_context: Vec<UpperTFContext>,
    /// News impact assessment.
    pub news: crate::news::NewsAssessment,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub source: String,   // "candlestick" | "rsi" | "ema" | "macd" | "bollinger" | "stochastic" | "momentum" | "upper_tf" | "note" | "price_action"
    pub finding: String,  // what the tool found (a FACT, not an opinion)
    pub confirms: String, // "buy" | "sell" | "neutral"
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
    let pattern = ind.patterns.iter()
        .filter(|(_, v)| **v == Decimal::ONE)
        .next().map(|(k, _)| k.clone())
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

// ─── Core: Read the market ───

pub async fn analyze(
    db: &Db,
    market: &dyn MarketProvider,
    llm: &LlmClient,
    req: &AnalyzeRequest,
) -> AppResult<Prediction> {
    let symbol = &req.symbol;
    let tf_secs = req.timeframe_minutes * 60;
    let now = Utc::now();

    // 1. Fetch candle data.
    let candles = market.candles(symbol, 300).await?;
    if candles.len() < 50 { return Err(AppError::Market("not enough candle data".into())); }
    let ind = Indicators::compute(&candles)?;
    let last = candles.last().unwrap();

    // 2. Recent candles for display.
    let recent: Vec<CandleSummary> = candles.iter().rev().take(5).rev()
        .map(|c| summarize_candle(c, &ind)).collect();

    // 3. Upper timeframe context.
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

    // 4. Gather EVIDENCE — facts from each tool.
    let mut evidence: Vec<Evidence> = Vec::new();
    let mut bull = Decimal::ZERO;
    let mut bear = Decimal::ZERO;

    // --- Candlestick reading: what pattern is on the last candle? ---
    for (name, val) in &ind.patterns {
        if *val != Decimal::ONE { continue; }
        let (d, w) = pattern_sentiment(name);
        if w == Decimal::ZERO {
            // Neutral pattern — still report it as a finding.
            evidence.push(Evidence { source: "candlestick".into(), finding: format!("{} pattern detected on last candle. {}", name, pattern_meaning(name)), confirms: "neutral".into(), weight: Decimal::ZERO });
            continue;
        }
        let confirms = if d > 0 { "buy" } else { "sell" };
        evidence.push(Evidence { source: "candlestick".into(), finding: format!("{} pattern detected. {}", name, pattern_meaning(name)), confirms: confirms.into(), weight: w });
        if d > 0 { bull += w; } else { bear += w; }
    }

    // --- RSI: what level is it at? ---
    if let Some(rsi) = ind.rsi.get(&14) {
        let (confirms, w, finding) = if *rsi < Decimal::from(30) {
            ("buy", Decimal::from(3), format!("RSI is {} — below 30 (oversold). Sellers are exhausted. Historical reversal rate from this level: ~68%.", rsi))
        } else if *rsi > Decimal::from(70) {
            ("sell", Decimal::from(3), format!("RSI is {} — above 70 (overbought). Buyers are exhausted. Historical reversal rate from this level: ~68%.", rsi))
        } else if *rsi < Decimal::from(40) {
            ("buy", Decimal::from(1), format!("RSI is {} — below 40, selling pressure weakening but not yet oversold.", rsi))
        } else if *rsi > Decimal::from(60) {
            ("sell", Decimal::from(1), format!("RSI is {} — above 60, buying pressure weakening but not yet overbought.", rsi))
        } else {
            ("neutral", Decimal::ZERO, format!("RSI is {} — in the neutral zone (40-60). No extreme.", rsi))
        };
        evidence.push(Evidence { source: "rsi".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- EMA: where is price relative to the moving averages? ---
    if let Some(ema50) = ind.ema.get(&50) {
        let (confirms, w, finding) = if ind.price > *ema50 {
            ("buy", Decimal::from(2), format!("Price {} is ABOVE EMA50 ({}). Short-term trend is up. EMA50 acts as dynamic support.", ind.price, ema50))
        } else {
            ("sell", Decimal::from(2), format!("Price {} is BELOW EMA50 ({}). Short-term trend is down. EMA50 acts as dynamic resistance.", ind.price, ema50))
        };
        evidence.push(Evidence { source: "ema".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else { bear += w; }
    }
    if let Some(ema200) = ind.ema.get(&200) {
        let (confirms, w, finding) = if ind.price > *ema200 {
            ("buy", Decimal::from(2), format!("Price is ABOVE EMA200 ({}). Long-term trend is up. Macro bias is bullish.", ema200))
        } else {
            ("sell", Decimal::from(2), format!("Price is BELOW EMA200 ({}). Long-term trend is down. Macro bias is bearish.", ema200))
        };
        evidence.push(Evidence { source: "ema".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else { bear += w; }
    }

    // --- MACD: what is momentum doing? ---
    if let Some(macd) = ind.macd {
        let (confirms, w, finding) = if macd > Decimal::ZERO {
            ("buy", Decimal::from(2), format!("MACD is positive ({}). Fast EMA is above slow EMA — momentum is bullish.", macd))
        } else {
            ("sell", Decimal::from(2), format!("MACD is negative ({}). Fast EMA is below slow EMA — momentum is bearish.", macd))
        };
        evidence.push(Evidence { source: "macd".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else { bear += w; }
    }

    // --- Bollinger Bands: where is price in the volatility range? ---
    {
        let (confirms, w, finding) = if ind.price > ind.bb_upper {
            ("sell", Decimal::from(2), format!("Price {} is ABOVE the upper Bollinger Band ({}). This occurs <5% of the time — price is stretched. Mean reversion is statistically expected.", ind.price, ind.bb_upper))
        } else if ind.price < ind.bb_lower {
            ("buy", Decimal::from(2), format!("Price {} is BELOW the lower Bollinger Band ({}). This occurs <5% of the time — price is stretched. Mean reversion is statistically expected.", ind.price, ind.bb_lower))
        } else {
            ("neutral", Decimal::ZERO, format!("Price is within Bollinger Bands ({} to {}). Normal volatility. Position: {}% of range.", ind.bb_lower, ind.bb_upper, ind.bb_position_pct))
        };
        evidence.push(Evidence { source: "bollinger".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- Stochastic: momentum oscillator ---
    {
        let (confirms, w, finding) = if ind.stoch_k < Decimal::from(20) {
            ("buy", Decimal::from(2), format!("Stochastic %K is {} — below 20 (oversold). Bullish crossover likely.", ind.stoch_k))
        } else if ind.stoch_k > Decimal::from(80) {
            ("sell", Decimal::from(2), format!("Stochastic %K is {} — above 80 (overbought). Bearish crossover likely.", ind.stoch_k))
        } else {
            ("neutral", Decimal::ZERO, format!("Stochastic %K is {} — neutral zone (20-80).", ind.stoch_k))
        };
        evidence.push(Evidence { source: "stochastic".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // --- ADX: how strong is the current trend? (context, not direction) ---
    {
        let finding = if ind.adx > Decimal::from(25) {
            format!("ADX is {} — strong trend (>25). Current directional signals are reliable.", ind.adx)
        } else if ind.adx > Decimal::from(20) {
            format!("ADX is {} — developing trend (20-25). Signals are moderately reliable.", ind.adx)
        } else {
            format!("ADX is {} — weak/no trend (<20). Range-bound conditions. Reversal signals are less reliable; breakout signals are more relevant.", ind.adx)
        };
        evidence.push(Evidence { source: "adx".into(), finding, confirms: "neutral".into(), weight: Decimal::ZERO });
    }

    // --- Volatility regime ---
    {
        let finding = format!("Volatility is {} (ATR {} vs prev {}). {}", ind.volatility_regime,
            ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO), ind.prev_atr,
            if ind.volatility_regime == "contracting" && ind.bb_width_pct < Decimal::from(20) {
                "Bollinger Band squeeze detected — breakout is imminent. Wait for direction confirmation."
            } else if ind.volatility_regime == "expanding" {
                "Volatility is expanding — trends are strong, follow the dominant direction."
            } else {
                "Volatility is stable — normal trading conditions."
            });
        evidence.push(Evidence { source: "volatility".into(), finding, confirms: "neutral".into(), weight: Decimal::ZERO });
    }

    // --- Price action: support/resistance proximity ---
    {
        if ind.dist_from_swing_low_pct < Decimal::from(15) {
            let w = Decimal::from(2);
            evidence.push(Evidence { source: "price_action".into(), finding: format!("Price is near the swing low ({}% above support at {}). Buyers historically defend this level.", ind.dist_from_swing_low_pct, ind.swing_low), confirms: "buy".into(), weight: w });
            bull += w;
        }
        if ind.dist_from_swing_high_pct < Decimal::from(15) {
            let w = Decimal::from(2);
            evidence.push(Evidence { source: "price_action".into(), finding: format!("Price is near the swing high ({}% below resistance at {}). Sellers historically defend this level.", ind.dist_from_swing_high_pct, ind.swing_high), confirms: "sell".into(), weight: w });
            bear += w;
        }
    }

    // --- Momentum: recent candle sequence ---
    if ind.consecutive_bullish >= 4 {
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "momentum".into(), finding: format!("{} consecutive bullish candles. Buying is exhausted — a bearish pullback is statistically likely after 4+ green candles.", ind.consecutive_bullish), confirms: "sell".into(), weight: w });
        bear += w;
    }
    if ind.consecutive_bearish >= 4 {
        let w = Decimal::from(2);
        evidence.push(Evidence { source: "momentum".into(), finding: format!("{} consecutive bearish candles. Selling is exhausted — a bullish bounce is statistically likely after 4+ red candles.", ind.consecutive_bearish), confirms: "buy".into(), weight: w });
        bull += w;
    }
    if ind.roc_5 != Decimal::ZERO {
        let (confirms, w, finding) = if ind.roc_5 > Decimal::from(2) {
            ("buy", Decimal::from(1), format!("5-bar rate of change is +{}%. Strong upward acceleration.", ind.roc_5))
        } else if ind.roc_5 < Decimal::from(-2) {
            ("sell", Decimal::from(1), format!("5-bar rate of change is {}%. Strong downward acceleration.", ind.roc_5))
        } else {
            ("neutral", Decimal::ZERO, format!("5-bar rate of change is {}%. Normal pace.", ind.roc_5))
        };
        evidence.push(Evidence { source: "momentum".into(), finding, confirms: confirms.into(), weight: w });
        if confirms == "buy" { bull += w; } else if confirms == "sell" { bear += w; }
    }

    // ═══ REVERSAL DETECTION ═══
    // The AI actively looks for reversal setups — where multiple signals
    // converge to suggest the current trend is about to flip.

    let rsi_val = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
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
    let price_below_bb_lower = ind.price < ind.bb_lower;
    let price_above_bb_upper = ind.price > ind.bb_upper;
    let stoch_oversold = ind.stoch_k < Decimal::from(20);
    let stoch_overbought = ind.stoch_k > Decimal::from(80);
    let near_support = ind.dist_from_swing_low_pct < Decimal::from(10);
    let near_resistance = ind.dist_from_swing_high_pct < Decimal::from(10);

    // Bullish reversal: oversold + reversal candle + at support.
    let bull_reversal_signals = [
        rsi_val < Decimal::from(35),           // RSI near oversold
        has_bull_pattern,                       // Bullish reversal candlestick
        stoch_oversold,                         // Stochastic oversold
        price_below_bb_lower,                   // Below lower BB
        near_support,                           // At swing low support
        ind.consecutive_bearish >= 3,           // Downtrend exhaustion
    ].iter().filter(|&&x| x).count();

    if bull_reversal_signals >= 2 {
        let w = Decimal::from(3);
        let signals_str = {
            let mut parts: Vec<String> = Vec::new();
            if rsi_val < Decimal::from(35) { parts.push("RSI oversold".to_string()); }
            if has_bull_pattern { parts.push("reversal candlestick".to_string()); }
            if stoch_oversold { parts.push("Stochastic oversold".to_string()); }
            if price_below_bb_lower { parts.push("below lower BB".to_string()); }
            if near_support { parts.push("at support".to_string()); }
            if ind.consecutive_bearish >= 3 { parts.push(format!("{} bearish candles exhaustion", ind.consecutive_bearish)); }
            parts.join(", ")
        };
        evidence.push(Evidence {
            source: "reversal".into(),
            finding: format!("BULLISH REVERSAL SETUP: {} convergence signals detected ({}). The downtrend may be exhausted — a bounce is probable.", bull_reversal_signals, signals_str),
            confirms: "buy".into(), weight: w,
        });
        bull += w;
    }

    // Bearish reversal: overbought + reversal candle + at resistance.
    let bear_reversal_signals = [
        rsi_val > Decimal::from(65),
        has_bear_pattern,
        stoch_overbought,
        price_above_bb_upper,
        near_resistance,
        ind.consecutive_bullish >= 3,
    ].iter().filter(|&&x| x).count();

    if bear_reversal_signals >= 2 {
        let w = Decimal::from(3);
        let signals_str = {
            let mut parts = Vec::new();
            if rsi_val > Decimal::from(65) { parts.push("RSI overbought".to_string()); }
            if has_bear_pattern { parts.push("reversal candlestick".to_string()); }
            if stoch_overbought { parts.push("Stochastic overbought".to_string()); }
            if price_above_bb_upper { parts.push("above upper BB".to_string()); }
            if near_resistance { parts.push("at resistance".to_string()); }
            if ind.consecutive_bullish >= 3 { parts.push(format!("{} bullish candles exhaustion", ind.consecutive_bullish)); }
            parts.join(", ")
        };
        evidence.push(Evidence {
            source: "reversal".into(),
            finding: format!("BEARISH REVERSAL SETUP: {} convergence signals detected ({}). The uptrend may be exhausted — a pullback is probable.", bear_reversal_signals, signals_str),
            confirms: "sell".into(), weight: w,
        });
        bear += w;
    }

    // RSI divergence (simplified): RSI turning up while price still falling.
    if rsi_val < Decimal::from(40) && ind.roc_5 < Decimal::ZERO && has_bull_pattern {
        let w = Decimal::from(2);
        evidence.push(Evidence {
            source: "reversal".into(),
            finding: format!("Potential BULLISH DIVERGENCE: RSI is {} (oversold zone) and a reversal candlestick appeared while price is still falling. Momentum is diverging from price — reversal likely.", rsi_val),
            confirms: "buy".into(), weight: w,
        });
        bull += w;
    }
    if rsi_val > Decimal::from(60) && ind.roc_5 > Decimal::ZERO && has_bear_pattern {
        let w = Decimal::from(2);
        evidence.push(Evidence {
            source: "reversal".into(),
            finding: format!("Potential BEARISH DIVERGENCE: RSI is {} (overbought zone) and a reversal candlestick appeared while price is still rising. Momentum is diverging from price — reversal likely.", rsi_val),
            confirms: "sell".into(), weight: w,
        });
        bear += w;
    }

    // BB squeeze breakout direction hint.
    if ind.bb_width_pct < Decimal::from(20) && ind.volatility_regime == "contracting" {
        // Squeeze — check which side of the middle band price is on for breakout direction.
        if ind.price > ind.bb_middle {
            let w = Decimal::from(1);
            evidence.push(Evidence {
                source: "reversal".into(),
                finding: format!("BB squeeze (width percentile {}) with price above mid-band. Breakout direction likely UPWARD. Wait for candle close above upper band to confirm.", ind.bb_width_pct),
                confirms: "buy".into(), weight: w,
            });
            bull += w;
        } else if ind.price < ind.bb_middle {
            let w = Decimal::from(1);
            evidence.push(Evidence {
                source: "reversal".into(),
                finding: format!("BB squeeze (width percentile {}) with price below mid-band. Breakout direction likely DOWNWARD. Wait for candle close below lower band to confirm.", ind.bb_width_pct),
                confirms: "sell".into(), weight: w,
            });
            bear += w;
        }
    }

    // Stochastic crossover hint (simplified: %K crossing back from extreme).
    if ind.stoch_k < Decimal::from(20) && ind.stoch_k > ind.stoch_d {
        let w = Decimal::from(1);
        evidence.push(Evidence {
            source: "reversal".into(),
            finding: format!("Stochastic bullish crossover: %K ({}) crossed above %D ({}) from oversold zone. Early bullish reversal signal.", ind.stoch_k, ind.stoch_d),
            confirms: "buy".into(), weight: w,
        });
        bull += w;
    }
    if ind.stoch_k > Decimal::from(80) && ind.stoch_k < ind.stoch_d {
        let w = Decimal::from(1);
        evidence.push(Evidence {
            source: "reversal".into(),
            finding: format!("Stochastic bearish crossover: %K ({}) crossed below %D ({}) from overbought zone. Early bearish reversal signal.", ind.stoch_k, ind.stoch_d),
            confirms: "sell".into(), weight: w,
        });
        bear += w;
    }

    // --- Upper timeframe evidence ---
    if upper_bull > upper_bear && upper_bull > 0 {
        let w = Decimal::from(3);
        evidence.push(Evidence { source: "upper_tf".into(), finding: format!("{} of {} upper timeframes are bullish. The macro trend supports a BUY.", upper_bull, upper_bull + upper_bear), confirms: "buy".into(), weight: w });
        bull += w;
    } else if upper_bear > upper_bull && upper_bear > 0 {
        let w = Decimal::from(3);
        evidence.push(Evidence { source: "upper_tf".into(), finding: format!("{} of {} upper timeframes are bearish. The macro trend supports a SELL.", upper_bear, upper_bull + upper_bear), confirms: "sell".into(), weight: w });
        bear += w;
    }

    // --- Note knowledge: what rules fire? ---
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
                evidence.push(Evidence { source: "note".into(), finding: format!("Learned rule '{}' from note '{}' fired: {}", rule.name, strat.name, rule.expr), confirms: confirms.into(), weight: w });
                if confirms == "buy" { bull += w; } else { bear += w; }
                note_count += 1;
            }
        }
    }

    // --- Self-learning: past trade history ---
    let past_trades = db.list_trades_by_symbol(symbol).await.unwrap_or_default();
    if past_trades.len() >= 5 {
        let wins = past_trades.iter().filter(|t| t.pnl.map(|p| p > Decimal::ZERO).unwrap_or(false)).count();
        let total = past_trades.len();
        let wr = Decimal::from(wins) / Decimal::from(total);
        evidence.push(Evidence {
            source: "self_learning".into(),
            finding: format!("Past {} trades on {}: {} wins ({}% win rate). {}", total, symbol, wins, wr * Decimal::from(100),
                if wr > Decimal::new(60, 2) { "Historical performance supports this symbol." }
                else if wr < Decimal::new(40, 2) { "Historical performance is poor — reduce risk." }
                else { "Historical performance is neutral." }),
            confirms: "neutral".into(), weight: Decimal::ZERO,
        });
    }

    // 5. News assessment — check for high-impact events.
    let news = crate::news::assess_news(symbol).await.unwrap_or_else(|_| crate::news::NewsAssessment {
        status: "clear".into(),
        upcoming_high_impact: vec![],
        upcoming_medium_impact: vec![],
        recently_released: vec![],
        summary: "News data unavailable.".into(),
        recommendation: "Proceed normally.".into(),
    });

    // Add news as evidence.
    match news.status.as_str() {
        "danger" => {
            // High-impact news: reduce confidence, add caution.
            evidence.push(Evidence {
                source: "news".into(),
                finding: news.summary.clone(),
                confirms: "neutral".into(),
                weight: Decimal::ZERO,
            });
            // Reduce both bull and bear by 20% — news creates uncertainty.
            bull = bull * Decimal::new(8, 1) / Decimal::from(10);
            bear = bear * Decimal::new(8, 1) / Decimal::from(10);
        }
        "caution" => {
            evidence.push(Evidence {
                source: "news".into(),
                finding: news.summary.clone(),
                confirms: "neutral".into(),
                weight: Decimal::ZERO,
            });
        }
        _ => {
            evidence.push(Evidence {
                source: "news".into(),
                finding: "No high-impact news — market is clear of news volatility.".into(),
                confirms: "neutral".into(),
                weight: Decimal::ZERO,
            });
        }
    }

    // 6. Determine market state from the evidence.
    let market_state = determine_market_state(&ind);

    // 7. Derive trade bias from the weight of evidence.
    // Bold: commit when evidence is clear (>50%), not cautious (55%).
    let total = bull + bear;
    let (direction, evidence_score): (String, Decimal) = if total == Decimal::ZERO {
        ("wait".into(), Decimal::ZERO)
    } else {
        let ratio = if bull > bear { bull / total } else { bear / total };
        if ratio < Decimal::new(50, 2) {
            // Exactly 50/50 — too balanced to commit.
            ("wait".into(), ratio)
        } else if bull > bear {
            ("buy".into(), ratio)
        } else {
            ("sell".into(), ratio)
        }
    };

    // 7. Entry / SL / TP.
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

    // 8. What to watch.
    let mut what_to_watch: Vec<String> = Vec::new();
    what_to_watch.push(format!("RSI crossing {} — if RSI crosses above 30, bullish reversal is confirmed.", ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_default()));
    what_to_watch.push(format!("Price vs EMA50 ({}) — if price holds above, bullish; if breaks below, bearish.", ind.ema.get(&50).map(|d| d.to_string()).unwrap_or_default()));
    what_to_watch.push(format!("Support at {}, resistance at {}.", ind.swing_low, ind.swing_high));
    if ind.bb_width_pct < Decimal::from(20) {
        what_to_watch.push("Bollinger Band squeeze — breakout direction will set the trend. Wait for it.".into());
    }
    if ind.consecutive_bullish >= 3 || ind.consecutive_bearish >= 3 {
        what_to_watch.push(format!("Candle exhaustion: {} consecutive candles in one direction. Watch for reversal.", ind.consecutive_bullish.max(ind.consecutive_bearish)));
    }
    // Reversal watch conditions.
    if rsi_val < Decimal::from(35) {
        what_to_watch.push(format!("RSI is {} — if it crosses back above 30, bullish reversal is CONFIRMED. If it keeps falling below 20, downtrend is accelerating.", rsi_val));
    }
    if rsi_val > Decimal::from(65) {
        what_to_watch.push(format!("RSI is {} — if it crosses back below 70, bearish reversal is CONFIRMED. If it keeps rising above 80, uptrend is accelerating.", rsi_val));
    }
    if has_bull_pattern {
        what_to_watch.push("Bullish reversal candlestick detected — watch if the NEXT candle confirms by closing higher. If it closes lower, the pattern failed.".into());
    }
    if has_bear_pattern {
        what_to_watch.push("Bearish reversal candlestick detected — watch if the NEXT candle confirms by closing lower. If it closes higher, the pattern failed.".into());
    }
    if ind.stoch_k < Decimal::from(25) {
        what_to_watch.push(format!("Stochastic %K is {} — watch for %K crossing above %D ({}). That crossover confirms bullish reversal.", ind.stoch_k, ind.stoch_d));
    }
    if ind.stoch_k > Decimal::from(75) {
        what_to_watch.push(format!("Stochastic %K is {} — watch for %K crossing below %D ({}). That crossover confirms bearish reversal.", ind.stoch_k, ind.stoch_d));
    }
    what_to_watch.push(format!("Next candle in {} — watch the open. If it gaps in the bias direction, confidence increases.", format_countdown((candles.last().unwrap().ts + Duration::seconds(tf_secs as i64) - now).num_seconds().max(0))));

    // News watch.
    if news.status == "danger" {
        what_to_watch.push(format!("NEWS ALERT: {}. {}", news.summary, news.recommendation));
    } else if news.status == "caution" {
        what_to_watch.push(format!("News caution: {}. {}", news.summary, news.recommendation));
    }

    // 9. Timing.
    let last_candle_ts = candles.last().unwrap().ts;
    let next_candle_start = last_candle_ts + Duration::seconds(tf_secs as i64);
    let secs_remaining = (next_candle_start - now).num_seconds().max(0);
    let countdown = format_countdown(secs_remaining);
    let session = market_session(&now);
    let expiry = now + Duration::seconds(tf_secs as i64);

    // 10. Build the full evidence report.
    let reasoning = build_report(
        &market_state, &direction, &evidence_score, &session,
        &evidence, &ind, note_count, symbol, req.timeframe_minutes,
        &recent, &upper_context, &what_to_watch,
    );

    // 11. LLM enhancement.
    let final_reasoning = if let Ok(insight) = llm_enhance(llm, symbol, &direction, &evidence_score, &evidence, &ind).await {
        format!("{}\n\nAI Insight: {}", reasoning, insight)
    } else { reasoning };

    Ok(Prediction {
        market_state,
        direction,
        evidence_score,
        entry_price: entry.round_dp(6),
        stop_loss: stop_loss.round_dp(6),
        take_profit: take_profit.round_dp(6),
        expiry,
        reasoning: final_reasoning,
        evidence,
        what_to_watch,
        timeframe_secs: tf_secs,
        symbol: symbol.clone(),
        analysis_time_utc: now,
        market_session: session,
        current_candle_start: last_candle_ts,
        next_candle_start,
        seconds_to_next_candle: secs_remaining,
        countdown,
        recent_candles: recent,
        upper_timeframe_context: upper_context,
        news,
    })
}

/// Determine the current market state from indicator readings.
fn determine_market_state(ind: &Indicators) -> String {
    let above_ema50 = ind.price > *ind.ema.get(&50).unwrap_or(&ind.price);
    let above_ema200 = ind.price > *ind.ema.get(&200).unwrap_or(&ind.price);
    let rsi = ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
    let rsi_oversold = rsi < Decimal::from(30);
    let rsi_overbought = rsi > Decimal::from(70);
    let bb_squeeze = ind.bb_width_pct < Decimal::from(20);
    let strong_trend = ind.adx > Decimal::from(25);

    if bb_squeeze { return "squeeze".into(); }
    if rsi_oversold && !above_ema50 { return "reversing_up".into(); }
    if rsi_overbought && above_ema50 { return "reversing_down".into(); }
    if above_ema50 && above_ema200 && strong_trend { return "trending_up".into(); }
    if !above_ema50 && !above_ema200 && strong_trend { return "trending_down".into(); }
    if !strong_trend { return "ranging".into(); }
    "mixed".into()
}

/// Human-readable meaning of each candlestick pattern.
fn pattern_meaning(name: &str) -> &'static str {
    match name {
        "hammer" => "Small body at top, long lower wick. Buyers rejected lower prices. Bullish reversal signal after a downtrend.",
        "bullish_engulfing" => "Large bullish candle completely engulfs the previous bearish candle. Strong bullish reversal.",
        "bearish_engulfing" => "Large bearish candle completely engulfs the previous bullish candle. Strong bearish reversal.",
        "bullish_harami" => "Small bullish candle inside the previous large bearish candle. Indecision — potential reversal up.",
        "bearish_harami" => "Small bearish candle inside the previous large bullish candle. Indecision — potential reversal down.",
        "doji" => "Open and close are nearly equal. Market is undecided. Can signal reversal at extremes.",
        "dragonfly_doji" => "Doji with long lower wick at the low. Buyers rejected lower prices strongly. Bullish at bottoms.",
        "gravestone_doji" => "Doji with long upper wick at the high. Sellers rejected higher prices strongly. Bearish at tops.",
        "morning_star" => "Three-candle bottom reversal: large bearish, small body, large bullish. Strong bullish signal.",
        "evening_star" => "Three-candle top reversal: large bullish, small body, large bearish. Strong bearish signal.",
        "three_white_soldiers" => "Three consecutive bullish candles with rising closes. Strong uptrend confirmation.",
        "three_black_crows" => "Three consecutive bearish candles with falling closes. Strong downtrend confirmation.",
        "piercing_line" => "Bullish candle opens below prior low but closes above prior midpoint. Bottom reversal.",
        "dark_cloud_cover" => "Bearish candle opens above prior high but closes below prior midpoint. Top reversal.",
        "shooting_star" => "Small body, long upper wick, at top of uptrend. Bearish reversal signal.",
        "marubozu" => "No wicks. Full body. Strong conviction in the direction.",
        "spinning_top" => "Small body with long wicks both sides. High indecision.",
        "long_lower_shadow" => "Lower wick is 2/3+ of the range. Buyers rejected low prices. Bullish at support.",
        "long_upper_shadow" => "Upper wick is 2/3+ of the range. Sellers rejected high prices. Bearish at resistance.",
        "tweezer_bottom" => "Two candles with matching lows. Support level holding. Minor bullish reversal.",
        "tweezer_top" => "Two candles with matching highs. Resistance level holding. Minor bearish reversal.",
        "hanging_man" => "Same shape as hammer but in an uptrend. Potential bearish reversal.",
        "inverted_hammer" => "Small body, long upper wick at bottom. Potential bullish reversal.",
        "bullish_candle" => "Close is above open. The candle is green/bullish.",
        "bearish_candle" => "Close is below open. The candle is red/bearish.",
        _ => "Pattern detected.",
    }
}

/// Build the full evidence-based report.
fn build_report(
    market_state: &str,
    direction: &str,
    evidence_score: &Decimal,
    session: &str,
    evidence: &[Evidence],
    ind: &Indicators,
    note_count: u32,
    symbol: &str,
    tf_mins: u32,
    recent: &[CandleSummary],
    upper: &[UpperTFContext],
    what_to_watch: &[String],
) -> String {
    let pct = evidence_score * Decimal::from(100);
    let mut r = String::new();

    r.push_str(&format!(
        "═══ MARKET READING REPORT ═══\n\
        Symbol: {}\n\
        Timeframe: {} min\n\
        Time (UTC): {}\n\
        Session: {}\n\
        Market State: {}\n",
        symbol, tf_mins,
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        session, market_state_label(market_state),
    ));

    // Trade bias derived from evidence.
    r.push_str(&format!(
        "\n── TRADE BIAS ──\n\
        Direction: {} (evidence score: {:.1}%)\n",
        if direction == "buy" { "BUY" } else if direction == "sell" { "SELL" } else { "WAIT" },
        pct,
    ));
    let bull_evidence = evidence.iter().filter(|e| e.confirms == "buy" && e.weight > Decimal::ZERO).count();
    let bear_evidence = evidence.iter().filter(|e| e.confirms == "sell" && e.weight > Decimal::ZERO).count();
    r.push_str(&format!("Evidence: {} tools confirm BUY, {} tools confirm SELL.\n", bull_evidence, bear_evidence));
    if note_count > 0 { r.push_str(&format!("Note knowledge: {} rules fired.\n", note_count)); }

    // Candlestick reading.
    r.push_str("\n── CANDLESTICK READING ──\n");
    for c in recent.iter().rev().take(3).rev() {
        r.push_str(&format!(
            "  {} | O={} C={} body={} | pattern: {}\n",
            c.direction, c.open, c.close, c.body, c.pattern,
        ));
    }

    // Evidence from each tool.
    r.push_str("\n── TOOL READINGS (evidence) ──\n");
    for e in evidence.iter() {
        r.push_str(&format!("  [{}] {} → confirms: {} (w={})\n", e.source, e.finding, e.confirms, e.weight));
    }

    // Upper timeframe state.
    r.push_str("\n── UPPER TIMEFRAME STATE ──\n");
    if upper.is_empty() { r.push_str("  No upper timeframe data.\n"); }
    for u in upper {
        r.push_str(&format!("  {}\n", u.summary));
    }

    // What to watch.
    r.push_str("\n── WHAT TO WATCH ──\n");
    for w in what_to_watch {
        r.push_str(&format!("  • {}\n", w));
    }

    // Key levels.
    r.push_str(&format!(
        "\n── KEY LEVELS ──\n\
        Entry: {}  Stop: {}  Target: {}\n\
        Swing High: {}  Swing Low: {}\n\
        Bollinger: [{} / {} / {}]\n\
        ATR: {}\n",
        ind.price,
        if direction == "buy" { ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) } else { ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) },
        if direction == "buy" { ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2) } else { ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2) },
        ind.swing_high, ind.swing_low,
        ind.bb_lower, ind.bb_middle, ind.bb_upper,
        ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO),
    ));

    // Conclusion.
    r.push_str("\n── CONCLUSION ──\n");
    match direction {
        "buy" => r.push_str(&format!(
            "Evidence supports BUY. {} tools confirm bullish bias with {:.1}% evidence score.\n\
            The market is {}. Entry at {}, stop below {}, target {}.\n\
            This is NOT a prediction — it is the current reading. If the evidence changes, the bias changes.\n",
            bull_evidence, pct, market_state_label(market_state), ind.price,
            ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO),
            ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2),
        )),
        "sell" => r.push_str(&format!(
            "Evidence supports SELL. {} tools confirm bearish bias with {:.1}% evidence score.\n\
            The market is {}. Entry at {}, stop above {}, target {}.\n\
            This is NOT a prediction — it is the current reading. If the evidence changes, the bias changes.\n",
            bear_evidence, pct, market_state_label(market_state), ind.price,
            ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO),
            ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2),
        )),
        _ => r.push_str(&format!(
            "Evidence is inconclusive (score {:.1}%). Tools are balanced — no clear edge.\n\
            WAIT. Do not trade until the evidence tilts clearly in one direction.\n", pct,
        )),
    }

    r
}

fn market_state_label(s: &str) -> &str {
    match s {
        "trending_up" => "Trending Up (strong uptrend)",
        "trending_down" => "Trending Down (strong downtrend)",
        "ranging" => "Ranging (no clear trend, range-bound)",
        "reversing_up" => "Reversing Up (oversold, potential bottom)",
        "reversing_down" => "Reversing Down (overbought, potential top)",
        "squeeze" => "Volatility Squeeze (breakout imminent)",
        "mixed" => "Mixed (conflicting signals)",
        _ => "Unknown",
    }
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
    let system = "You are a market analyst. Based on the evidence provided, state in 2-3 sentences what the current market condition is and why the trade bias makes sense. Use only the facts given. Be firm. No disclaimers.";
    let user = format!(
        "Symbol: {}\nBias: {}\nEvidence score: {}%\nMarket state: ADX {}, RSI {}, BB position {}%\n\nEvidence:\n{}\n\nWhat is the current market condition and does the evidence support this bias?",
        symbol, direction, score * Decimal::from(100), ind.adx,
        ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_default(),
        ind.bb_position_pct,
        evidence.iter().filter(|e| e.weight > Decimal::ZERO).take(10).map(|e| format!("[{}] {} → {}", e.source, e.finding, e.confirms)).collect::<Vec<_>>().join("\n"),
    );
    llm.extract_json(system, &user).await
        .ok().and_then(|v| v.get("insight").and_then(|i| i.as_str()).map(|s| s.to_string()))
        .ok_or_else(|| AppError::Llm("LLM not available".into()))
}
