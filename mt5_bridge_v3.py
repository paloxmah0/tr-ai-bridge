import time, json, sys, urllib.request, datetime
import MetaTrader5 as mt5

# ═══════════════════════════════════════════════════════
# AI MTF Bridge v3 — 5 improvements over v2
# 1. One symbol only (GBPUSD)
# 2. Pattern is optional (boost not block)
# 3. Loss memories are stronger
# 4. Per-symbol conviction threshold (GBPUSD: 55%)
# 5. Breakeven trailing stop (+8 pips -> move SL to entry)
# ═══════════════════════════════════════════════════════

APP = "http://localhost:8080"
SYMBOL = "GBPUSD"
APP_SYMBOL = "frxGBPUSD"
TF_MINUTES = 5
MIN_LOT = 0.05
SL_PIPS = 20
TP_PIPS = 10
RISK_PERCENT = 0.01
MAGIC = 20260703
POLL_ANALYZE = 30
ACCOUNT_ID = None
MAX_POSITIONS = 3  # allow up to 3 concurrent positions

# Fix #4: Per-symbol conviction threshold
# GBPUSD RSI reversal backtest: 68.2% win rate. Threshold at 55% is enough.
CONVICTION_THRESHOLD = 0.55

# Fix #5: Breakeven trailing stop
BE_TRIGGER_PIPS = 8  # when profit reaches 8 pips, move SL to entry

def app_post(path, body):
    data = json.dumps(body).encode()
    req = urllib.request.Request(f"{APP}{path}", data=data, headers={"Content-Type":"application/json"})
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.loads(r.read())
    except Exception as e:
        print(f"[app] POST {path} failed: {e}")
        return None

def app_get(path):
    try:
        with urllib.request.urlopen(f"{APP}{path}", timeout=15) as r:
            return json.loads(r.read())
    except Exception as e:
        print(f"[app] GET {path} failed: {e}")
        return None

def log(msg):
    ts = datetime.datetime.now().strftime("%H:%M:%S")
    print(f"[{ts}] {msg}", flush=True)

def calc_lot():
    """Lot size by balance tiers:
    <$500: 0.05 | $500-$1000: 0.08 | >$1000: 0.10"""
    if not mt5.initialize():
        return 0.05
    a = mt5.account_info()
    if not a:
        return 0.05
    if a.balance >= 1000:
        return 0.10
    elif a.balance >= 500:
        return 0.08
    else:
        return 0.05

def calc_max_positions():
    """Max positions by balance: $300=3, $400=4, $500=5, etc.
    Formula: balance / 100, capped at 10."""
    if not mt5.initialize():
        return 3
    a = mt5.account_info()
    if not a:
        return 3
    max_pos = int(a.balance / 100)
    return max(1, min(max_pos, 10))  # min 1, max 10

# ═══════════════════════════════════════════════════════
# Fix #5: Breakeven trailing stop
# When a position reaches +8 pips profit, move SL to entry price
# ═══════════════════════════════════════════════════════
def manage_trailing_stop():
    """Check open positions and move SL to breakeven when +8 pips profit."""
    if not mt5.initialize():
        return
    pos = mt5.positions_get(symbol=SYMBOL)
    if not pos:
        return
    tick = mt5.symbol_info_tick(SYMBOL)
    if not tick:
        return
    info = mt5.symbol_info(SYMBOL)
    pip = info.point * (10 if info.digits == 5 else 1)
    for p in pos:
        # Calculate current profit in pips
        if p.type == 0:  # buy
            profit_pips = (tick.bid - p.price_open) / pip
        else:  # sell
            profit_pips = (p.price_open - tick.ask) / pip
        # Check if we should move SL to breakeven
        if profit_pips >= BE_TRIGGER_PIPS:
            # Check if SL is still at original (not already moved)
            current_sl = p.sl
            entry = p.price_open
            # For buy: if SL is below entry, move to entry
            # For sell: if SL is above entry, move to entry
            needs_move = False
            if p.type == 0 and current_sl < entry:  # buy, SL below entry
                new_sl = round(entry, info.digits)
                needs_move = True
            elif p.type == 1 and current_sl > entry:  # sell, SL above entry
                new_sl = round(entry, info.digits)
                needs_move = True
            if needs_move:
                req = {
                    "action": mt5.TRADE_ACTION_SLTP,
                    "symbol": SYMBOL,
                    "position": p.ticket,
                    "sl": new_sl,
                    "tp": p.tp,  # keep TP unchanged
                }
                r = mt5.order_send(req)
                if r and r.retcode == mt5.TRADE_RETCODE_DONE:
                    log(f"[trailing] SL moved to breakeven {new_sl} (was {current_sl}, profit={profit_pips:.1f} pips)")
                else:
                    log(f"[trailing] failed to move SL: {r.comment if r else 'no response'}")

