# Full Restart Procedure

Use this when the backend or bridge stops (power outage, crash, PC restart).
opencode can do all of this for you — just say "restart everything".

## Quick Version (tell opencode)

```
restart everything
```

## Manual Version (PowerShell)

### Step 0: Kill old processes

```powershell
Get-Process trading-backend -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process python -ErrorAction SilentlyContinue | Where-Object {$_.WorkingSet64 -gt 10MB} | Stop-Process -Force
Start-Sleep -Seconds 3
```

### Step 1: Check .env exists

```powershell
Test-Path C:\Users\san\AppData\Local\Temp\opencode\tr\.env
```

If False, recreate it with:
```powershell
@'
SERVER_HOST=0.0.0.0
SERVER_PORT=8080
DATABASE_URL=sqlite://trading.db?mode=rwc
LLM_BASE_URL=https://text.pollinations.ai/openai
LLM_API_KEY=none
LLM_MODEL=openai
LLM_TIMEOUT_SECS=60
DEFAULT_TRADING_MODE=paper
DERIV_PROVIDER_BASE_URL=https://api.deriv.com
DERIV_PROVIDER_API_TOKEN=
DERIV_PROVIDER_ACCOUNT_ID=
DERIV_APP_ID=1089
DERIV_GRANULARITY_SECS=60
ENGINE_TICK_SECS=10
MAX_NOTE_UPLOAD_BYTES=5242880
'@ | Set-Content C:\Users\san\AppData\Local\Temp\opencode\tr\.env
```

### Step 2: Start backend

```powershell
Start-Process -FilePath "C:\Users\san\AppData\Local\Temp\opencode\tr\target\release\trading-backend.exe" -WindowStyle Hidden -RedirectStandardOutput "C:\Users\san\AppData\Local\Temp\opencode\backend.out" -RedirectStandardError "C:\Users\san\AppData\Local\Temp\opencode\backend.err" -WorkingDirectory "C:\Users\san\AppData\Local\Temp\opencode\tr"
Start-Sleep -Seconds 8
```

Verify:
```powershell
Invoke-RestMethod "http://localhost:8080/api/accounts" -TimeoutSec 10
```

### Step 3: Clean up strategies (CRITICAL)

The server re-enables junk strategies from the database on restart. This pollutes the AI's signals. ALWAYS run this after a restart:

```powershell
cd C:\Users\san\AppData\Local\Temp\opencode\tr
python cleanup_strategies.py
```

Expected output:
```
Cleaning up strategies...
  KEPT: GBPUSD RSI Reversal | ['frxGBPUSD'] | SL=20 TP=10

Done: 1 kept, 0 disabled.
OK: Only GBPUSD RSI Reversal is enabled. Safe to trade.
```

If it says "WARNING", investigate before starting the bridge.

### Step 4: Ensure MT5 is ready

- MT5 terminal must be running
- Logged into account 5052679753 (MetaQuotes-Demo)
- Algo Trading button must be GREEN (toolbar)
- Tools → Options → Expert Advisors → "Allow Automated Trading" checked

Verify:
```powershell
python -c "import MetaTrader5 as mt5; mt5.initialize(); a=mt5.account_info(); print('login=%s balance=%.2f trade_allowed=%s' % (a.login, a.balance, a.trade_allowed)); mt5.shutdown()"
```

### Step 5: Start bridge

```powershell
Start-Process -FilePath "python" -ArgumentList "-u","C:\Users\san\AppData\Local\Temp\opencode\tr\mt5_bridge_v3.py" -WindowStyle Hidden -RedirectStandardOutput "C:\Users\san\AppData\Local\Temp\opencode\bridge.out" -RedirectStandardError "C:\Users\san\AppData\Local\Temp\opencode\bridge.err"
Start-Sleep -Seconds 8
```

Verify:
```powershell
Get-Content C:\Users\san\AppData\Local\Temp\opencode\bridge.out
```

Should show:
```
=== AI MTF Bridge v3 ===
FIX 1: symbol=GBPUSD ONLY (no other pairs)
FIX 2: pattern is OPTIONAL (boost not block)
FIX 3: loss memories are STRONG (critical weight)
FIX 4: conviction threshold=55% (per-symbol tuned)
FIX 5: breakeven trailing stop at +8 pips
  lot=0.05 SL=20pips TP=10pips poll=30s max_pos=3
```

### Step 6: Full status check

```powershell
cd C:\Users\san\AppData\Local\Temp\opencode\tr
python status_check.py
```

## What survives a restart

| Component | Survives? | Notes |
|---|---|---|
| Open MT5 trades | YES | Broker server holds them |
| SL/TP on open trades | YES | Broker manages them |
| AI memories (loss/win notes) | YES | Stored in trading.db |
| AI strategies | YES | Stored in trading.db (but may re-enable junk ones) |
| Backend process | NO | Must restart manually |
| Bridge process | NO | Must restart manually |

## Common Issues

### "Unable to connect" after starting backend
- Wait 10 seconds (server is slow to start)
- Check if .env exists (Step 1)
- Check if port 8080 is occupied: `Get-NetTCPConnection -LocalPort 8080`

### "AutoTrading disabled by client"
- In MT5: click the Algo Trading button (green robot icon) to turn it green
- Tools → Options → Expert Advisors → check "Allow Automated Trading"

### "No money" error
- Lot size too big for balance. Check calc_lot() in mt5_bridge_v3.py
- At $300+ balance, 0.05 lot should work fine

### "Market closed"
- Forex is closed weekends (Friday ~11PM to Sunday ~11PM Kenya time)
- Positions stay frozen until market opens
- Bridge keeps analyzing but won't trade

### Deriv rate limit warnings
- "You have reached the rate limit for ticks_history"
- Normal when too many strategies are enabled
- cleanup_strategies.py fixes this by disabling junk strategies

## File Locations (all in one folder)

```
C:\Users\san\AppData\Local\Temp\opencode\tr\
  ├── .env                          # config (LLM, database, Deriv)
  ├── trading.db                    # database (strategies, memories, trades)
  ├── trading-backend.exe           # compiled server binary
  ├── mt5_bridge_v3.py              # the bridge (latest version)
  ├── status_check.py               # quick status check
  ├── cleanup_strategies.py         # disable junk strategies after restart
  ├── GBPUSD_CONFIG.md              # current config documentation
  ├── AGENTS.md                     # session continuation guide
  └── README_AI_BRIDGE.md           # full setup guide
```

## Quick Reference Commands

| What to do | Command |
|---|---|
| Check status | `python status_check.py` |
| Clean strategies | `python cleanup_strategies.py` |
| Start backend | (see Step 2) |
| Start bridge | (see Step 5) |
| Full restart | Tell opencode: `restart everything` |
| Check MT5 | `python -c "import MetaTrader5 as mt5; mt5.initialize(); print(mt5.account_info().balance); mt5.shutdown()"` |
