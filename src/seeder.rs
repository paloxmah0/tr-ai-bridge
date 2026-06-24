//! Seeds the database with pre-built trading strategies, educational notes, and
//! a demo account on first startup. Users don't need to add anything manually —
//! strategies are ready to backtest and run immediately.

use crate::db::Db;
use crate::domain::strategy::{CreateRule, CreateStrategy};
use crate::domain::{AssetClass, StrategySource, TradingMode};
use crate::error::AppResult;
use rust_decimal::Decimal;
use uuid::Uuid;

pub async fn seed_if_empty(db: &Db) -> AppResult<()> {
    // Check if already seeded.
    let accounts = db.list_accounts().await?;
    if !accounts.is_empty() {
        return Ok(());
    }

    tracing::info!("seeding database with demo account, strategies, and notes…");

    // 1. Create a demo account in paper mode.
    let account = db
        .create_account("Demo Account", "deriv", "demo", Decimal::from(10000), "USD", TradingMode::Paper)
        .await?;

    // 2. Create pre-built strategies based on real technical analysis.
    for strat in builtin_strategies() {
        let _ = db.create_strategy(account.id, &strat, StrategySource::Manual).await;
    }

    // 3. Create educational notes.
    for note in builtin_notes() {
        let _ = db.create_note(account.id, &note).await;
    }

    tracing::info!("seeded: 1 account, {} strategies, {} notes", builtin_strategies().len(), builtin_notes().len());
    Ok(())
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap_or(Decimal::ZERO)
}