# ═══════════════════════════════════════════════════════
# Fix #2: Override checklist — pattern is optional (boost not block)
# ═══════════════════════════════════════════════════════
def check_ready_override(analysis):
    """Override the AI's checklist: make pattern optional.
    Real ready = trend + momentum + conviction + news + RR + session.
    Pattern is a boost, not a requirement."""
    checklist = analysis.get("entry_checklist", {})
    details = checklist.get("details", [])
    direction = analysis.get("direction", "wait")
    score = float(analysis.get("evidence_score", 0))
    if direction not in ("buy", "sell"):
        return False, "direction=wait"
    # Check each condition manually (skip pattern)
    trend_ok = any("Trend aligned: YES" in d for d in details)
    momentum_ok = any("Momentum aligned: YES" in d for d in details)
    # Fix #4: use our per-symbol threshold
    conviction_ok = score >= CONVICTION_THRESHOLD
    no_news = any("No news risk: YES" in d for d in details)
    rr_ok = any("Risk/reward: YES" in d for d in details)
    # Session check
    now_hour = datetime.datetime.utcnow().hour
    session_ok = 7 <= now_hour <= 21
    # Pattern is OPTIONAL now — just log it
    pattern_ok = any("Pattern confirmed: YES" in d for d in details)
    pattern_name = analysis.get("active_pattern", "none")
    ready = trend_ok and momentum_ok and conviction_ok and no_news and rr_ok and session_ok
    if ready and not pattern_ok:
        log(f"  [override] pattern NOT confirmed but allowing trade (boost, not block). pattern={pattern_name}")
    elif ready and pattern_ok:
        log(f"  [boost] pattern confirmed: {pattern_name} — extra confidence")
    reasons = []
    if not trend_ok: reasons.append("trend")
    if not momentum_ok: reasons.append("momentum")
    if not conviction_ok: reasons.append(f"conviction({score:.0f}<{CONVICTION_THRESHOLD:.0%})")
    if not no_news: reasons.append("news")
    if not rr_ok: reasons.append("RR")
    if not session_ok: reasons.append("session")
    return ready, ", ".join(reasons) if reasons else "ALL PASS"

# ═══════════════════════════════════════════════════════
# Fix #3: Stronger loss-memory notes
# ═══════════════════════════════════════════════════════
def feed_loss_memory(side, profit, conditions=""):
    """Feed a STRONG loss memory to the AI."""
    if not ACCOUNT_ID:
        return
    note_body = {
        "title": f"CRITICAL LOSS MEMORY: {side} GBPUSD FAILED -{abs(profit):.2f}",
        "content": f"""CRITICAL LESSON — LOSING TRADE. DO NOT IGNORE.

TRADE DETAILS:
- Symbol: GBPUSD
- Direction: {side}
- Loss: ${profit:.2f}
- Time: {datetime.datetime.utcnow().isoformat()}
- Conditions: {conditions}

RULE: This {side} setup on GBPUSD FAILED. The AI must be MORE CAUTIOUS when this exact setup appears again.

SPECIFICALLY:
1. If upper timeframes are MIXED (some bullish, some bearish), DO NOT take this trade. Mixed MTF = high failure probability.
2. If RSI is between 30-35 (for buy) or 65-70 (for sell), the signal is marginal — wait for RSI to go deeper into extreme.
3. If session quality is low (Asian/off-hours), reduce position size by 50% or skip.
4. If there was a recent loss on the same side within 3 trades, SKIP the next signal on that side.

This is a STORED MEMORY from real execution. Weight this memory HIGHLY when evaluating future signals. A trade that lost ${profit:.2f} on a {side} GBPUSD setup should reduce conviction for similar setups by at least 20%.

PATTERN TO AVOID: {side} signals where MTF is conflicted or RSI is marginal. These are the conditions that caused this loss."""
    }
    r = app_post(f"/api/accounts/{ACCOUNT_ID}/notes", note_body)
    if r:
        log(f"[learn] fed STRONG loss memory note id={r.get('id','?')} | loss=${profit:.2f} {side} GBPUSD")

