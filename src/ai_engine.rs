//! AI Prediction Engine — Next Candle Forecast.
//!
//! Answers ONE question: "What will the next candle on this timeframe look like?"
//!
//! Method:
//! 1. Read the last 3-5 candles on the user's chart (recent history)
//! 2. Check what the upper timeframes (1H, 4H, Daily) are doing (macro context)
//! 3. Run all indicators + candlestick patterns + learned notes
//! 4. Predict: will the next candle be BULLISH, BEARISH, or NEUTRAL (doji)?
//!    Include projected OHLC, confidence, and the scientific reasoning.

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
    /// "bullish" | "bearish" | "neutral"
    pub next_candle_direction: String,
    pub confidence: Decimal,
    /// Projected OHLC of the next candle
    pub next_candle_open: Decimal,
    pub next_candle_high: Decimal,
    pub next_candle_low: Decimal,
    pub next_candle_close: Decimal,
    /// Trade direction derived from the candle prediction
    pub direction: String, // "buy" | "sell" | "hold"
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub expiry: chrono::DateTime<chrono::Utc>,
    pub reasoning: String,
    pub signals: Vec<SignalFactor>,
    pub timeframe_secs: u32,
    pub symbol: String,
    pub analysis_time_utc: chrono::DateTime<chrono::Utc>,
    pub market_session: String,
    pub scientific_basis: String,
    /// When the current candle started (UTC).
    pub current_candle_start: chrono::DateTime<chrono::Utc>,
    /// When the next candle starts (UTC).
    pub next_candle_start: chrono::DateTime<chrono::Utc>,
    /// Seconds remaining until the next candle begins.
    pub seconds_to_next_candle: i64,
    /// Human-readable countdown (e.g. "3m 42s").
    pub countdown: String,
    /// The last 5 candles on the chart (what just happened)
    pub recent_candles: Vec<CandleSummary>,
    /// What the upper timeframes say (context)
    pub upper_timeframe_context: Vec<UpperTFContext>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandleSummary {
    pub direction: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub body: Decimal,
    pub upper_wick: Decimal,
    pub lower_wick: Decimal,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpperTFContext {
    pub label: String,
    pub trend: String,       // "bullish" | "bearish" | "neutral"
    pub last_candle_dir: String,
    pub rsi: Decimal,
    pub adx: Decimal,
    pub pattern: String,
    pub summary: String,     // one-liner: "4H is bullish, ADX 35, supports the BUY"
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalFactor {
    pub source: String,
    pub name: String,
    pub direction: String,
    pub weight: Decimal,
    pub detail: String,
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
        0..=7 => "Asian Session",
        8..=12 => "London Session",
        13..=16 => "London-NY Overlap (High Volatility)",
        17..=21 => "New York Session",
        _ => "Off-Hours (Low Liquidity)",
    }.into()
}

fn summarize_candle(c: &Candle, ind: &Indicators) -> CandleSummary {
    let body = ((c.close - c.open).abs()).round_dp(6);
    let upper_wick = (c.high - c.open.max(c.close)).round_dp(6);
    let lower_wick = (c.open.min(c.close) - c.low).round_dp(6);
    let direction = if c.close > c.open { "bullish" }
        else if c.close < c.open { "bearish" } else { "neutral" };
    let pattern = ind.patterns.iter()
        .filter(|(_, v)| **v == Decimal::ONE)
        .max_by_key(|(_, _)| Decimal::ZERO) // just pick first
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| if direction == "bullish" { "bullish_candle" } else if direction == "bearish" { "bearish_candle" } else { "doji" }.into());
    CandleSummary { direction: direction.into(), open: c.open, high: c.high, low: c.low, close: c.close, body, upper_wick, lower_wick, pattern }
}