fn builtin_strategies() -> Vec<CreateStrategy> {
    vec![
        // === FOREX STRATEGIES ===

        // 1. RSI Oversold Reversal (EUR/USD)
        CreateStrategy {
            name: "RSI Oversold Reversal".into(),
            description: Some("Classic mean-reversion: buy when RSI drops below 30 (oversold) and price is above the 200 EMA (long-term uptrend). Exits via 30-pip stop, 60-pip target.".into()),
            asset_class: AssetClass::Forex,
            symbols: vec!["frxEURUSD".into()],
            stop_loss: Some(dec("30")),
            take_profit: Some(dec("60")),
            risk_per_trade: dec("0.01"),
            rules: vec![
                CreateRule { name: "rsi_oversold".into(), expr: "rsi(14) < 30".into(), weight: dec("2") },
                CreateRule { name: "uptrend_filter".into(), expr: "price > ema(200)".into(), weight: dec("1") },
            ],
        },

        // 2. Hammer Candlestick Reversal (EUR/USD)
        CreateStrategy {
            name: "Hammer Candle Reversal".into(),
            description: Some("Bullish hammer pattern at support with RSI confirmation. The hammer shows buyers stepped in after selling pressure — a classic bottom reversal signal.".into()),
            asset_class: AssetClass::Forex,
            symbols: vec!["frxEURUSD".into()],
            stop_loss: Some(dec("25")),
            take_profit: Some(dec("50")),
            risk_per_trade: dec("0.01"),
            rules: vec![
                CreateRule { name: "hammer_pattern".into(), expr: "hammer == 1".into(), weight: dec("2") },
                CreateRule { name: "rsi_oversold".into(), expr: "rsi(14) < 40".into(), weight: dec("1.5") },
                CreateRule { name: "trend".into(), expr: "price > ema(50)".into(), weight: dec("1") },
            ],
        },

        // 3. Bullish Engulfing + MACD (GBP/USD)
        CreateStrategy {
            name: "Bullish Engulfing + MACD".into(),
            description: Some("Bullish engulfing candlestick pattern confirmed by positive MACD momentum. The engulfing pattern shows strong buying pressure overpowering sellers.".into()),
            asset_class: AssetClass::Forex,
            symbols: vec!["frxGBPUSD".into()],
            stop_loss: Some(dec("30")),
            take_profit: Some(dec("60")),
            risk_per_trade: dec("0.01"),
            rules: vec![
                CreateRule { name: "engulfing".into(), expr: "bullish_engulfing == 1".into(), weight: dec("2") },
                CreateRule { name: "macd_positive".into(), expr: "macd() > 0".into(), weight: dec("1.5") },
            ],
        },

        // 4. Doji Reversal at Support (EUR/USD)
        CreateStrategy {
            name: "Doji Reversal".into(),
            description: Some("Doji candlestick (indecision) with RSI oversold. A doji after a downtrend suggests sellers are losing control and a reversal may be imminent.".into()),
            asset_class: AssetClass::Forex,
            symbols: vec!["frxEURUSD".into()],
            stop_loss: Some(dec("20")),
            take_profit: Some(dec("40")),
            risk_per_trade: dec("0.01"),
            rules: vec![
                CreateRule { name: "doji".into(), expr: "doji == 1".into(), weight: dec("2") },
                CreateRule { name: "rsi".into(), expr: "rsi(14) < 40".into(), weight: dec("1") },
            ],
        },

        // 5. EMA Crossover Trend Following (EUR/USD)
        CreateStrategy {
            name: "EMA Trend Following".into(),
            description: Some("Trend-following: buy when price is above both EMA50 and EMA200 (golden trend). Uses wide stops for trend riding.".into()),
            asset_class: AssetClass::Forex,
            symbols: vec!["frxEURUSD".into()],
            stop_loss: Some(dec("40")),
            take_profit: Some(dec("80")),
            risk_per_trade: dec("0.01"),
            rules: vec![
                CreateRule { name: "above_ema50".into(), expr: "price > ema(50)".into(), weight: dec("1.5") },
                CreateRule { name: "above_ema200".into(), expr: "price > ema(200)".into(), weight: dec("1.5") },
                CreateRule { name: "macd_positive".into(), expr: "macd() > 0".into(), weight: dec("1") },
            ],
        },

        // === DERIVATIVE INDEX STRATEGIES ===

        // 6. Hammer Reversal on Volatility 100
        CreateStrategy {
            name: "R_100 Hammer Reversal".into(),
            description: Some("Bullish hammer pattern on the Volatility 100 Index with RSI oversold confirmation. Synthetic indices trend well and hammers mark reliable reversal points.".into()),
            asset_class: AssetClass::DerivIndex,
            symbols: vec!["R_100".into()],
            stop_loss: Some(dec("50")),
            take_profit: Some(dec("100")),
            risk_per_trade: dec("0.02"),
            rules: vec![
                CreateRule { name: "hammer".into(), expr: "hammer == 1".into(), weight: dec("2") },
                CreateRule { name: "rsi".into(), expr: "rsi(14) < 35".into(), weight: dec("1.5") },
                CreateRule { name: "trend".into(), expr: "price > ema(50)".into(), weight: dec("1") },
            ],
        },

        // 7. Bullish Engulfing on Volatility 75
        CreateStrategy {
            name: "R_75 Bullish Engulfing".into(),
            description: Some("Bullish engulfing pattern on Volatility 75 Index with RSI and trend filter. V75 is less volatile than V100, giving cleaner signals.".into()),
            asset_class: AssetClass::DerivIndex,
            symbols: vec!["R_75".into()],
            stop_loss: Some(dec("30")),
            take_profit: Some(dec("60")),
            risk_per_trade: dec("0.02"),
            rules: vec![
                CreateRule { name: "engulfing".into(), expr: "bullish_engulfing == 1".into(), weight: dec("2") },
                CreateRule { name: "rsi".into(), expr: "rsi(14) < 40".into(), weight: dec("1") },
                CreateRule { name: "trend".into(), expr: "price > ema(50)".into(), weight: dec("1") },
            ],
        },

        // 8. Three White Soldiers (R_100)
        CreateStrategy {
            name: "Three White Soldiers".into(),
            description: Some("Three consecutive bullish candles with higher closes — a strong bottom reversal signal. Combined with EMA trend filter for confirmation.".into()),
            asset_class: AssetClass::DerivIndex,
            symbols: vec!["R_100".into()],
            stop_loss: Some(dec("40")),
            take_profit: Some(dec("80")),
            risk_per_trade: dec("0.02"),
            rules: vec![
                CreateRule { name: "soldiers".into(), expr: "three_white_soldiers == 1".into(), weight: dec("2.5") },
                CreateRule { name: "trend".into(), expr: "price > ema(200)".into(), weight: dec("1") },
            ],
        },

        // 9. Bearish Engulfing Short (R_100)
        CreateStrategy {
            name: "R_100 Bearish Engulfing Short".into(),
            description: Some("Bearish engulfing pattern with overbought RSI — a top reversal signal for shorting. Synthetic indices reverse sharply after overbought conditions.".into()),
            asset_class: AssetClass::DerivIndex,
            symbols: vec!["R_100".into()],
            stop_loss: Some(dec("40")),
            take_profit: Some(dec("80")),
            risk_per_trade: dec("0.02"),
            rules: vec![
                CreateRule { name: "bear_engulfing".into(), expr: "bearish_engulfing == 1".into(), weight: dec("2") },
                CreateRule { name: "rsi_overbought".into(), expr: "rsi(14) > 65".into(), weight: dec("1.5") },
            ],
        },

        // 10. Morning Star Reversal (R_75)
        CreateStrategy {
            name: "Morning Star Reversal".into(),
            description: Some("Three-candle morning star pattern — a major bullish reversal signal. The pattern shows a transition from bearishness to bullish indecision to strong buying.".into()),
            asset_class: AssetClass::DerivIndex,
            symbols: vec!["R_75".into()],
            stop_loss: Some(dec("35")),
            take_profit: Some(dec("70")),
            risk_per_trade: dec("0.02"),
            rules: vec![
                CreateRule { name: "morning_star".into(), expr: "morning_star == 1".into(), weight: dec("2.5") },
                CreateRule { name: "rsi".into(), expr: "rsi(14) < 45".into(), weight: dec("1") },
            ],
        },

        // 11. Piercing Line (EUR/USD)
        CreateStrategy {
            name: "Piercing Line Reversal".into(),
            description: Some("Piercing line candlestick: a bearish candle followed by a bullish candle that opens below the prior low but closes above the midpoint. Strong bottom reversal signal.".into()),
            asset_class: AssetClass::Forex,
            symbols: vec!["frxEURUSD".into()],
            stop_loss: Some(dec("25")),
            take_profit: Some(dec("50")),
            risk_per_trade: dec("0.01"),
            rules: vec![
                CreateRule { name: "piercing".into(), expr: "piercing_line == 1".into(), weight: dec("2") },
                CreateRule { name: "rsi".into(), expr: "rsi(14) < 40".into(), weight: dec("1") },
            ],
        },

        // 12. RSI + MACD Momentum (R_100)
        CreateStrategy {
            name: "RSI + MACD Momentum".into(),
            description: Some("Momentum strategy: RSI recovering from oversold plus positive MACD. Combines two of the most popular indicators for trend confirmation.".into()),
            asset_class: AssetClass::DerivIndex,
            symbols: vec!["R_100".into()],
            stop_loss: Some(dec("45")),
            take_profit: Some(dec("90")),
            risk_per_trade: dec("0.02"),
            rules: vec![
                CreateRule { name: "rsi_recovering".into(), expr: "rsi(14) > 30 and rsi(14) < 50".into(), weight: dec("1.5") },
                CreateRule { name: "macd_positive".into(), expr: "macd() > 0".into(), weight: dec("1.5") },
                CreateRule { name: "trend".into(), expr: "price > ema(50)".into(), weight: dec("1") },
            ],
        },
    ]
}

