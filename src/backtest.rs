//! Historical backtesting harness.
//!
//! Replays a candle series through the rule engine bar-by-bar. At each bar it
//! computes indicators over the trailing window, evaluates the strategy's rules,
//! opens a simulated position on a fresh signal (one position at a time), and
//! closes it when price hits stop-loss/take-profit or on an opposite signal.
//! Produces an equity curve and summary statistics. Pure: no DB writes.

use crate::domain::strategy::{Rule, Strategy};
use crate::domain::{AssetClass, Candle, Side};
use crate::engine::rules::Indicators;
use crate::engine::evaluate_at;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Minimum trailing bars needed before the engine starts producing signals.
const WARMUP: usize = 210;

#[derive(Debug, Clone, Deserialize)]
pub struct BacktestRequest {
    /// Symbol to backtest (must match a symbol fetchable from the market provider).
    pub symbol: String,
    /// Initial account balance.
    #[serde(default = "default_balance")]
    pub initial_balance: Decimal,
    /// Number of historical candles to fetch (default 1000).
    #[serde(default = "default_candles")]
    pub candles: usize,
}

fn default_balance() -> Decimal { Decimal::new(10000, 0) }
fn default_candles() -> usize { 1000 }

#[derive(Debug, Clone, Serialize)]
pub struct BacktestTrade {
    pub side: Side,
    pub entry_ts: DateTime<Utc>,
    pub entry_price: Decimal,
    pub exit_ts: DateTime<Utc>,
    pub exit_price: Decimal,
    pub pnl: Decimal,
    pub exit_reason: String,
    pub strength: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub ts: DateTime<Utc>,
    pub equity: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestResult {
    pub symbol: String,
    pub initial_balance: Decimal,
    pub final_equity: Decimal,
    pub total_return_pct: Decimal,
    pub trades: Vec<BacktestTrade>,
    pub closed_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub avg_pnl: Decimal,
    pub max_drawdown_pct: Decimal,
    pub sharpe_ratio: Decimal,
    pub equity_curve: Vec<EquityPoint>,
}

/// Run a backtest for a strategy over the given candle series.
pub fn run(strategy: &Strategy, rules: &[Rule], candles: &[Candle]) -> AppResult<BacktestResult> {
    if candles.len() < WARMUP {
        return Err(AppError::BadRequest(format!(
            "need at least {WARMUP} candles for warmup, got {}",
            candles.len()
        )));
    }
    let symbol = strategy
        .symbols
        .first()
        .cloned()
        .unwrap_or_else(|| candles.first().map(|c| c.symbol.clone()).unwrap_or_default());

    let initial_balance = Decimal::new(10000, 0);
    let mut equity = initial_balance;
    let mut peak = equity;
    let mut max_dd = Decimal::ZERO;
    let mut equity_curve = Vec::new();
    let mut trades: Vec<BacktestTrade> = Vec::new();
    let mut returns: Vec<Decimal> = Vec::new();

    let mut position: Option<OpenPos> = None;

    for i in WARMUP..candles.len() {
        let window = &candles[..=i];
        let ind = Indicators::compute(window)?;
        let bar = &candles[i];

        // Manage open position against this bar's high/low first.
        if let Some(pos) = position.as_ref() {
            if let Some((exit_price, reason)) = check_exit(pos, bar) {
                let pnl = pos.pnl(exit_price);
                equity += pnl;
                returns.push(pnl);
                trades.push(BacktestTrade {
                    side: pos.side,
                    entry_ts: pos.entry_ts,
                    entry_price: pos.entry_price,
                    exit_ts: bar.ts,
                    exit_price,
                    pnl,
                    exit_reason: reason,
                    strength: pos.strength,
                });
                position = None;
            }
        }

        // Evaluate rules at this bar's close.
        if position.is_none() {
            if let Some(sig) = evaluate_at(rules, &ind)? {
                if sig.strength > Decimal::ZERO {
                    let (stop, tp) = levels(strategy, sig.side, sig.price);
                    position = Some(OpenPos {
                        side: sig.side,
                        entry_ts: bar.ts,
                        entry_price: bar.close,
                        stop,
                        take_profit: tp,
                        strength: sig.strength,
                        size: position_size(equity, strategy.risk_per_trade, bar.close, stop),
                    });
                }
            }
        }

        // Track equity curve + drawdown.
        let unrealized = position.as_ref().map(|p| p.unrealized(bar.close)).unwrap_or(Decimal::ZERO);
        let mark = (equity + unrealized).round_dp(6);
        equity_curve.push(EquityPoint { ts: bar.ts, equity: mark });
        if mark > peak { peak = mark; }
        let dd = if peak > Decimal::ZERO {
            ((peak - mark) / peak * Decimal::from(100)).round_dp(4)
        } else { Decimal::ZERO };
        if dd > max_dd { max_dd = dd; }
    }

    // Close any remaining position at the last close.
    if let Some(pos) = position {
        let last = candles.last().ok_or_else(|| AppError::Internal("no candles".into()))?;
        let pnl = pos.pnl(last.close);
        equity += pnl;
        returns.push(pnl);
        trades.push(BacktestTrade {
            side: pos.side,
            entry_ts: pos.entry_ts,
            entry_price: pos.entry_price,
            exit_ts: last.ts,
            exit_price: last.close,
            pnl,
            exit_reason: "end_of_data".into(),
            strength: pos.strength,
        });
    }

    let closed = trades.len();
    let wins = trades.iter().filter(|t| t.pnl > Decimal::ZERO).count();
    let losses = trades.iter().filter(|t| t.pnl <= Decimal::ZERO).count();
    let total_pnl = equity - initial_balance;
    let win_rate = if closed > 0 {
        Decimal::from(wins) / Decimal::from(closed)
    } else { Decimal::ZERO };
    let avg_pnl = if closed > 0 { total_pnl / Decimal::from(closed) } else { Decimal::ZERO };
    let total_return_pct = if initial_balance != Decimal::ZERO {
        (total_pnl / initial_balance * Decimal::from(100)).round_dp(2)
    } else { Decimal::ZERO };
    let sharpe = sharpe(&returns);

    Ok(BacktestResult {
        symbol,
        initial_balance,
        final_equity: equity,
        total_return_pct,
        trades,
        closed_trades: closed,
        winning_trades: wins,
        losing_trades: losses,
        win_rate,
        total_pnl,
        avg_pnl,
        max_drawdown_pct: max_dd,
        sharpe_ratio: sharpe,
        equity_curve,
    })
}

struct OpenPos {
    side: Side,
    entry_ts: DateTime<Utc>,
    entry_price: Decimal,
    stop: Option<Decimal>,
    take_profit: Option<Decimal>,
    strength: Decimal,
    size: Decimal,
}

impl OpenPos {
    fn pnl(&self, exit: Decimal) -> Decimal {
        let per_unit = match self.side {
            Side::Buy => exit - self.entry_price,
            Side::Sell => self.entry_price - exit,
        };
        (per_unit * self.size).round_dp(4)
    }
    fn unrealized(&self, price: Decimal) -> Decimal {
        self.pnl(price)
    }
}

/// Check whether the bar's high/low triggers SL/TP. Returns (exit_price, reason).
fn check_exit(pos: &OpenPos, bar: &Candle) -> Option<(Decimal, String)> {
    if let Some(sl) = pos.stop {
        match pos.side {
            Side::Buy if bar.low <= sl => return Some((sl, "stop_loss".into())),
            Side::Sell if bar.high >= sl => return Some((sl, "stop_loss".into())),
            _ => {}
        }
    }
    if let Some(tp) = pos.take_profit {
        match pos.side {
            Side::Buy if bar.high >= tp => return Some((tp, "take_profit".into())),
            Side::Sell if bar.low <= tp => return Some((tp, "take_profit".into())),
            _ => {}
        }
    }
    None
}

/// Convert SL/TP (in pips/points) to absolute price levels.
fn levels(strategy: &Strategy, side: Side, entry: Decimal) -> (Option<Decimal>, Option<Decimal>) {
    let pip = match strategy.asset_class {
        AssetClass::Forex => Decimal::new(1, 4),
        AssetClass::DerivIndex => Decimal::ONE,
    };
    let stop = strategy.stop_loss.map(|p| {
        let d = p * pip;
        match side { Side::Buy => entry - d, Side::Sell => entry + d }
    }).map(|v| v.round_dp(6));
    let tp = strategy.take_profit.map(|p| {
        let d = p * pip;
        match side { Side::Buy => entry + d, Side::Sell => entry - d }
    }).map(|v| v.round_dp(6));
    (stop, tp)
}

fn position_size(balance: Decimal, risk_per_trade: Decimal, entry: Decimal, stop: Option<Decimal>) -> Decimal {
    let risk_amount = balance * risk_per_trade;
    let size = match stop {
        Some(s) if s != entry && s != Decimal::ZERO => {
            let per_unit = (entry - s).abs();
            if per_unit == Decimal::ZERO { return Decimal::ZERO; }
            risk_amount / per_unit
        }
        _ => balance * Decimal::new(1, 2) / entry,
    };
    // Round to avoid Decimal precision overflow in downstream PnL calculations.
    size.round_dp(6)
}

/// Annualization-free, simplified Sharpe: mean(return) / std(return).
fn sharpe(returns: &[Decimal]) -> Decimal {
    if returns.len() < 2 { return Decimal::ZERO; }
    let n = Decimal::from(returns.len());
    let mean = returns.iter().sum::<Decimal>() / n;
    let var = returns.iter().map(|r| {
        let d = *r - mean;
        d * d
    }).sum::<Decimal>() / n;
    if var == Decimal::ZERO { return Decimal::ZERO; }
    // sqrt via f64 approximation.
    let std = Decimal::try_from(var.to_string().parse::<f64>().unwrap_or(0.0).sqrt())
        .unwrap_or(Decimal::ZERO);
    if std == Decimal::ZERO { Decimal::ZERO } else { mean / std }
}

/// Convenience: fetch candles via a market provider then run the backtest.
pub async fn run_with_provider(
    strategy: &Strategy,
    rules: &[Rule],
    market: &dyn crate::market::MarketProvider,
    symbol: &str,
    count: usize,
) -> AppResult<BacktestResult> {
    let candles = market.candles(symbol, count).await?;
    run(strategy, rules, &candles)
}