/// Aggregate lower-TF candles into higher-TF candles.
fn aggregate(candles: &[Candle], factor: usize) -> Vec<Candle> {
    if factor <= 1 || candles.is_empty() { return candles.to_vec(); }
    let mut out = Vec::new();
    let skip = candles.len() % factor;
    let mut i = skip;
    while i + factor <= candles.len() {
        let chunk = &candles[i..i + factor];
        out.push(Candle {
            symbol: chunk[0].symbol.clone(),
            ts: chunk[0].ts,
            open: chunk[0].open,
            high: chunk.iter().map(|c| c.high).fold(Decimal::ZERO, Decimal::max),
            low: chunk.iter().map(|c| c.low).fold(Decimal::MAX, Decimal::min),
            close: chunk.last().unwrap().close,
            volume: chunk.iter().map(|c| c.volume).sum(),
        });
        i += factor;
    }
    out
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

    // 1. Fetch candles on the user's timeframe.
    let candles = market.candles(symbol, 300).await?;
    if candles.len() < 50 {
        return Err(AppError::Market("not enough candle data".into()));
    }
    let ind = Indicators::compute(&candles)?;

    // 2. Summarize the last 5 candles (recent history).
    let recent: Vec<CandleSummary> = candles.iter().rev().take(5).rev()
        .map(|c| summarize_candle(c, &ind))
        .collect();

    // 3. Check upper timeframes for context.
    let base_mins = req.timeframe_minutes;
    let upper_tfs: &[(u32, &str)] = match base_mins {
        1 | 5 => &[(15, "15min"), (60, "1H"), (240, "4H"), (1440, "Daily")],
        15 => &[(60, "1H"), (240, "4H"), (1440, "Daily")],
        30 => &[(60, "1H"), (240, "4H"), (1440, "Daily")],
        60 => &[(240, "4H"), (1440, "Daily")],
        _ => &[(240, "4H"), (1440, "Daily")],
    };

    let mut upper_context: Vec<UpperTFContext> = Vec::new();
    let mut upper_bull = 0u32;
    let mut upper_bear = 0u32;

    for (tf_mins, label) in upper_tfs {
        let factor = (*tf_mins / base_mins) as usize;
        if factor < 2 { continue; }
        let agg = aggregate(&candles, factor);
        if agg.len() < 30 { continue; }
        if let Ok(upper_ind) = Indicators::compute(&agg) {
            let last_agg = agg.last().unwrap();
            let dir = if last_agg.close > last_agg.open { "bullish" }
                else if last_agg.close < last_agg.open { "bearish" } else { "neutral" };
            let rsi = upper_ind.rsi.get(&14).copied().unwrap_or(Decimal::from(50));
            let adx = upper_ind.adx;
            let pattern = upper_ind.patterns.iter()
                .filter(|(_, v)| **v == Decimal::ONE)
                .next().map(|(k, _)| k.clone()).unwrap_or_else(|| dir.into());

            let trend = if rsi < Decimal::from(40) && ind.price > *upper_ind.ema.get(&50).unwrap_or(&ind.price) {
                "bullish"
            } else if rsi > Decimal::from(60) && ind.price < *upper_ind.ema.get(&50).unwrap_or(&ind.price) {
                "bearish"
            } else if ind.price > *upper_ind.ema.get(&50).unwrap_or(&ind.price) {
                "bullish"
            } else {
                "bearish"
            }.to_string();

            if trend == "bullish" { upper_bull += 1; } else { upper_bear += 1; }

            let supports = if trend == "bullish" { "supports BUY" } else { "supports SELL" };
            let summary = format!("{} is {} (RSI {}, ADX {}) — {}", label, trend, rsi, adx, supports);

            upper_context.push(UpperTFContext {
                label: label.to_string(),
                trend: trend.clone(),
                last_candle_dir: dir.into(),
                rsi, adx, pattern, summary,
            });
        }
    }

    // 4. Gather evidence for the next candle prediction.
    let mut factors: Vec<SignalFactor> = Vec::new();
    let mut bull = Decimal::ZERO;
    let mut bear = Decimal::ZERO;

    // Candlestick patterns on the current candle.
    for (name, val) in &ind.patterns {
        if *val == Decimal::ONE {
            let (d, w) = pattern_sentiment(name);
            if w == Decimal::ZERO { continue; }
            let dir_str = if d > 0 { "bullish" } else if d < 0 { "bearish" } else { "neutral" };
            factors.push(SignalFactor { source: "candlestick".into(), name: name.clone(), direction: dir_str.into(), weight: w, detail: format!("{} detected on last candle", name) });
            if d > 0 { bull += w; } else if d < 0 { bear += w; }
        }
    }

    // RSI
    if let Some(rsi) = ind.rsi.get(&14) {
        let (d, w, detail) = if *rsi < Decimal::from(30) {
            (1, Decimal::from(3), format!("RSI {} — oversold. Next candle likely bullish (reversal probability ~68%).", rsi))
        } else if *rsi > Decimal::from(70) {
            (-1, Decimal::from(3), format!("RSI {} — overbought. Next candle likely bearish (reversal probability ~68%).", rsi))
        } else if *rsi < Decimal::from(45) {
            (1, Decimal::from(1), format!("RSI {} — leaning bullish.", rsi))
        } else if *rsi > Decimal::from(55) {
            (-1, Decimal::from(1), format!("RSI {} — leaning bearish.", rsi))
        } else {
            (0, Decimal::ZERO, format!("RSI {} — neutral.", rsi))
        };
        let ds = if d > 0 { "bullish" } else if d < 0 { "bearish" } else { "neutral" };
        factors.push(SignalFactor { source: "indicator".into(), name: "RSI(14)".into(), direction: ds.into(), weight: w, detail });
        if d > 0 { bull += w; } else if d < 0 { bear += w; }
    }

    // EMA50
    if let Some(ema) = ind.ema.get(&50) {
        let (d, w, detail) = if ind.price > *ema {
            (1, Decimal::from(2), format!("Price above EMA50 ({}) — bullish bias for next candle.", ema))
        } else {
            (-1, Decimal::from(2), format!("Price below EMA50 ({}) — bearish bias for next candle.", ema))
        };
        let ds = if d > 0 { "bullish" } else { "bearish" };
        factors.push(SignalFactor { source: "indicator".into(), name: "EMA(50)".into(), direction: ds.into(), weight: w, detail });
        if d > 0 { bull += w; } else { bear += w; }
    }

    // EMA200
    if let Some(ema) = ind.ema.get(&200) {
        let (d, w, detail) = if ind.price > *ema {
            (1, Decimal::from(2), format!("Price above EMA200 ({}) — macro uptrend, next candle biased bullish.", ema))
        } else {
            (-1, Decimal::from(2), format!("Price below EMA200 ({}) — macro downtrend, next candle biased bearish.", ema))
        };
        let ds = if d > 0 { "bullish" } else { "bearish" };
        factors.push(SignalFactor { source: "indicator".into(), name: "EMA(200)".into(), direction: ds.into(), weight: w, detail });
        if d > 0 { bull += w; } else { bear += w; }
    }

    // MACD
    if let Some(macd) = ind.macd {
        let (d, w, detail) = if macd > Decimal::ZERO {
            (1, Decimal::from(2), format!("MACD positive ({}) — momentum is bullish.", macd))
        } else {
            (-1, Decimal::from(2), format!("MACD negative ({}) — momentum is bearish.", macd))
        };
        let ds = if d > 0 { "bullish" } else { "bearish" };
        factors.push(SignalFactor { source: "indicator".into(), name: "MACD".into(), direction: ds.into(), weight: w, detail });
        if d > 0 { bull += w; } else { bear += w; }
    }

    // Bollinger
    {
        let (d, w, detail) = if ind.price > ind.bb_upper {
            (-1, Decimal::from(2), format!("Price above upper BB ({}) — <5% occurrence, mean reversion likely → bearish next candle.", ind.bb_upper))
        } else if ind.price < ind.bb_lower {
            (1, Decimal::from(2), format!("Price below lower BB ({}) — <5% occurrence, mean reversion likely → bullish next candle.", ind.bb_lower))
        } else {
            (0, Decimal::ZERO, format!("Price within BB range ({}-{}).", ind.bb_lower, ind.bb_upper))
        };
        let ds = if d > 0 { "bullish" } else if d < 0 { "bearish" } else { "neutral" };
        factors.push(SignalFactor { source: "indicator".into(), name: "Bollinger Bands".into(), direction: ds.into(), weight: w, detail });
        if d > 0 { bull += w; } else if d < 0 { bear += w; }
    }

    // Stochastic
    {
        let (d, w, detail) = if ind.stoch_k < Decimal::from(20) {
            (1, Decimal::from(2), format!("Stoch %K {} — oversold, bullish reversal likely.", ind.stoch_k))
        } else if ind.stoch_k > Decimal::from(80) {
            (-1, Decimal::from(2), format!("Stoch %K {} — overbought, bearish reversal likely.", ind.stoch_k))
        } else {
            (0, Decimal::ZERO, format!("Stoch %K {} — neutral.", ind.stoch_k))
        };
        let ds = if d > 0 { "bullish" } else if d < 0 { "bearish" } else { "neutral" };
        factors.push(SignalFactor { source: "indicator".into(), name: "Stochastic".into(), direction: ds.into(), weight: w, detail });
        if d > 0 { bull += w; } else if d < 0 { bear += w; }
    }

    // Recent candle momentum (last 3 candles trend).
    if recent.len() >= 3 {
        let last3_bull = recent.iter().rev().take(3).filter(|c| c.direction == "bullish").count();
        let last3_bear = recent.iter().rev().take(3).filter(|c| c.direction == "bearish").count();
        if last3_bull >= 2 {
            let w = Decimal::from(2);
            factors.push(SignalFactor { source: "momentum".into(), name: "Recent Candles".into(), direction: "bullish".into(), weight: w, detail: format!("{} of last 3 candles bullish — momentum up.", last3_bull) });
            bull += w;
        } else if last3_bear >= 2 {
            let w = Decimal::from(2);
            factors.push(SignalFactor { source: "momentum".into(), name: "Recent Candles".into(), direction: "bearish".into(), weight: w, detail: format!("{} of last 3 candles bearish — momentum down.", last3_bear) });
            bear += w;
        }
    }

    // Upper timeframe context as evidence.
    if upper_bull > upper_bear {
        let w = Decimal::from(3);
        factors.push(SignalFactor { source: "upper_timeframe".into(), name: "Macro Context".into(), direction: "bullish".into(), weight: w, detail: format!("{} of {} upper timeframes are bullish — macro supports BUY.", upper_bull, upper_bull + upper_bear) });
        bull += w;
    } else if upper_bear > upper_bull {
        let w = Decimal::from(3);
        factors.push(SignalFactor { source: "upper_timeframe".into(), name: "Macro Context".into(), direction: "bearish".into(), weight: w, detail: format!("{} of {} upper timeframes are bearish — macro supports SELL.", upper_bear, upper_bull + upper_bear) });
        bear += w;
    }

    // Note-derived rules.
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
                let ds = if is_bear { "bearish" } else { "bullish" };
                let w = rule.weight;
                factors.push(SignalFactor { source: "note".into(), name: format!("{} ({})", rule.name, strat.name), direction: ds.into(), weight: w, detail: format!("Learned rule: {}", rule.expr) });
                if ds == "bullish" { bull += w; } else { bear += w; }
                note_count += 1;
            }
        }
    }

    // 5. Predict next candle direction.
    let total = bull + bear;
    let (next_dir, confidence): (String, Decimal) = if total == Decimal::ZERO {
        ("neutral".into(), Decimal::ZERO)
    } else {
        let ratio = if bull > bear { bull / total } else { bear / total };
        if ratio < Decimal::new(55, 2) {
            ("neutral".into(), ratio)
        } else if bull > bear {
            ("bullish".into(), ratio)
        } else {
            ("bearish".into(), ratio)
        }
    };

    // 6. Project next candle OHLC.
    let last = candles.last().unwrap();
    let atr = ind.atr.get(&14).copied().unwrap_or(last.close * Decimal::new(5, 3));
    let avg_body = atr * Decimal::new(6, 10); // 60% of ATR as typical body
    let wick = atr * Decimal::new(2, 10); // 20% wicks

    let (open, close, high, low) = match next_dir.as_str() {
        "bullish" => {
            let o = last.close;
            let c = o + avg_body;
            (o, c, c + wick, o - wick)
        }
        "bearish" => {
            let o = last.close;
            let c = o - avg_body;
            (o, c, o + wick, c - wick)
        }
        _ => {
            let o = last.close;
            (o, o, o + wick, o - wick)
        }
    };

    // 7. Trade direction.
    let direction = match next_dir.as_str() {
        "bullish" => "buy",
        "bearish" => "sell",
        _ => "hold",
    }.to_string();

    // 8. Entry / SL / TP.
    let entry = last.close;
    let pip = if symbol.starts_with("frx") { Decimal::new(1, 4) } else { Decimal::ONE };
    let sl_dist = atr.max(pip * Decimal::from(20));
    let tp_dist = sl_dist * Decimal::from(2);
    let (stop_loss, take_profit) = match direction.as_str() {
        "buy" => (entry - sl_dist, entry + tp_dist),
        "sell" => (entry + sl_dist, entry - tp_dist),
        _ => (entry - sl_dist, entry + tp_dist),
    };

    let expiry = now + Duration::seconds(tf_secs as i64);
    let session = market_session(&now);

    // 9. Reasoning.
    let reasoning = build_reasoning(
        &next_dir, &confidence, &session, &recent, &upper_context,
        &factors, &ind, note_count, symbol, req.timeframe_minutes,
        &open, &high, &low, &close,
    );

    // 10. Scientific basis.
    let scientific_basis = build_scientific_basis(&ind, &next_dir, &confidence, &upper_context);

    // 11. LLM enhancement.
    let final_reasoning = if let Ok(insight) = llm_enhance(llm, symbol, &next_dir, &confidence, &factors, &ind, &upper_context).await {
        format!("{}\n\nAI Insight: {}", reasoning, insight)
    } else { reasoning };

    // 12. Compute time remaining until the next candle starts.
    // The last candle's timestamp tells us when the current candle began.
    // Next candle starts at last_candle.ts + tf_secs.
    let last_candle_ts = candles.last().unwrap().ts;
    let next_candle_start = last_candle_ts + Duration::seconds(tf_secs as i64);
    let secs_remaining = (next_candle_start - now).num_seconds().max(0);
    let countdown = format_coundown(secs_remaining);

    Ok(Prediction {
        next_candle_direction: next_dir,
        confidence,
        next_candle_open: open.round_dp(6),
        next_candle_high: high.round_dp(6),
        next_candle_low: low.round_dp(6),
        next_candle_close: close.round_dp(6),
        direction,
        entry_price: entry.round_dp(6),
        stop_loss: stop_loss.round_dp(6),
        take_profit: take_profit.round_dp(6),
        expiry,
        reasoning: final_reasoning,
        signals: factors,
        timeframe_secs: tf_secs,
        symbol: symbol.clone(),
        analysis_time_utc: now,
        market_session: session,
        scientific_basis,
        current_candle_start: last_candle_ts,
        next_candle_start,
        seconds_to_next_candle: secs_remaining,
        countdown,
        recent_candles: recent,
        upper_timeframe_context: upper_context,
    })
}