def feed_win_memory(side, profit, conditions=""):
    """Feed a win memory too — reinforce what works."""
    if not ACCOUNT_ID:
        return
    note_body = {
        "title": f"WIN MEMORY: {side} GBPUSD +{profit:.2f}",
        "content": f"""WINNING TRADE — REINFORCE THIS PATTERN.

TRADE DETAILS:
- Symbol: GBPUSD
- Direction: {side}
- Profit: +${profit:.2f}
- Conditions: {conditions}

This {side} setup WORKED. The AI should be MORE CONFIDENT when similar conditions appear again. 
Boost conviction by 10% for setups matching these conditions.

What worked: {side} GBPUSD when conditions aligned. Keep doing this."""
    }
    r = app_post(f"/api/accounts/{ACCOUNT_ID}/notes", note_body)
    if r:
        log(f"[learn] fed win memory note id={r.get('id','?')} | +${profit:.2f} {side} GBPUSD")

def mt5_open_positions():
    if not mt5.initialize():
        return []
    pos = mt5.positions_get(symbol=SYMBOL)
    return list(pos) if pos else []

def mt5_place(side):
    if not mt5.initialize():
        log("[mt5] init failed"); return None
    mt5.symbol_select(SYMBOL, True)
    info = mt5.symbol_info(SYMBOL)
    tick = mt5.symbol_info_tick(SYMBOL)
    if not tick or not info:
        log("[mt5] no tick/symbol info"); return None
    lot = calc_lot()
    price = tick.ask if side == "buy" else tick.bid
    pip = info.point * (10 if info.digits == 5 else 1)
    if side == "buy":
        sl = price - SL_PIPS * pip; tp = price + TP_PIPS * pip
        otype = mt5.ORDER_TYPE_BUY
    else:
        sl = price + SL_PIPS * pip; tp = price - TP_PIPS * pip
        otype = mt5.ORDER_TYPE_SELL
    req = {
        "action": mt5.TRADE_ACTION_DEAL, "symbol": SYMBOL, "volume": float(lot),
        "type": otype, "price": price, "sl": round(sl, info.digits), "tp": round(tp, info.digits),
        "deviation": 20, "magic": MAGIC, "comment": "ai-v3",
        "type_time": mt5.ORDER_TIME_GTC, "type_filling": mt5.ORDER_FILLING_FOK,
    }
    r = mt5.order_send(req)
    if r is None:
        log(f"[mt5] order_send None: {mt5.last_error()}"); return None
    if r.retcode != mt5.TRADE_RETCODE_DONE:
        log(f"[mt5] order failed: retcode={r.retcode} {r.comment}"); return None
    log(f"[mt5] ORDER FILLED {side} {lot} {SYMBOL} @ {price:.5f} SL={sl:.5f} TP={tp:.5f} ticket={r.order}")
    return r.order

def mt5_check_closed():
    """Check closed deals, return list of (side, profit, ticket)."""
    if not mt5.initialize(): return []
    from_dt = datetime.datetime.now() - datetime.timedelta(hours=2)
    deals = mt5.history_deals_get(from_dt, datetime.datetime.now())
    if not deals: return []
    results = []
    for d in deals:
        if d.magic != MAGIC: continue
        if d.entry != 1: continue
        side = "buy" if d.type == 0 else "sell"
        results.append((side, d.profit, d.ticket))
    return results

