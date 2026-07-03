import json, websocket, numpy as np

SYMBOL="R_75"
GRANS={"5min":300, "10min":600}
COUNT=5000
WARM=200

def fetch(symbol, gran, count):
    ws=websocket.create_connection("wss://ws.derivws.com/websockets/v3?app_id=1089",timeout=30)
    ws.send(json.dumps({"ticks_history":symbol,"end":"latest","style":"candles","granularity":gran,"count":count}))
    msg=json.loads(ws.recv()); ws.close()
    if "error" in msg: raise RuntimeError(msg["error"])
    cols=msg["candles"]
    o=np.array([x["open"] for x in cols],float)
    c=np.array([x["close"] for x in cols],float)
    return o,c

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
    return 100.0 if al==0 else 100-100/(1+ag/al)
def bb(c,p=20):
    if len(c)<p: return None,None,None
    s=c[-p:]; m=s.mean(); var=((s-m)**2).mean()
    sd=var**0.5
    return m+2*sd, m, m-2*sd
def pc(c):
    return 0.0 if len(c)<2 or c[-2]==0 else (c[-1]-c[-2])/c[-2]*100

# Mean-reversion rule variants. Each returns "rise"/"fall"/None at bar i.
def v_rsi(o,c,i):
    r=rsi(c[:i+1])
    if r<25: return "rise"
    if r>75: return "fall"
    return None
def v_bb(o,c,i):
    up,md,lo=bb(c[:i+1])
    if up is None: return None
    if c[i]<lo: return "rise"
    if c[i]>up: return "fall"
    return None
def v_exhaust(o,c,i):
    if i<3: return None
    last3=o[i-2:i+1],c[i-2:i+1]
    opens=o[i-2:i+1]; cls=c[i-2:i+1]
    if all(cls[k]<opens[k] for k in range(3)): return "rise"  # 3 down -> snap up
    if all(cls[k]>opens[k] for k in range(3)): return "fall"
    return None
def v_bigmove(o,c,i):
    p=pc(c[:i+1])
    if p<-0.4: return "rise"
    if p>0.4: return "fall"
    return None
def v_combo(o,c,i):
    # RSI extreme OR (exhaustion AND big move)
    r=rsi(c[:i+1]); p=pc(c[:i+1])
    opens=o[i-2:i+1] if i>=2 else np.array([]); cls=c[i-2:i+1] if i>=2 else np.array([])
    ex_down = len(cls)==3 and all(cls[k]<opens[k] for k in range(3))
    ex_up   = len(cls)==3 and all(cls[k]>opens[k] for k in range(3))
    if r<25 or (ex_down and p<-0.2): return "rise"
    if r>75 or (ex_up and p>0.2): return "fall"
    return None

VARIANTS={"RSI extreme":v_rsi, "Bollinger pierce":v_bb, "3-candle exhaustion":v_exhaust,
          "Big move reversal":v_bigmove, "Combo (RSI|exhaust+move)":v_combo}

def run(o,c,label):
    print(f"\n##### {SYMBOL} @ {label}  ({len(c)} candles) #####")
    print(f"{'variant':<28}{'horiz':<6}{'sig':<6}{'win':<6}{'rate':<7}{'rise%':<7}{'fall%':<7}{'streak'}")
    for vname,fn in VARIANTS.items():
        for H in [1,3,5]:
            fired=win=0; rn=fn_=rw=fw=0; best=cur=0
            for i in range(WARM, len(c)-H):
                s=fn(o,c,i)
                if s is None: continue
                fut=c[i+H]; up=fut>c[i]; dn=fut<c[i]
                fired+=1
                if s=="rise":
                    rn+=1
                    if up: win+=1; rw+=1; cur=cur+1 if cur>=0 else 1
                    else: cur=0
                else:
                    fn_+=1
                    if dn: win+=1; fw+=1; cur=cur+1 if cur<=0 else -1
                    else: cur=0
                if abs(cur)>best: best=abs(cur)
            if fired:
                r=win/fired*100
                rpw=f"{rw/rn*100:.0f}%" if rn else "-"
                fpw=f"{fw/fn_*100:.0f}%" if fn_ else "-"
                print(f"{vname:<28}{H:<6}{fired:<6}{win:<6}{r:<7.1f}{rpw:<7}{fpw:<7}{best}")
            else:
                print(f"{vname:<28}{H:<6}{'-':<6}{'-':<6}{'-':<7}{'-':<7}{'-':<7}-")

print("Fetching R_75 candles from Deriv...")
data={}
for gname,gs in GRANS.items():
    o,c=fetch(SYMBOL,gs,COUNT); data[gname]=(o,c)
    print(f"  {gname}: {len(c)} candles")
for gname,(o,c) in data.items():
    run(o,c,gname)
print("\nBreak-even for Deriv Rise/Fall @~80% payout: ~55.5%  |  random: 50%")