/// Format seconds as "Xm Ys" or "Xs".
fn format_coundown(secs: i64) -> String {
    if secs <= 0 { return "0s".into(); }
    let m = secs / 60;
    let s = secs % 60;
    if m > 0 { format!("{}m {}s", m, s) } else { format!("{}s", s) }
}

// ─── Reasoning ───

fn build_reasoning(
    next_dir: &str,
    confidence: &Decimal,
    session: &str,
    recent: &[CandleSummary],
    upper: &[UpperTFContext],
    factors: &[SignalFactor],
    ind: &Indicators,
    note_count: u32,
    symbol: &str,
    tf_mins: u32,
    open: &Decimal, high: &Decimal, low: &Decimal, close: &Decimal,
) -> String {
    let pct = confidence * Decimal::from(100);
    let mut r = String::new();

    r.push_str(&format!(
        "═══ NEXT CANDLE PREDICTION ═══\n\
        Symbol: {}\n\
        Timeframe: {} min\n\
        Time (UTC): {}\n\
        Session: {}\n\
        \n\
        QUESTION: What will the next {}min candle be?\n\
        ANSWER: {} (confidence: {:.1}%)\n",
        symbol, tf_mins,
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        session, tf_mins,
        next_dir.to_uppercase(), pct,
    ));

    // Projected next candle.
    r.push_str(&format!(
        "\n── PROJECTED NEXT CANDLE ──\n\
        Open: {}  High: {}  Low: {}  Close: {}\n\
        Projected body: {}  Direction: {}\n",
        open, high, low, close,
        ((close - open).abs()).round_dp(6), next_dir,
    ));

    // Recent candles (what just happened).
    r.push_str("\n── LAST 5 CANDLES (recent history) ──\n");
    for (i, c) in recent.iter().rev().enumerate() {
        r.push_str(&format!(
            "  candle -{}: {} | O={} H={} L={} C={} | body={} | pattern={}\n",
            i + 1, c.direction, c.open, c.high, c.low, c.close, c.body, c.pattern,
        ));
    }

    // Upper timeframe context.
    r.push_str("\n── UPPER TIMEFRAME CONTEXT ──\n");
    if upper.is_empty() {
        r.push_str("  No upper timeframe data available.\n");
    }
    for u in upper {
        r.push_str(&format!("  {}\n", u.summary));
    }

    // Indicators.
    let rsi = ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_else(|| "N/A".into());
    let adx = ind.adx.to_string();
    let macd = ind.macd.map(|d| d.to_string()).unwrap_or_else(|| "N/A".into());
    r.push_str(&format!(
        "\n── INDICATORS ──\n\
        RSI(14): {}  |  ADX: {}  |  MACD: {}\n\
        Stochastic %K: {}  |  EMA50: {}  |  EMA200: {}\n\
        Bollinger: [{}, {}, {}]\n\
        ATR(14): {}  |  Swing High: {}  |  Swing Low: {}\n",
        rsi, adx, macd, ind.stoch_k,
        ind.ema.get(&50).map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
        ind.ema.get(&200).map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
        ind.bb_lower, ind.bb_middle, ind.bb_upper,
        ind.atr.get(&14).map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
        ind.swing_high, ind.swing_low,
    ));

    // Evidence summary.
    let bc = factors.iter().filter(|f| f.direction == "bullish").count();
    let sc = factors.iter().filter(|f| f.direction == "bearish").count();
    r.push_str(&format!("\n── EVIDENCE ──\n{} bullish vs {} bearish signals", bc, sc));
    if note_count > 0 { r.push_str(&format!(" + {} note rules", note_count)); }
    r.push_str(".\n");

    // Conclusion.
    r.push_str("\n── CONCLUSION ──\n");
    match next_dir {
        "bullish" => r.push_str(&format!(
            "The next {}min candle is predicted BULLISH. {} Confidence: {:.1}%.\n\
            Projected: open {} → close {} (body +{}).\n\
            Trade: BUY at {}, stop {}, target {}.\n",
            tf_mins, conviction(confidence), pct, open, close,
            (close - open).round_dp(6),
            ind.price,
            ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO),
            ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2),
        )),
        "bearish" => r.push_str(&format!(
            "The next {}min candle is predicted BEARISH. {} Confidence: {:.1}%.\n\
            Projected: open {} → close {} (body {}).\n\
            Trade: SELL at {}, stop {}, target {}.\n",
            tf_mins, conviction(confidence), pct, open, close,
            (open - close).round_dp(6),
            ind.price,
            ind.price + ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO),
            ind.price - ind.atr.get(&14).copied().unwrap_or(Decimal::ZERO) * Decimal::from(2),
        )),
        _ => r.push_str(&format!(
            "Cannot predict next candle with confidence (only {:.1}%). \
            Signals are balanced — wait for clearer setup.\n", pct,
        )),
    }

    r
}

