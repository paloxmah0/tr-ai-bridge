import json, time, websocket, numpy as np

SYMBOL = "R_100"
GRAN = 60
COUNT = 5000

def fetch_candles(symbol, gran, count):
    ws = websocket.create_connection(
        "wss://ws.derivws.com/websockets/v3?app_id=1089", timeout=30)
    req = {"ticks_history": symbol, "end": "latest", "style": "candles",
           "granularity": gran, "count": count}
    ws.send(json.dumps(req))
    msg = json.loads(ws.recv())
    ws.close()
    if "error" in msg:
        raise RuntimeError(f"Deriv error: {msg['error']}")
    cols = msg["candles"]
    # cols: list of [epoch, open, high, low, close]
    o = np.array([c["open"] for c in cols], float)
    h = np.array([c["high"] for c in cols], float)
    l = np.array([c["low"] for c in cols], float)
    c = np.array([c["close"] for c in cols], float)
    return o, h, l, c

def ema(closes, period):
    if len(closes) < period: return None
    k = 2.0/(period+1)
    e = closes[:period].mean()
    for i in range(period, len(closes)):
        e = c[i]*k + e*(1-k)
    return e

def rsi(closes, period=14):
    if len(closes) <= period: return 50.0
    w = closes[len(closes)-period-1:]
    diffs = np.diff(w)
    gains = np.where(diffs>0, diffs, 0.0).sum()
    losses = np.where(diffs<0, -diffs, 0.0).sum()
    avg_gain = gains/period
    avg_loss = losses/period
    if avg_loss == 0: return 100.0
    rs = avg_gain/avg_loss
    return 100 - 100/(1+rs)

def macd(closes):
    fast = ema(closes, 12)
    slow = ema(closes, 26)
    if fast is None or slow is None: return 0.0
    return fast - slow

def pct_change(closes):
    if len(closes) < 2: return 0.0
    prev = closes[-2]
    if prev == 0: return 0.0
    return (closes[-1]-prev)/prev*100

def long_signal(c):
    e50 = ema(c, 50); e200 = ema(c, 200)
    if e50 is None or e200 is None: return False
    r = rsi(c, 14); m = macd(c); pc = pct_change(c)
    return (c[-1] > e50 and c[-1] > e200 and 45 <= r <= 65 and m > 0 and pc > 0)

def short_signal(c):
    e50 = ema(c, 50); e200 = ema(c, 200)
    if e50 is None or e200 is None: return False
    r = rsi(c, 14); m = macd(c); pc = pct_change(c)
    return (c[-1] < e50 and c[-1] < e200 and 35 <= r <= 55 and m < 0 and pc < 0)

print(f"Fetching {COUNT} {SYMBOL} 1-min candles from Deriv...")
o,h,l,c = fetch_candles(SYMBOL, GRAN, COUNT)
print(f"Got {len(c)} candles. Range {c.min():.2f}-{c.max():.2f}")

WARM = 200
long_pred = short_pred = 0
long_win = short_win = 0
streak_max = 0
streak = 0
for i in range(WARM, len(c)-1):
    window = c[:i+1]
    nxt = c[i+1]
    rose = nxt > c[i]
    fell = nxt < c[i]
    L = long_signal(window)
    S = short_signal(window)
    if L:
        long_pred += 1
        if rose: long_win += 1; streak = streak+1 if streak>=0 else 1
        else: streak = 0
    elif S:
        short_pred += 1
        if fell: short_win += 1; streak = streak+1 if streak>=0 else 1
        else: streak = 0
    else:
        streak = 0
    if streak > streak_max: streak_max = streak

total = long_pred + short_pred
wins = long_win + short_win
print("\n=== RISE/FALL SIGNAL ACCURACY ===")
print(f"Symbol: {SYMBOL} | candles: {len(c)} | warmup: {WARM}")
print(f"Signals fired: {total} ({long_pred} rise-calls, {short_pred} fall-calls)")
print(f"Correct calls: {wins} ({long_win} rise, {short_win} fall)")
if total: print(f"Directional win rate: {wins/total*100:.1f}%")
if long_pred: print(f"  Rise-call accuracy:  {long_win/long_pred*100:.1f}%")
if short_pred: print(f"  Fall-call accuracy:  {short_win/short_pred*100:.1f}%")
print(f"Best correct-signal streak: {streak_max}")
print(f"Random baseline (50/50): ~50.0%")
