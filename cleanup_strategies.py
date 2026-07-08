"""Run this AFTER backend restart to ensure only GBPUSD RSI Reversal is enabled.
All other strategies get disabled to prevent signal pollution."""
import json, urllib.request

APP = "http://localhost:8080"
KEEP = "GBPUSD RSI Reversal"

def get(path):
    try:
        with urllib.request.urlopen(f"{APP}{path}", timeout=10) as r:
            return json.loads(r.read())
    except: return None

def put(path, body):
    data = json.dumps(body).encode()
    req = urllib.request.Request(f"{APP}{path}", data=data, headers={"Content-Type":"application/json"}, method="PUT")
    try:
        with urllib.request.urlopen(req, timeout=10) as r: return json.loads(r.read())
    except: return None

print("Cleaning up strategies...")
accs = get("/api/accounts") or []
disabled = 0
kept = 0
for a in accs:
    strats = get(f"/api/accounts/{a['id']}/strategies") or []
    for s in strats:
        if s["name"] != KEEP and s["enabled"]:
            put(f"/api/strategies/{s['id']}", {"enabled": False})
            print(f"  DISABLED: {s['name']} | {s['symbols']}")
            disabled += 1
        elif s["name"] == KEEP:
            print(f"  KEPT: {s['name']} | {s['symbols']} | SL={s['stop_loss']} TP={s['take_profit']}")
            kept += 1

print(f"\nDone: {kept} kept, {disabled} disabled.")
if kept == 0:
    print("WARNING: GBPUSD RSI Reversal not found! Check the database.")
elif kept > 1:
    print("WARNING: Multiple RSI Reversal strategies found!")
else:
    print("OK: Only GBPUSD RSI Reversal is enabled. Safe to trade.")
