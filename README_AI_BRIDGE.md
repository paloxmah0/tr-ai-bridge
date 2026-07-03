# Trading Backend + AI MT5 Bridge

A Rust trading backend that learns trading strategies from notes via an OpenAI-compatible LLM, evaluates them against live market data (Deriv WebSocket), and an AI bridge that connects the app's multi-timeframe analysis to MetaTrader 5 for automated execution.

## Project Origin

Forked from [paloxmah0/tr](https://github.com/paloxmah0/tr) — a Rust/Axum backend that ingests trading notes, uses an LLM to extract executable rule-DSL strategies, and runs them against live Deriv market data.

## What This Fork Adds

1. **Pollinations AI integration** — free, no-key, no-account OpenAI-compatible LLM endpoint (replaces the need for an OpenAI/OpenRouter API key)
2. **MT5 Bridge** (`mt5_bridge_v2.py`) — Python script that polls the app's `/api/analyze` endpoint (which uses MTF + patterns + notes + learning memories) and auto-executes trades in MetaTrader 5
3. **Rise/Fall testing scripts** — Python scripts that fetch real Deriv candles and test directional signal accuracy
4. **Tested AI-extracted strategies** — R_75 and EURUSD trend-following strategies with documented backtest results
5. **Drawdown-controlled strategy** — `ema(50)>ema(200)` alignment filter that cut drawdown from 43% to 17%

## Quick Start (How to Recreate the Working State)

### Prerequisites
- Rust 1.75+ (rustup.rs)
- Python 3.12+ with: `pip install requests pandas numpy websocket-client MetaTrader5`
- MetaTrader 5 terminal installed and logged in (demo or live)
- Node.js 18+ (only for frontend build, optional)

### 1. Clone and Configure
```bash
git clone https://github.com/paloxmah0/tr-ai-bridge.git
cd tr-ai-bridge
cp .env.example .env
```

Edit `.env` — set these two lines for the free Pollinations LLM (no API key needed):
```
LLM_BASE_URL=https://text.pollinations.ai/openai
LLM_API_KEY=none
LLM_MODEL=openai
```

Everything else stays default:
- `DATABASE_URL=sqlite://trading.db?mode=rwc` (file-based, no PostgreSQL needed)
- `DERIV_APP_ID=1089` (Deriv's public test ID — free anonymous market data)
- `DEFAULT_TRADING_MODE=paper`

### 2. Build
```bash
# Windows
build.bat

# Or manual
cargo build --release    # ~7 minutes first time
```

### 3. Run the Backend
```bash
# Windows
start.bat

# Or manual
./target/release/trading-backend.exe
```
Server listens on `http://localhost:8080`.

### 4. Start the MT5 Bridge
```bash
python mt5_bridge_v2.py
```

**Requirements for the bridge:**
- MT5 terminal must be running and logged in
- In MT5: enable **Algo Trading** (the green robot button in toolbar)
- Tools → Options → Expert Advisors → check "Allow Automated Trading"
- The bridge uses **FOK filling mode** (confirmed working on MetaQuotes-Demo)

### 5. Feed the AI a Strategy Note
The AI extracts strategies from natural-language notes. Example:
```bash
curl -X POST localhost:8080/api/accounts/<id>/notes \
  -H 'content-type: application/json' \
  -d '{"title":"EURUSD Trend Rider","content":"...your strategy description..."}'

# Trigger extraction
curl -X POST localhost:8080/api/notes/<note_id>
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  Rust Backend (trading-backend.exe)                 │
│  - /api/analyze: MTF + patterns + notes + memories  │
│  - /api/strategies: CRUD strategies                 │
│  - /api/strategies/:id/backtest: backtest           │
│  - LLM: Pollinations (free, no key)                 │
│  - Market data: Deriv WebSocket (anonymous)         │
│  - Learning loop: tracks win/loss per symbol        │
└────────────────────┬────────────────────────────────┘
                     │ POST /api/analyze (every 30s)
                     ▼
┌─────────────────────────────────────────────────────┐
│  MT5 Bridge (mt5_bridge_v2.py)                      │
│  - Polls AI analysis every 30s                      │
│  - Only trades when checklist ready=true            │
│  - Max 1 position at a time                         │
│  - 0.01 lot, SL=10 pips, TP=20 pips (2:1 RR)        │
│  - On loss: feeds memory note back to AI            │
└────────────────────┬────────────────────────────────┘
                     │ order_send()
                     ▼
┌─────────────────────────────────────────────────────┐
│  MetaTrader 5 Terminal                              │
│  - Executes real orders (demo or live)              │
│  - SL/TP managed by MT5 automatically               │
└─────────────────────────────────────────────────────┘
```

## Key Findings (Tested with Real Data)

### Rise/Fall (Binary Direction) — DOES NOT WORK
- R_75/R_100 next-candle direction accuracy: **~52%** (coin flip)
- With MTF filter: improved to **~54%** but still below **55.5% break-even** (Deriv ~80% payout)
- Conclusion: synthetic indices are near-random-walk at candle level; no rule beats break-even

### Trend-Following with 2:1 RR — PROFITABLE
- R_75 Trend Rider v3 (5000 candles): **+58% return, 17% drawdown, 36% win rate**
- EURUSD Trend Rider v3 (5000 candles): **+5.7% return, 1.8% drawdown, 55% win rate**
- The edge is reward:risk (2:1), not directional accuracy. Breakeven = 33.3% win rate.

### Critical: SL/TP Must Scale to Price
- Deriv-index SL/TP is in **price units** (1 point = 1.0). R_75 trades at ~49,000, so SL=5 = 0.01% (noise). Need SL=30+.
- Forex SL/TP is in **pips** (0.0001). EURUSD SL=30 pips = 0.00300. SL=10 pips works well.
- If backtest shows <5 trades, SL/TP is too tight — scale up.

## The AI Analysis Endpoint

`POST /api/analyze` with body `{"symbol":"frxEURUSD","timeframe_minutes":5,"asset_class":"forex"}` returns a full analysis:

- **Direction**: buy/sell/wait (derived from weighted evidence, not guessed)
- **Evidence score**: conviction ratio (must be ≥58%)
- **7-point checklist**: trend + momentum + pattern + conviction + news + RR + session — ALL must pass
- **MTF context**: 15min, 1H, 4H, Daily trend alignment
- **Patterns**: hammer, engulfing, morning star, doji, etc. (full candlestick detection)
- **Notes**: your strategy rules evaluated as evidence
- **Memories**: learning scores from past trades (stored in DB)
- **AI insight**: LLM enhancement of the analysis

The bridge only places trades when `entry_checklist.ready == true`.

## Learning from Losses

The app has a built-in learning loop (`src/learning.rs`):
- Every 60s, analyzes closed trades and updates a win/loss score table per symbol/direction
- Scores stored in DB as JSON (`learn_scores` setting)
- The AI engine reads these to boost evidence sources that win and dampen those that lose

The MT5 bridge adds an additional layer: when a trade closes at a loss, it POSTs a "LOSS MEMORY" note to the account, so the AI has explicit text memory of what failed.

## API Reference

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/health` | Liveness |
| GET/POST | `/api/accounts` | List/create accounts |
| POST | `/api/accounts/:id/mode` | Set mode (paper/signals/live) |
| GET/POST | `/api/accounts/:id/strategies` | List/create strategies |
| GET/PUT/DELETE | `/api/strategies/:id` | Get/update/delete strategy |
| GET/POST | `/api/accounts/:id/notes` | List/upload notes |
| POST | `/api/notes/:id` | Trigger LLM extraction |
| GET | `/api/accounts/:id/signals` | Recent signals |
| GET | `/api/accounts/:id/trades` | Trades |
| POST | `/api/trades/:id/close` | Close a trade |
| POST | `/api/strategies/:id/backtest` | Backtest over historical candles |
| **POST** | **`/api/analyze`** | **Full AI analysis (MTF + patterns + notes + memories)** |

## Rule DSL

Boolean expressions evaluated against computed indicators:
```
rsi(14) < 30
price > ema(50) and macd() > 0
ema(50) > ema(200) and rsi(14) > 40 and rsi(14) < 65
```

Functions: `rsi(p)`, `ema(p)`, `sma(p)`, `atr(p)`, `macd()`, `price`/`close`, `high`, `low`, `open`, `volume`, `pct_change`, `cross`, `crossup`, `crossdown`.
Operators: `and or not`, `< <= > >= == !=`, `+ - * /`, parentheses.
Patterns: `hammer`, `bullish_engulfing`, `bearish_engulfing`, `doji`, `morning_star`, `evening_star`, `three_white_soldiers`, `three_black_crows`, `piercing_line`, `dark_cloud_cover`, etc.

## Files Added in This Fork

| File | Purpose |
|------|---------|
| `mt5_bridge_v2.py` | **Main MT5 bridge** — polls /api/analyze, executes in MT5, learns from losses |
| `mt5_bridge.py` | Earlier bridge version (basic signal-based, no MTF) |
| `risefall.py` | Rise/Fall tester — 1-candle directional accuracy on R_100 |
| `risefall2.py` | Multi-horizon rise/fall tester (1/3/5/10 candles) |
| `mr_r75.py` | Mean-reversion Rise/Fall tester for R_75 (5min/10min) |
| `mtf_r75.py` | MTF-filtered Rise/Fall tester (slow version) |
| `mtf2.py` | MTF-filtered Rise/Fall tester (optimized, with patterns) |
| `opencode-scripts/` | Copy of all Python scripts |

## Safety Notes

- **Live trading is disabled by default** (`DEFAULT_TRADING_MODE=paper`)
- The MT5 bridge starts on whatever MT5 account is logged in — use a **demo account first**
- For a $30 live account: use 0.01 lot (minimum), 10-pip SL = ~$1 risk per trade (3.3%)
- The AI only trades when ALL 7 checklist conditions pass — it's conservative by design
- Always review AI-extracted strategies before enabling live mode
- Trading forex and derivatives carries substantial risk. This is research/educational software.

## LLM Setup (No API Key Needed)

This fork uses **Pollinations AI** (`text.pollinations.ai/openai`) — a free, no-key, no-account OpenAI-compatible endpoint.

In `.env`:
```
LLM_BASE_URL=https://text.pollinations.ai/openai
LLM_API_KEY=none
LLM_MODEL=openai
```

The app appends `/chat/completions` to `LLM_BASE_URL`, so the full URL becomes `https://text.pollinations.ai/openai/chat/completions`.

The `LLM_API_KEY=none` is a dummy value — the app checks for empty strings (client.rs:69) and errors if empty, so a non-empty placeholder is required even though Pollinations doesn't need a key.

### Why not OpenAI/OpenRouter?
- OpenAI requires a paid API key
- OpenRouter keys are often org-locked with data-policy restrictions that block all models
- Pollinations is free, works instantly, supports JSON mode (`response_format: {type: "json_object"}`)

## How to Recreate the Working State in a New Session

1. Clone this repo
2. `cp .env.example .env` and set the Pollinations LLM config (see above)
3. `cargo build --release` (or `build.bat`)
4. Start backend: `./target/release/trading-backend.exe` (or `start.bat`)
5. Verify: `curl http://localhost:8080/api/accounts` — should return demo account
6. Verify LLM: `curl -X POST http://localhost:8080/api/analyze -H 'content-type: application/json' -d '{"symbol":"frxEURUSD","timeframe_minutes":5,"asset_class":"forex"}'`
7. Install Python deps: `pip install MetaTrader5 websocket-client requests numpy`
8. Ensure MT5 terminal is running, logged in, and Algo Trading is enabled
9. Start bridge: `python mt5_bridge_v2.py`
10. The bridge will poll every 30s and trade when the AI says ready

### To recreate the EURUSD trend strategy via AI:
```bash
# Upload note
curl -X POST localhost:8080/api/accounts/<id>/notes \
  -H 'content-type: application/json' \
  -d '{"title":"EURUSD Trend Rider","content":"Trend-following: price>ema(50) and price>ema(200) and ema(50)>ema(200) and rsi(14)>40 and rsi(14)<65 and macd()>0. SL=10 pips TP=20 pips (2:1 RR). risk=0.5%."}'

# Extract
curl -X POST localhost:8080/api/notes/<note_id>
```

## Tech Stack

- **Rust** (edition 2021), Axum, Tokio
- **SQLite** + SQLx (file-based, no install needed)
- **reqwest** + **tokio-tungstenite** for HTTP/WebSocket
- **Python 3.12** for the MT5 bridge (MetaTrader5 package)
- **Pollinations AI** for LLM (free, OpenAI-compatible)
- **Deriv WebSocket** for market data (anonymous, app_id 1089)
