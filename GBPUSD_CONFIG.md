# GBPUSD Bridge Configuration

This file documents the current GBPUSD bridge setup for the MT5 bridge v3.

## Current Configuration (active)

Bridge file: `mt5_bridge_v3.py`

```python
APP = "http://localhost:8080"
SYMBOL = "GBPUSD"
APP_SYMBOL = "frxGBPUSD"
TF_MINUTES = 5
MIN_LOT = 0.05        # tiered: <$500=0.05, $500-$1000=0.08, >$1000=0.10
SL_PIPS = 20
TP_PIPS = 10
MAGIC = 20260703
POLL_ANALYZE = 30
MAX_POSITIONS = dynamic  # balance/100 (e.g. $300=3, $400=4, $500=5, max 10)
```

## Settings

- **Symbol:** GBPUSD only (EURUSD fully removed)
- **Timeframe:** 5 min
- **SL/TP:** 20/10 pips (0.5:1 reward-to-risk — tight TP, favors win rate)
- **Lot:** Tiered by balance (see below)
- **Filling mode:** FOK (required on MetaQuotes-Demo)
- **Max positions:** Dynamic — scales with balance ($300=3, $500=5, etc.)
- **Magic:** 20260703
- **Poll interval:** 30s
- **Conviction threshold:** 55% (per-symbol tuned for GBPUSD)

## 5 Improvements in Bridge v3

1. **One symbol only** — GBPUSD, no EURUSD or other pairs
2. **Pattern optional** — candlestick pattern is a boost, not a block (more trades qualify)
3. **Stronger loss memories** — critical weight, 4 specific avoidance rules fed to AI
4. **Per-symbol conviction threshold** — 55% for GBPUSD (was 58%)
5. **Breakeven trailing stop** — at +8 pips profit, SL auto-moves to entry price

## Lot Size Tiers (self-adjusting)

```python
if balance >= 1000: lot = 0.10
elif balance >= 500: lot = 0.08
else: lot = 0.05
```

## Max Positions (self-adjusting)

```python
max_pos = int(balance / 100)  # $300=3, $400=4, $500=5...
# capped at 10
```

## Only Enabled Strategy

| Strategy | SL | TP | RR | Rules |
|---|---|---|---|---|
| GBPUSD RSI Reversal | 20 pips | 10 pips | 0.5:1 | rsi(14)<30 buy, rsi(14)>70 sell |

**All other strategies are DISABLED.** This is critical — the AI's `/api/analyze` endpoint uses all enabled strategies as evidence. Having junk strategies enabled pollutes the AI's signals.

## Backtest Results (GBPUSD RSI Reversal, 5000 candles)

| Metric | Value |
|---|---|
| Trades | 44 |
| Win rate | 68.2% |
| Return | +0.96% |
| Drawdown | 3.08% |
| Breakeven | 66.7% |

## MT5 Account

- **Login:** 5052679753
- **Server:** MetaQuotes-Demo
- **Starting balance:** $300 (demo)
- **Current balance:** Check with `python status_check.py`

## Running Everything

### One-time setup
```
cd C:\Users\san\AppData\Local\Temp\opencode\tr
# Ensure .env exists with LLM config (Pollinations)
# Ensure MT5 terminal running + Algo Trading enabled (green)
```

### Start backend
```powershell
Start-Process -FilePath "C:\Users\san\AppData\Local\Temp\opencode\tr\target\release\trading-backend.exe" -WindowStyle Hidden -RedirectStandardOutput "run.out" -RedirectStandardError "run.err" -WorkingDirectory "C:\Users\san\AppData\Local\Temp\opencode\tr"
```

### Start bridge
```powershell
Start-Process -FilePath "python" -ArgumentList "-u","C:\Users\san\AppData\Local\Temp\opencode\tr\mt5_bridge_v3.py" -WindowStyle Hidden -RedirectStandardOutput "bridge.out" -RedirectStandardError "bridge.err"
```

### Status check
```powershell
python status_check.py
```

### Or just tell opencode
```
check status
restart everything
```

## Critical Gotchas

- SL/TP for forex = pips (GBPUSD SL=20 pips = 0.00200)
- MT5 needs FOK filling mode (not IOC) on MetaQuotes-Demo
- High-impact news within 30 min → trade BLOCKED automatically by `src/news.rs`
- News filter covers GBP + USD events only
- Bridge stops when PC shuts down — open positions still resolve server-side (SL/TP at broker)
- Session filter: trades only 7AM-9PM UTC (10AM-midnight Kenya time)
- If server restarts, check that only GBPUSD RSI Reversal is enabled (other strategies may re-enable from DB)
