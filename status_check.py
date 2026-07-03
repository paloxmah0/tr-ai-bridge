"""Quick status check - run this at the start of a new session to see the current state."""
import json, urllib.request, datetime, sys

APP = "http://localhost:8080"

def get(path):
    try:
        with urllib.request.urlopen(f"{APP}{path}", timeout=10) as r:
            return json.loads(r.read())
    except Exception as e:
        return None

def mt5_status():
    try:
        import MetaTrader5 as mt5
        if not mt5.initialize():
            return "MT5 NOT connected"
        a = mt5.account_info()
        pos = mt5.positions_get(symbol="EURUSD")
        deals = mt5.history_deals_get(
            datetime.datetime.now() - datetime.timedelta(hours=24),
            datetime.datetime.now())
        our_deals = [d for d in (deals or []) if d.magic == 20260703] if deals else []
        wins = sum(1 for d in our_deals if d.entry == 1 and d.profit > 0)
        losses = sum(1 for d in our_deals if d.entry == 1 and d.profit <= 0)
        total_pnl = sum(d.profit for d in our_deals if d.entry == 1)
        mt5.shutdown()
        return f"MT5: {a.login} | balance={a.balance:.2f} {a.currency} | open={len(pos) if pos else 0} | 24h trades={len(our_deals)} (W{wins}/L{losses}) pnl={total_pnl:.2f}"
    except ImportError:
        return "MT5: MetaTrader5 package not installed"
    except Exception as e:
        return f"MT5: error - {e}"

print("=" * 60)
print("  TRADING SYSTEM STATUS - " + datetime.datetime.now().strftime("%Y-%m-%d %H:%M"))
print("=" * 60)

# Backend
accs = get("/api/accounts")
if accs is None:
    print("BACKEND: NOT running (start: ./target/release/trading-backend.exe)")
    sys.exit(1)
print(f"BACKEND: running on :8080 | {len(accs)} account(s)")
for a in accs:
    print(f"  - {a['label']} | balance={a.get('balance','?')} | mode={a.get('mode','?')}")

# Latest analysis
analysis = get("/api/analyze") if False else None
# Note: analyze is POST, skip auto-call. Just show last signals.
if accs:
    sigs = get(f"/api/accounts/{accs[0]['id']}/signals") or []
    print(f"\nSIGNALS: {len(sigs)} total (last 5):")
    for s in sigs[:5]:
        print(f"  {s.get('created_at','?')} | {s.get('symbol','?')} | {s.get('side','?')} | str={s.get('strength','?')}")

# MT5
print(f"\n{mt5_status()}")

# Bridge
import os
bridge_log = r"C:\Users\san\AppData\Local\Temp\opencode\bridge3.out"
if os.path.exists(bridge_log):
    with open(bridge_log) as f:
        lines = f.readlines()[-5:]
    print(f"\nBRIDGE LOG (last 5 lines):")
    for l in lines:
        print(f"  {l.rstrip()}")
else:
    print("\nBRIDGE: log not found (may not be running)")

print("\n" + "=" * 60)
print("  To recreate: see AGENTS.md")
print("=" * 60)