fn builtin_notes() -> Vec<crate::domain::note::CreateNote> {
    use crate::domain::note::CreateNote;
    vec![
        CreateNote {
            title: "RSI (Relative Strength Index) — How to Use It".into(),
            content: r#"# RSI — Relative Strength Index

RSI measures the speed and change of price movements on a scale of 0-100.

## Key Levels
- **Below 30**: Oversold — price may bounce up (buy signal)
- **Above 70**: Overbought — price may drop (sell signal)
- **50 line**: Midpoint — above 50 is bullish, below is bearish

## How to Trade RSI
1. **Reversal**: Buy when RSI crosses back above 30 from below. Sell when RSI crosses below 70.
2. **Divergence**: If price makes a lower low but RSI makes a higher low, a reversal is likely.
3. **Trend confirmation**: In an uptrend, RSI should stay above 40. In a downtrend, below 60.

## Best Combinations
- RSI + EMA: Confirm RSI signals with trend direction (price above EMA50)
- RSI + MACD: Wait for both to agree before entering
- RSI + Candlestick patterns: A hammer + RSI < 30 is a strong buy signal

## Rule DSL Examples
- `rsi(14) < 30` — oversold
- `rsi(14) > 70` — overbought
- `rsi(14) < 35 and price > ema(50)` — oversold in an uptrend
"#.into(),
            content_type: "markdown".into(),
        },
        CreateNote {
            title: "Candlestick Patterns Every Trader Should Know".into(),
            content: r#"# Candlestick Patterns

Candlesticks show the battle between buyers (bulls) and sellers (bears) in each time period.

## Single-Candle Patterns
- **Hammer**: Small body at top, long lower wick. Bullish reversal at bottoms.
- **Doji**: Open ≈ close. Indecision — potential reversal.
- **Dragonfly Doji**: Doji with long lower wick. Strong bullish signal at bottoms.
- **Gravestone Doji**: Doji with long upper wick. Bearish signal at tops.
- **Shooting Star**: Small body, long upper wick. Bearish reversal at tops.
- **Marubozu**: No wicks. Strong continuation signal.

## Two-Candle Patterns
- **Bullish Engulfing**: Large white candle engulfs prior black candle. Strong buy signal.
- **Bearish Engulfing**: Large black candle engulfs prior white candle. Strong sell signal.
- **Bullish Harami**: Small white candle inside large black candle. Reversal up.
- **Piercing Line**: Opens below prior low, closes above midpoint. Bullish reversal.
- **Dark Cloud Cover**: Opens above prior high, closes below midpoint. Bearish reversal.
- **Tweezer Bottom/Top**: Two candles with matching lows/highs. Minor reversal.

## Three-Candle Patterns
- **Morning Star**: Bearish → small body → bullish. Major bottom reversal.
- **Evening Star**: Bullish → small body → bearish. Major top reversal.
- **Three White Soldiers**: Three bullish candles, higher closes. Strong uptrend.
- **Three Black Crows**: Three bearish candles, lower closes. Strong downtrend.

## Rule DSL Examples
- `hammer == 1` — hammer pattern detected
- `bullish_engulfing == 1` — bullish engulfing detected
- `doji == 1 and rsi(14) < 35` — doji at oversold
"#.into(),
            content_type: "markdown".into(),
        },
        CreateNote {
            title: "Moving Averages — EMA and SMA".into(),
            content: r#"# Moving Averages

Moving averages smooth price data to show the underlying trend.

## Types
- **SMA (Simple Moving Average)**: Equal weight to all periods.
- **EMA (Exponential Moving Average)**: More weight to recent prices. Reacts faster.

## Key Averages
- **EMA 50**: Short-term trend
- **EMA 200**: Long-term trend (most watched)
- **Golden Cross**: EMA50 crosses above EMA200 — major bullish signal
- **Death Cross**: EMA50 crosses below EMA200 — major bearish signal

## Trading Rules
1. Price above EMA200 = long-term uptrend (only look for buys)
2. Price below EMA200 = long-term downtrend (only look for sells)
3. Price above EMA50 = short-term strength
4. Price pulling back to EMA50 = potential buy zone

## Rule DSL Examples
- `price > ema(50)` — price above 50 EMA
- `price > ema(200)` — price above 200 EMA (long-term uptrend)
- `ema(50) > ema(200)` — golden cross condition
"#.into(),
            content_type: "markdown".into(),
        },
        CreateNote {
            title: "MACD — Moving Average Convergence Divergence".into(),
            content: r#"# MACD

MACD shows the relationship between two moving averages (12 and 26 EMA).

## Components
- **MACD line**: EMA12 - EMA26 (the difference)
- **Signal line**: 9 EMA of the MACD line
- **Histogram**: MACD - Signal (visual momentum)

## Trading Rules
1. **MACD > 0**: Bullish momentum (buyers in control)
2. **MACD < 0**: Bearish momentum (sellers in control)
3. **MACD rising**: Momentum increasing
4. **MACD falling**: Momentum decreasing

## Best Used With
- EMA trend filter: Only take MACD signals in the direction of the EMA200
- RSI: Wait for MACD and RSI to agree
- Candlestick patterns: A hammer + MACD > 0 is a strong buy

## Rule DSL Examples
- `macd() > 0` — bullish momentum
- `macd() < 0` — bearish momentum
"#.into(),
            content_type: "markdown".into(),
        },
        CreateNote {
            title: "Risk Management — Position Sizing and Stop Losses".into(),
            content: r#"# Risk Management

The most important part of trading. Without it, even the best strategy will lose money.

## Golden Rules
1. **Never risk more than 1-2% of your account** on a single trade
2. **Always use a stop loss** — no exceptions
3. **Aim for at least 1:2 risk/reward ratio** (risk 1 to make 2)
4. **Don't overtrade** — wait for high-quality setups

## Position Sizing
- Account: $10,000
- Risk per trade: 1% = $100
- Stop loss: 30 pips on EUR/USD
- Position size: $100 / 30 pips = 3.3 mini lots

## Stop Loss Placement
- **Forex**: 20-40 pips for scalping, 50-100 for swing trading
- **Indices**: 30-100 points depending on volatility
- **Behind support**: Place below recent swing low (for buys)

## Take Profit
- **1:2 minimum**: If your stop is 30 pips, target at least 60 pips
- **Trail your stop**: Move stop to breakeven once trade is profitable
- **Partial close**: Take half off at 1:1, let the rest run

## In This App
- Set `risk_per_trade` to 0.01 (1%) or 0.02 (2%)
- Set `stop_loss` in pips (forex) or points (indices)
- Set `take_profit` to at least 2x your stop loss
- Use paper mode to test before going live
"#.into(),
            content_type: "markdown".into(),
        },
    ]
}