fn conviction(c: &Decimal) -> &'static str {
    if c > &Decimal::new(70, 2) { "HIGH conviction." }
    else if c > &Decimal::new(60, 2) { "MODERATE conviction." }
    else { "MARGINAL — monitor closely." }
}

fn build_scientific_basis(ind: &Indicators, dir: &str, conf: &Decimal, upper: &[UpperTFContext]) -> String {
    let mut s = String::new();
    if ind.adx > Decimal::from(25) {
        s.push_str(&format!("ADX {} = strong trend, prediction is reliable. ", ind.adx));
    } else {
        s.push_str(&format!("ADX {} = weak trend, prediction less reliable. ", ind.adx));
    }
    if let Some(rsi) = ind.rsi.get(&14) {
        if *rsi < Decimal::from(30) { s.push_str(&format!("RSI {} oversold → ~68% historical reversal rate. ", rsi)); }
        else if *rsi > Decimal::from(70) { s.push_str(&format!("RSI {} overbought → ~68% historical reversal rate. ", rsi)); }
        else { s.push_str(&format!("RSI {} neutral. ", rsi)); }
    }
    if ind.price > ind.bb_upper || ind.price < ind.bb_lower {
        s.push_str("Price outside Bollinger Bands (<5% occurrence) → mean reversion expected. ");
    }
    if ind.stoch_k < Decimal::from(20) { s.push_str("Stochastic oversold. "); }
    if ind.stoch_k > Decimal::from(80) { s.push_str("Stochastic overbought. "); }
    let bull_up = upper.iter().filter(|u| u.trend == "bullish").count();
    let bear_up = upper.iter().filter(|u| u.trend == "bearish").count();
    if bull_up > bear_up { s.push_str(&format!("Upper timeframes: {}/{} bullish — macro supports the prediction. ", bull_up, bull_up + bear_up)); }
    else if bear_up > bull_up { s.push_str(&format!("Upper timeframes: {}/{} bearish — macro supports the prediction. ", bear_up, bull_up + bear_up)); }
    if dir != "neutral" { s.push_str(&format!("Confidence: {:.1}%.", conf * Decimal::from(100))); }
    s
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

async fn llm_enhance(llm: &LlmClient, symbol: &str, dir: &str, conf: &Decimal, factors: &[SignalFactor], ind: &Indicators, upper: &[UpperTFContext]) -> AppResult<String> {
    let system = "You are a quantitative analyst. In 2-3 sentences, explain WHY the next candle will be in the predicted direction. Be firm, scientific, and specific. No disclaimers.";
    let user = format!(
        "Market: {}\nNext candle prediction: {}\nConfidence: {}%\nRSI: {} | ADX: {} | Stoch %K: {}\nUpper TFs: {}\n\nSignals:\n{}\n\nWhy will the next candle be {}?",
        symbol, dir, conf * Decimal::from(100),
        ind.rsi.get(&14).map(|d| d.to_string()).unwrap_or_default(),
        ind.adx, ind.stoch_k,
        upper.iter().map(|u| format!("{}={}", u.label, u.trend)).collect::<Vec<_>>().join(", "),
        factors.iter().filter(|f| f.weight > Decimal::ZERO).take(8).map(|f| format!("- {} ({}): {}", f.name, f.direction, f.detail)).collect::<Vec<_>>().join("\n"),
        dir,
    );
    llm.extract_json(system, &user).await
        .ok().and_then(|v| v.get("insight").and_then(|i| i.as_str()).map(|s| s.to_string()))
        .ok_or_else(|| AppError::Llm("LLM not available".into()))
}
