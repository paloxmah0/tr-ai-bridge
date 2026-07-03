import json, websocket, numpy as np

def fetch(symbol, gran, count):
    ws=websocket.create_connection("wss://ws.derivws.com/websockets/v3?app_id=1089",timeout=30)
    ws.send(json.dumps({"ticks_history":symbol,"end":"latest","style":"candles","granularity":gran,"count":count}))
    m=json.loads(ws.recv()); ws.close()
    if "error" in m: raise RuntimeError(m["error"])
    cols=m["candles"]
    return (np.array([x["open"] for x in cols],float),
            np.array([x["high"] for x in cols],float),
            np.array([x["low"] for x in cols],float),
            np.array([x["close"] for x in cols],float))

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
    sd=var**0.5; return m+2*sd, m, m-2*sd

# Candlestick patterns (matching app's detect_patterns logic, simplified to present/absent)
def hammer(o,h,l,c,i):
    if i<1: return False
    O,H,L,C=o[i],h[i],l[i],c[i]
    body=abs(C-O); rng=abs(H-L)
    if rng==0: return False
    low_wick=(min(O,C)-L); up_wick=(H-max(O,C))
    return low_wick>=body*2 and up_wick<=body*0.1 and body>0
def bull_engulf(o,h,l,c,i):
    if i<1: return False
    po,pc=o[i-1],c[i-1]; O,C=o[i],c[i]
    return pc<po and C>O and O<=pc and C>=po
def bear_engulf(o,h,l,c,i):
    if i<1: return False
    po,pc=o[i-1],c[i-1]; O,C=o[i],c[i]
    return pc>po and C<O and O>=pc and C<=po

WARM=210
def htf_up(c,i):  # higher-timeframe trend = ema(50)>ema(200) on 10min series
    e50=ema(c[:i+1],50); e200=ema(c[:i+1],200)
    if e50 is None or e200 is None: return None
    return 1 if e50>e200 else (-1 if e50<e200 else 0)

def sig_exhaust(o,h,l,c,i):
    t=htf_up(c,i)
    if i<3 or t is None: return None
    opens=o[i-2:i+1]; cls=c[i-2:i+1]
    three_down=all(cls[k]<opens[k] for k in range(3))
    three_up=all(cls[k]>opens[k] for k in range(3))
    if t==1 and three_down: return "rise"
    if t==-1 and three_up: return "fall"
    return None
def sig_rsi(o,h,l,c,i):
    t=htf_up(c,i)
    if t is None: return None
    r=rsi(c[:i+1])
    if t==1 and r<35: return "rise"
    if t==-1 and r>65: return "fall"
    return None
def sig_bb(o,h,l,c,i):
    t=htf_up(c,i)
    if t is None: return None
    up,md,lo=bb(c[:i+1])
    if up is None: return None
    if t==1 and c[i]<lo: return "rise"
    if t==-1 and c[i]>up: return "fall"
    return None
def sig_pat(o,h,l,c,i):
    t=htf_up(c,i)
    if t is None: return None
    if t==1 and (hammer(o,h,l,c,i) or bull_engulf(o,h,l,c,i)): return "rise"
    if t==-1 and bear_engulf(o,h,l,c,i): return "fall"
    return None
def sig_combo(o,h,l,c,i):
    # any bullish pattern in uptrend -> rise; any bearish in downtrend -> fall
    t=htf_up(c,i)
    if t is None: return None
    r=rsi(c[:i+1])
    bull = hammer(o,h,l,c,i) or bull_engulf(o,h,l,c,i) or (r<35)
    bear = bear_engulf(o,h,l,c,i) or (r>65)
    if t==1 and bull: return "rise"
    if t==-1 and bear: return "fall"
    return None

VS={"MTF+exhaustion":sig_exhaust,"MTF+RSI extreme":sig_rsi,"MTF+BB pierce":sig_bb,
    "MTF+patterns(hammer/engulf)":sig_pat,"MTF+combo(pat|RSI)":sig_combo}

print("Fetching R_75 10-min candles...")
o,h,l,c=fetch("R_75",600,5000)
print(f"Got {len(c)} candles. price={c[-1]:.0f}\n")
print(f"{'variant':<26}{'H':<4}{'sig':<6}{'win':<6}{'rate':<7}{'rise%':<7}{'fall%':<7}{'streak'}")
for vn,fn in VS.items():
    for HZ in [1,3,5]:
        fired=win=0; rn=fn_=rw=fw=0; best=cur=0
        for i in range(WARM,len(c)-HZ):
            s=fn(o,h,l,c,i)
            if s is None: continue
            fut=c[i+HZ]; up=fut>c[i]; dn=fut<c[i]
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
            print(f"{vn:<26}{HZ:<4}{fired:<6}{win:<6}{r:<7.1f}{rpw:<7}{fpw:<7}{best}")
print("\nBreak-even @80% payout: 55.5%  |  random: 50%")
