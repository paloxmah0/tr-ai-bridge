# Switching Back to EURUSD

This file documents the procedure to switch the MT5 bridge from GBPUSD back to EURUSD,
including the EURUSD strategies available in the backend.

## Quick Switch (one-line change)

Edit `mt5_bridge_v2.py` lines 5-10:

```python
# FROM (current GBPUSD):
SYMBOL = "GBPUSD"
APP_SYMBOL = "frxGBPUSD"
SL_PIPS = 20
TP_PIPS = 10

# TO (EURUSD, 2:1 RR per AGENTS.md strategy):
SYMBOL = "EURUSD"
APP_SYMBOL = "frxEURUSD"
SL_PIPS = 10
TP_PIPS = 20
```

Then restart the bridge:
```powershell
Stop-Process -Id <bridge_pid> -Force
python mt5_bridge_v2.py
```

## Alternative: Use the EURUSD copy directly

`opencode-scripts/mt5_bridge_v2.py` is already configured for EURUSD
(SL=10, TP=20, fixed LOT=0.01). To run it instead:

```powershell
python opencode-scripts/mt5_bridge_v2.py
```

Note: that version uses a fixed `LOT = 0.01`, not auto-scaling. The root
`mt5_bridge_v2.py` uses `calc_lot()` which auto-scales to 1% risk per trade.

## EURUSD Strategies (in backend DB)

Account ID: `6290d054-b1d3-4f2e-aeb9-4f3f4e6aa2db`

| Strategy ID | Name | SL (pips) | TP (pips) | RR | Notes |
|---|---|---|---|---|---|
| 3d5297e7-afb3-4ade-8b5b-8a8ee928153d | EURUSD Trend Rider v3 | 10 | 20 | 1:2 | Recommended per AGENTS.md. 55% win rate, +5.7% return, 1.8% DD in prior backtest |
| 72205f55-3b19-4b8d-9dd5-5bd3b3a8d1e0 | EURUSD 1:1 Test | 10 | 10 | 1:1 | Baseline comparison |
| 8ffa0a67-500e-4ef8-82f8-6467dd5d4ef6 | EURUSD Tight TP Test | 20 | 10 | 2:1 | Reversed RR (favor TP) |

## Recommended Configuration (per AGENTS.md "Key Decisions")

- **Symbol:** EURUSD
- **Timeframe:** 5 min
- **SL/TP:** 10/20 pips (2:1 reward-to-risk)
- **Lot:** auto via `calc_lot()` (1% risk per trade, min 0.01)
- **Filling mode:** FOK (required on MetaQuotes-Demo)
- **Max positions:** 1 at a time
- **Magic:** 20260703

## Backtesting EURUSD

```powershell
$body = @{symbol="frxEURUSD"; candles=1000} | ConvertTo-Json
Invoke-RestMethod -Method POST `
  -Uri "http://localhost:8080/api/strategies/3d5297e7-afb3-4ade-8b5b-8a8ee928153d/backtest" `
  -Body $body -ContentType "application/json"
```

Note: prior backtest of Trend Rider v3 over 1000 candles produced only 1 trade
(rules too restrictive). If <5 trades appear, SL/TP too tight or rules too strict.

## Critical Gotchas (from AGENTS.md)

- SL/TP for forex = pips (EURUSD SL=10 pips = 0.00100)
- If backtest shows <5 trades → SL/TP too tight
- MT5 needs FOK filling mode (not IOC) on MetaQuotes-Demo
- High-impact news within 30 min → trade BLOCKED automatically by `src/news.rs`
- News filter covers EUR + USD events only

## Current State

Bridge currently runs GBPUSD (SL=20, TP=10, 0.5:1 RR). To return to EURUSD,
apply the change above. The backend does not need rebuilding — only the bridge
script changes.
