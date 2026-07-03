import time, json, sys, urllib.request, datetime
import MetaTrader5 as mt5

APP = "http://localhost:8080"
SYMBOL = "EURUSD"
APP_SYMBOL = "frxEURUSD"
TF_MINUTES = 5
LOT = 0.01
SL_PIPS = 10
TP_PIPS = 20
MAGIC = 20260703
POLL_ANALYZE = 30  # seconds between AI analyses
ACCOUNT_ID = None  # set after startup

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

def mt5_open_positions():
    if not mt5.initialize():
        log("[mt5] init failed"); return []
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
    price = tick.ask if side == "buy" else tick.bid
    pip = info.point * (10 if info.digits == 5 else 1)
    if side == "buy":
        sl = price - SL_PIPS * pip; tp = price + TP_PIPS * pip
        otype = mt5.ORDER_TYPE_BUY
    else:
        sl = price + SL_PIPS * pip; tp = price - TP_PIPS * pip
        otype = mt5.ORDER_TYPE_SELL
    req = {
        "action": mt5.TRADE_ACTION_DEAL, "symbol": SYMBOL, "volume": float(LOT),
        "type": otype, "price": price, "sl": round(sl, info.digits), "tp": round(tp, info.digits),
        "deviation": 20, "magic": MAGIC, "comment": "ai-mtf",
        "type_time": mt5.ORDER_TIME_GTC, "type_filling": mt5.ORDER_FILLING_FOK,
    }
    r = mt5.order_send(req)
    if r is None:
        log(f"[mt5] order_send None: {mt5.last_error()}"); return None
    if r.retcode != mt5.TRADE_RETCODE_DONE:
        log(f"[mt5] order failed: retcode={r.retcode} {r.comment}"); return None
    log(f"[mt5] ORDER FILLED {side} {LOT} {SYMBOL} @ {price:.5f} SL={sl:.5f} TP={tp:.5f} ticket={r.order}")
    return r.order

def mt5_check_closed():
    """Check if any position closed since last check, return list of (side, profit, ticket)."""
    if not mt5.initialize(): return []
    from_dt = datetime.datetime.now() - datetime.timedelta(hours=1)
    deals = mt5.history_deals_get(from_dt, datetime.datetime.now())
    if not deals: return []
    results = []
    for d in deals:
        if d.magic != MAGIC: continue
        if d.entry != 1: continue  # only closed positions
        side = "buy" if d.type == 0 else "sell"
        results.append((side, d.profit, d.ticket))
    return results

def feed_loss_memory(side, reason=""):
    """When a trade loses, feed a memory note to the AI so it learns."""
    if not ACCOUNT_ID: return
    note_body = {
        "title": f"LOSS MEMORY: {side} EURUSD failed",
        "content": f"LESSON LEARNED - LOSING TRADE.\nA {side} trade on EURUSD was opened and hit stop loss.\n\nMarket context at loss: {reason}\n\nRULE: The AI should be MORE CAUTIOUS in similar conditions. When this exact setup appears again, increase conviction threshold or skip the trade. This is a stored memory from real execution - do not ignore.\n\nPattern to AVOID: {side} signals that fail typically occur when upper timeframes are mixed or session is weak. Require ALL upper timeframes aligned before taking this setup again."
    }
    r = app_post(f"/api/accounts/{ACCOUNT_ID}/notes", note_body)
    if r:
        log(f"[learn] fed loss memory note id={r.get('id','?')}")

def main():
    global ACCOUNT_ID
    log("=== AI MTF Bridge -> MT5 ===")
    log(f"symbol={SYMBOL} tf={TF_MINUTES}min lot={LOT} SL={SL_PIPS}pips TP={TP_PIPS}pips")
    log(f"poll interval={POLL_ANALYZE}s  max 1 position at a time")

    # Find account
    accs = app_get("/api/accounts") or []
    if accs:
        ACCOUNT_ID = accs[0]["id"]
        log(f"using account: {ACCOUNT_ID}")

    last_trade_ticket = None
    cycle = 0
    while True:
        cycle += 1
        try:
            # Step 1: Check if any MT5 position closed (learn from losses)
            if last_trade_ticket:
                closed = mt5_check_closed()
                for side, profit, ticket in closed:
                    if ticket == last_trade_ticket or True:  # check all our deals
                        if profit < 0:
                            log(f"[loss] trade closed: {side} profit={profit:.2f} - feeding memory to AI")
                            feed_loss_memory(side, f"{side} EURUSD at {TF_MINUTES}min timeframe, lost {profit:.2f}")
                        else:
                            log(f"[win] trade closed: {side} profit={profit:.2f}")
                        last_trade_ticket = None

            # Step 2: Check open positions - if any, skip new trades
            open_pos = mt5_open_positions()
            if open_pos:
                if cycle % 10 == 0:  # log every 10 cycles to avoid spam
                    log(f"[wait] {len(open_pos)} position open, waiting for close...")
                time.sleep(POLL_ANALYZE)
                continue

            # Step 3: Ask AI to analyze the market (MTF + patterns + notes + memories)
            analysis = app_post("/api/analyze", {
                "symbol": APP_SYMBOL,
                "timeframe_minutes": TF_MINUTES,
                "asset_class": "forex"
            })
            if not analysis:
                time.sleep(POLL_ANALYZE)
                continue

            direction = analysis.get("direction", "wait")
            score = analysis.get("evidence_score", 0)
            ready = analysis.get("entry_checklist", {}).get("ready", False)
            pattern = analysis.get("active_pattern", "none")
            upper_tfs = analysis.get("upper_timeframe_context", [])
            mtf_summary = " | ".join([f"{u['label']}={u['trend']}" for u in upper_tfs])

            log(f"[analyze] dir={direction} score={score} ready={ready} pattern={pattern} | {mtf_summary}")

            # Step 4: Only trade when AI says ready AND direction is clear
            if ready and direction in ("buy", "sell"):
                log(f"[SIGNAL] AI says {direction.upper()} {SYMBOL} (ready=true, score={score})")
                log(f"  pattern={pattern}")
                log(f"  MTF: {mtf_summary}")
                checklist = analysis.get("entry_checklist", {}).get("details", [])
                for d in checklist:
                    log(f"  {d}")
                ticket = mt5_place(direction)
                if ticket:
                    last_trade_ticket = ticket
            elif direction in ("buy", "sell") and not ready:
                log(f"  [skip] direction={direction} but not ready (checklist failed)")

        except Exception as e:
            log(f"[error] {e}")

        time.sleep(POLL_ANALYZE)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        log("stopped"); mt5.shutdown()
