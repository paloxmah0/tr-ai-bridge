# GBPUSD Bridge Configuration

This file documents the current GBPUSD bridge setup for the MT5 bridge.

## Current Configuration (active)

Edit `mt5_bridge_v2.py` lines 4-12:

```python
APP = "http://localhost:8080"
SYMBOL = "GBPUSD"
APP_SYMBOL = "frxGBPUSD"
TF_MINUTES = 5
MIN_LOT = 0.01
SL_PIPS = 20
TP_PIPS = 10
RISK_PERCENT = 0.01  # risk 1% of balance per trade
MAGIC = 20260703
```

## Settings

- **Symbol:** GBPUSD
- **Timeframe:** 5 min
- **SL/TP:** 20/10 pips (0.5:1 reward-to-risk — tight TP, favors win rate)
- **Lot:** auto via `calc_lot()` (1% risk per trade, min 0.01)
- **Filling mode:** FOK (required on MetaQuotes-Demo)
- **Max positions:** 1 at a time
- **Magic:** 20260703
- **Poll interval:** 30s

## Auto-Lot Scaling

`calc_lot()` at line 16 auto-scales lot size to risk 1% of balance per trade:

```python
lot = (balance * 0.01) / (SL_PIPS * 10)
```

With SL=20 pips:
- $26 balance → 0.0013 → rounds to **0.01** (broker min floor)
- $200 → 0.01
- $500 → 0.02
- $1000 → 0.05
- $2000 → 0.10

Stuck at 0.01 floor until balance reaches ~$200+.

## GBPUSD Strategies (in backend DB)

Account ID: `6290d054-b1d3-4f2e-aeb9-4f3f4e6aa2db`

| Strategy ID | Name | SL (pips) | TP (pips) | RR | Notes |
|---|---|---|---|---|---|
| 6c7b6bbb-1132-46c2-8902-60a550af80da | GBPUSD RSI Reversal | 20 | 10 | 2:1 | Tight TP |
| 91227720-4bf3-4abb-ae5b-32e2a75bae84 | GBPUSD Mean-Reversion | 20 | 10 | 2:1 | Tight TP |
| 8851c63b-2e2f-445d-8bcc-39051969b80b | GBPUSD Wide 0.5:1 | 30 | 15 | 2:1 | Wide SL |
| 16a39227-278d-4cd8-a4cf-749dd773cf6d | GBPUSD 1:1 Test | 15 | 15 | 1:1 | Baseline |
| 7ea3e680-66c4-4f85-99c8-5a3699a2968a | GBPUSD Trend Rider 2:1 | 10 | 20 | 1:2 | Standard RR |
| defa4b29-4e6a-4002-8fcd-a234f4fb1b7d | GBPUSD Trend Rider | 20 | 10 | 2:1 | Tight TP (matches bridge) |

## Backtesting GBPUSD

```powershell
$body = @{symbol="frxGBPUSD"; candles=1000} | ConvertTo-Json
Invoke-RestMethod -Method POST `
  -Uri "http://localhost:8080/api/strategies/defa4b29-4e6a-4002-8fcd-a234f4fb1b7d/backtest" `
  -Body $body -ContentType "application/json"
```

## Running the Bridge

```powershell
# Start backend first
./target/release/trading-backend.exe

# Ensure MT5 terminal running + Algo Trading enabled

# Start bridge
python mt5_bridge_v2.py
```

Or detached with logging:
```powershell
Start-Process -FilePath "C:\Users\san\AppData\Local\Programs\Python\Python312\python.exe" `
  -ArgumentList "-u","C:\Users\san\AppData\Local\Temp\opencode\tr\mt5_bridge_v2.py" `
  -RedirectStandardOutput "C:\Users\san\AppData\Local\Temp\opencode\bridge3.out" `
  -RedirectStandardError "C:\Users\san\AppData\Local\Temp\opencode\bridge3.err" `
  -WindowStyle Hidden
```

## Status Check

```powershell
python status_check.py
```

Shows backend status, MT5 account balance/open positions, 24h trade history
(wins/losses/PnL), and last bridge log lines.

## Critical Gotchas

- SL/TP for forex = pips (GBPUSD SL=20 pips = 0.00200)
- MT5 needs FOK filling mode (not IOC) on MetaQuotes-Demo
- High-impact news within 30 min → trade BLOCKED automatically by `src/news.rs`
- News filter covers GBP + USD events only
- Bridge stops when PC shuts down — open positions still resolve server-side (SL/TP at broker)

## Switching to EURUSD

See `EURUSD_RETURN.md` for the procedure to switch the bridge back to EURUSD.
