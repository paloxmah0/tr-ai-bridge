import json, websocket, numpy as np

SYMBOL="R_100"; GRAN=60; COUNT=5000; WARM=200

def fetch(symbol, gran, count):
    ws=websocket.create_connection("wss://ws.derivws.com/websockets/v3?app_id=1089",timeout=30)
    ws.send(json.dumps({"ticks_history":symbol,"end":"latest","style":"candles","granularity":gran,"count":count}))
    msg=json.loads(ws.recv()); ws.close()
    if "error" in msg: raise RuntimeError(msg["error"])
    cols=msg["candles"]
    return np.array([c["close"] for c in cols],float)

def ema(c,p):
    if len(c)<p: return None
    k=2.0/(p+1); e=c[:p].mean()
    for i in range(p,len(c)): e=c[i]*k+e*(1-k)
    return e
def rsi(c,p=14):
    if len(c)<=p: return 50.0
    w=c[len(c)-p-1:]; d=np.diff(w)
    g=np.where(d>0,d,0.0).sum(); l=np.where(d<0,-d,0.0).sum()
    ag=g/p; al=l/p
    if al==0: return 100.0
    return 100-100/(1+ag/al)
def macd(c):
    f=ema(c,12); s=ema(c,26)
    return (f-s) if (f is not None and s is not None) else 0.0
def pc(c):
    return 0.0 if len(c)<2 or c[-2]==0 else (c[-1]-c[-2])/c[-2]*100

def sig(c):
    e50=ema(c,50); e200=ema(c,200)
    if e50 is None or e200 is None: return None
    r=rsi(c,14); m=macd(c); p=pc(c)
    if c[-1]>e50 and c[-1]>e200 and 45<=r<=65 and m>0 and p>0: return "rise"
    if c[-1]<e50 and c[-1]<e200 and 35<=r<=55 and m<0 and p<0: return "fall"
    return None

print(f"Fetching {COUNT} {SYMBOL} 1-min candles...")
c=fetch(SYMBOL,GRAN,COUNT)
print(f"Got {len(c)} candles.\n")

for HORIZON in [1,3,5,10]:
    fired=0; win=0; rise_n=fall_n=0; rise_w=fall_w=0; best=0; cur=0
    for i in range(WARM, len(c)-HORIZON):
        s=sig(c[:i+1])
        if s is None: continue
        fut=c[i+HORIZON]
        up=fut>c[i]; dn=fut<c[i]
        fired+=1
        if s=="rise":
            rise_n+=1
            if up: win+=1; rise_w+=1; cur=cur+1 if cur>=0 else 1
            else: cur=0
        else:
            fall_n+=1
            if dn: win+=1; fall_w+=1; cur=cur+1 if cur<=0 else -1
            else: cur=0
        if abs(cur)>best: best=abs(cur)
    print(f"=== Horizon {HORIZON} candle(s) ahead ===")
    print(f"  signals={fired}  correct={win}  winrate={win/fired*100:.1f}%" if fired else "  no signals")
    if rise_n: print(f"  rise: {rise_n} calls, {rise_w} correct ({rise_w/rise_n*100:.1f}%)")
    if fall_n: print(f"  fall: {fall_n} calls, {fall_w} correct ({fall_w/fall_n*100:.1f}%)")
    print(f"  best correct streak: {best}\n")
