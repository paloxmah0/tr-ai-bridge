import time, json, sys, urllib.request, datetime
import MetaTrader5 as mt5

APP = "http://localhost:8080"
SYMBOL = "EURUSD"
# poll interval (seconds)
POLL = 5
# lot size per trade
LOT = 0.1
# pip-based SL/TP for EURUSD (matches app strategy SL=10/TP=20 pips)
# magic number to tag our orders
MAGIC = 20260703
SL_PIPS = 10
TP_PIPS = 20

def app_get(path):
    try:
        with urllib.request.urlopen(f"{APP}{path}", timeout=10) as r:
            return json.loads(r.read())
    except Exception as e:
        print(f"[app] {path} failed: {e}")
        return None

def notify(msg):
    print(f"[SIGNAL] {msg}", flush=True)

def place_mt5(side, sl_pips, tp_pips):
    if not mt5.initialize():
        print("[mt5] init failed", mt5.last_error()); return None
    info = mt5.symbol_info(SYMBOL)
    if not info:
        print(f"[mt5] {SYMBOL} not found"); return None
    if not info.visible:
        mt5.symbol_select(SYMBOL, True)
    tick = mt5.symbol_info_tick(SYMBOL)
    if not tick:
        print("[mt5] no tick"); return None
    price = tick.ask if side == "buy" else tick.bid
    point = info.point
    pip = point * (10 if info.digits == 5 else 1)
    if side == "buy":
        sl = price - sl_pips * pip; tp = price + tp_pips * pip
        order_type = mt5.ORDER_TYPE_BUY
    else:
        sl = price + sl_pips * pip; tp = price - tp_pips * pip
        order_type = mt5.ORDER_TYPE_SELL
    req = {
        "action": mt5.TRADE_ACTION_DEAL,
        "symbol": SYMBOL,
        "volume": float(LOT),
        "type": order_type,
        "price": price,
        "sl": round(sl, info.digits),
        "tp": round(tp, info.digits),
        "deviation": 20,
        "magic": MAGIC,
        "comment": "ai-bridge",
        "type_time": mt5.ORDER_TIME_GTC,
        "type_filling": mt5.ORDER_FILLING_FOK,
    }
    r = mt5.order_send(req)
    if r is None:
        print("[mt5] order_send returned None:", mt5.last_error()); return None
    if r.retcode != mt5.TRADE_RETCODE_DONE:
        print(f"[mt5] order failed retcode={r.retcode} {r.comment}"); return None
    print(f"[mt5] ORDER FILLED {side} {LOT} {SYMBOL} @ {price} SL={round(sl,info.digits)} TP={round(tp,info.digits)} ticket={r.order}")
    return r.order

def main():
    print(f"=== AI->MT5 bridge ===  app={APP}  symbol={SYMBOL}  lot={LOT}")
    seen = set()
    # preload seen with current signal ids so we only act on NEW signals
    sigs = app_get(f"/api/accounts")
    if sigs:
        for acc in sigs:
            lst = app_get(f"/api/accounts/{acc['id']}/signals") or []
            for s in lst:
                seen.add(s.get("id") or s.get("created_at"))
    print(f"preloaded {len(seen)} existing signals (won't re-trade them)")
    while True:
        sigs = app_get("/api/accounts")
        if not sigs:
            time.sleep(POLL); continue
        for acc in sigs:
            lst = app_get(f"/api/accounts/{acc['id']}/signals") or []
            for s in reversed(lst):  # newest handling
                key = s.get("id") or s.get("created_at")
                if key in seen: continue
                seen.add(key)
                sym = s.get("symbol","")
                side = (s.get("side") or "").lower()
                if sym.upper().replace("FRX","") != "EURUSD":
                    continue
                if side not in ("buy","sell"):
                    continue
                ts = s.get("created_at","")
                notify(f"NEW {side.upper()} {sym} @ {ts}  -> placing MT5 order")
                place_mt5(side, SL_PIPS, TP_PIPS)
        time.sleep(POLL)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("stopped"); mt5.shutdown()
