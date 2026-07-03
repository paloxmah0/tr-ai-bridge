# AGENTS.md - Session Continuation Guide

## Project: Trading Backend + AI MT5 Bridge
**Repo:** github.com/paloxmah0/tr-ai-bridge
**Local path:** C:\Users\san\AppData\Local\Temp\opencode\tr
**Origin:** Forked from github.com/paloxmah0/tr

## Quick Recreation Steps (for a new session)

1. Clone: `git clone https://github.com/paloxmah0/tr-ai-bridge.git` into `C:\Users\san\AppData\Local\Temp\opencode\tr`
2. Configure `.env`: set `LLM_BASE_URL=https://text.pollinations.ai/openai`, `LLM_API_KEY=none`, `LLM_MODEL=openai` (free, no key)
3. Build: `cargo build --release` (or `build.bat`) — ~7 min
4. Run: `./target/release/trading-backend.exe` — server on :8080
5. Verify: `Invoke-RestMethod http://localhost:8080/api/accounts`
6. MT5 bridge: `python mt5_bridge_v2.py` — needs MT5 terminal running + Algo Trading enabled

## Key Decisions Made This Session

- **LLM:** Pollinations AI (free, no key). OpenRouter key was org-locked and unusable.
- **Strategy:** EURUSD trend-following with 2:1 RR (SL=10/TP=20 pips). Win rate 55%, return +5.7%, drawdown 1.8%.
- **Rise/Fall:** Tested extensively — does NOT work (~52% = coin flip, below 55.5% break-even).
- **MT5 bridge:** Uses `/api/analyze` endpoint (MTF+patterns+notes+memories). FOK filling mode. 0.01 lot. Max 1 position.
- **Deriv-index strategies:** Deleted (R_75/R_100 can't run on MT5). Only EURUSD kept.

## Build/Lint Commands
- Build: `cargo build --release`
- No test framework configured
- Lint: `cargo clippy` (not configured in CI)

## Critical Gotchas
- SL/TP for derivindex = price units (R_75 at 49000 needs SL=30+, not 5)
- SL/TP for forex = pips (EURUSD SL=10 pips = 0.00100)
- If backtest shows <5 trades → SL/TP too tight
- MT5 needs FOK filling mode (not IOC) on MetaQuotes-Demo
- `LLM_API_KEY` must be non-empty (app checks at client.rs:69) — use "none" for Pollinations
- Deriv rate limits: too many concurrent strategy evaluations → "ticks_history rate limit" warnings

## File Locations
- Backend source: `src/`
- AI engine (MTF+patterns+notes): `src/ai_engine.rs`
- News module (Forex Factory calendar): `src/news.rs` — blocks trades during high-impact news
- Learning loop (win/loss memories): `src/learning.rs`
- Rule DSL evaluator: `src/engine/rules.rs`
- Backtest harness: `src/backtest.rs`
- MT5 bridge: `mt5_bridge_v2.py` (root) and `opencode-scripts/mt5_bridge_v2.py`
- Rise/Fall testers: `risefall.py`, `risefall2.py`, `mr_r75.py`, `mtf2.py`
- Status check: `status_check.py` — run `python status_check.py` to see current state

## How to Check Status in a New Session

Run: `python status_check.py` — shows backend status, MT5 account balance/open positions, 24h trade history (wins/losses/PnL), and last bridge log lines.

## News Handling (Already Built In)
The app fetches the free Forex Factory calendar (`nfs.faireconomy.media/ff_calendar_thisweek.json`) every analysis cycle:
- **High-impact news imminent (30 min)** → status="danger" → trade BLOCKED (checklist fails)
- **Medium-impact news** → status="caution" → conviction reduced 20%
- **No news** → status="clear" → normal trading
- Filters by currency: EURUSD → EUR + USD events only
- No API key needed — Forex Factory calendar is free JSON