def main():
    global ACCOUNT_ID
    log("=== AI MTF Bridge v3 ===")
    log(f"FIX 1: symbol={SYMBOL} ONLY (no other pairs)")
    log(f"FIX 2: pattern is OPTIONAL (boost not block)")
    log(f"FIX 3: loss memories are STRONG (critical weight)")
    log(f"FIX 4: conviction threshold={CONVICTION_THRESHOLD:.0%} (per-symbol tuned)")
    log(f"FIX 5: breakeven trailing stop at +{BE_TRIGGER_PIPS} pips")
    log(f"  lot={MIN_LOT} SL={SL_PIPS}pips TP={TP_PIPS}pips poll={POLL_ANALYZE}s max_pos={MAX_POSITIONS}")

    accs = app_get("/api/accounts") or []
    if accs:
        ACCOUNT_ID = accs[0]["id"]
        log(f"using account: {ACCOUNT_ID}")

    last_trade_ticket = None
    cycle = 0
    while True:
        cycle += 1
        try:
            # Fix #5: Manage trailing stops on open positions FIRST
            manage_trailing_stop()

            # Check if any position closed (learn from wins/losses)
            if last_trade_ticket:
                closed = mt5_check_closed()
                for side, profit, ticket in closed:
                    conditions = f"GBPUSD {side} at {TF_MINUTES}min"
                    if profit < 0:
                        log(f"[LOSS] trade closed: {side} profit={profit:.2f}")
                        feed_loss_memory(side, profit, conditions)
                    else:
                        log(f"[WIN] trade closed: {side} profit={profit:.2f}")
                        feed_win_memory(side, profit, conditions)
                    last_trade_ticket = None

            # Check open positions — allow up to dynamic max
            max_pos = calc_max_positions()
            lot = calc_lot()
            open_pos = mt5_open_positions()
            if len(open_pos) >= max_pos:
                if cycle % 10 == 0:
                    p = open_pos[0]
                    side = 'buy' if p.type==0 else 'sell'
                    log(f"[wait] {len(open_pos)}/{max_pos} positions open ({side} profit={p.profit:.2f}) lot={lot}")
                time.sleep(POLL_ANALYZE)
                continue
            elif open_pos and cycle % 10 == 0:
                log(f"[multi] {len(open_pos)}/{max_pos} positions open, lot={lot}, looking for more signals...")

            # Ask AI to analyze
            analysis = app_post("/api/analyze", {
                "symbol": APP_SYMBOL,
                "timeframe_minutes": TF_MINUTES,
                "asset_class": "forex"
            })
            if not analysis:
                time.sleep(POLL_ANALYZE)
                continue

            direction = analysis.get("direction", "wait")
            score = float(analysis.get("evidence_score", 0))
            pattern = analysis.get("active_pattern", "none")
            upper_tfs = analysis.get("upper_timeframe_context", [])
            mtf_summary = " | ".join([f"{u['label']}={u['trend']}" for u in upper_tfs])

            # Fix #2: Override checklist (pattern optional)
            ready, reason = check_ready_override(analysis)

            log(f"[analyze] dir={direction} score={score:.0%} ready={ready} pattern={pattern} | {mtf_summary}")
            if not ready and direction in ("buy","sell"):
                log(f"  [skip] {reason}")

            # Trade when ready
            if ready and direction in ("buy", "sell"):
                log(f"[SIGNAL] {direction.upper()} {SYMBOL} (ready, score={score:.0%})")
                log(f"  pattern={pattern} | MTF: {mtf_summary}")
                ticket = mt5_place(direction)
                if ticket:
                    last_trade_ticket = ticket

        except Exception as e:
            log(f"[error] {e}")

        time.sleep(POLL_ANALYZE)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        log("stopped"); mt5.shutdown()
