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

def ema_series(c,p):
    out=np.full(len(c),np.nan)
    if len(c)<p: return out
    k=2.0/(p+1); out[p-1]=c[:p].mean()
    for i in range(p,len(c)): out[i]=c[i]*k+out[i-1]*(1-k)
    return out
def rsi_arr(c,p=14):
    out=np.full(len(c),50.0)
    for i in range(p,len(c)):
        w=c[i-p:i+1]; d=np.diff(w)
        g=np.where(d>0,d,0.0).sum(); l=np.where(d<0,-d,0.0).sum()
        ag=g/p; al=l/p
        out[i]=100.0 if al==0 else 100-100/(1+ag/al)
    return out
def bb_lower(c,p=20):
    out=np.full(len(c),np.nan)
    for i in range(p-1,len(c)):
        s=c[i-p+1:i+1]; m=s.mean(); var=((s-m)**2).mean()
        out[i]=m-2*var**0.5
    return out
def bb_upper(c,p=20):
    out=np.full(len(c),np.nan)
    for i in range(p-1,len(c)):
        s=c[i-p+1:i+1]; m=s.mean(); var=((s-m)**2).mean()
        out[i]=m+2*var**0.5
    return out
def hammer(o,h,l,c,i):
    if i<1: return False
    O,H,L,C=o[i],h[i],l[i],c[i]
    body=abs(C-O); rng=abs(H-L)
    if rng==0: return False
    return (min(O,C)-L)>=body*2 and (H-max(O,C))<=body*0.1 and body>0
def bull_engulf(o,h,l,c,i):
    if i<1: return False
    return c[i-1]<o[i-1] and c[i]>o[i] and o[i]<=c[i-1] and c[i]>=o[i-1]
def bear_engulf(o,h,l,c,i):
    if i<1: return False
    return c[i-1]>o[i-1] and c[i]<o[i] and o[i]>=c[i-1] and c[i]<=o[i-1]

print("Fetching R_75 10-min candles...")
o,h,l,c=fetch("R_75",600,5000)
n=len(c)
e50=ema_series(c,50); e200=ema_series(c,200)
rsi=rsi_arr(c,14); blo=bb_lower(c); bup=bb_upper(c)
WARM=210

def htf(i):
    if np.isnan(e50[i]) or np.isnan(e200[i]): return None
    return 1 if e50[i]>e200[i] else -1

def sig_exhaust(i):
    t=htf(i)
    if t is None or i<3: return None
    opens=o[i-2:i+1]; cls=c[i-2:i+1]
    td=all(cls[k]<opens[k] for k in range(3)); tu=all(cls[k]>opens[k] for k in range(3))
    if t==1 and td: return "rise"
    if t==-1 and tu: return "fall"
    return None
def sig_rsi(i):
    t=htf(i)
    if t is None: return None
    if t==1 and rsi[i]<35: return "rise"
    if t==-1 and rsi[i]>65: return "fall"
    return None
def sig_bb(i):
    t=htf(i)
    if t is None or np.isnan(blo[i]): return None
    if t==1 and c[i]<blo[i]: return "rise"
    if t==-1 and c[i]>bup[i]: return "fall"
    return None
def sig_pat(i):
    t=htf(i)
    if t is None: return None
    if t==1 and (hammer(o,h,l,c,i) or bull_engulf(o,h,l,c,i)): return "rise"
    if t==-1 and bear_engulf(o,h,l,c,i): return "fall"
    return None
def sig_combo(i):
    t=htf(i)
    if t is None: return None
    bull=hammer(o,h,l,c,i) or bull_engulf(o,h,l,c,i) or rsi[i]<35
    bear=bear_engulf(o,h,l,c,i) or rsi[i]>65
    if t==1 and bull: return "rise"
    if t==-1 and bear: return "fall"
    return None

VS={"MTF+exhaustion":sig_exhaust,"MTF+RSI extreme":sig_rsi,"MTF+BB pierce":sig_bb,
    "MTF+patterns":sig_pat,"MTF+combo(pat|RSI)":sig_combo}

print(f"Got {n} candles. price={c[-1]:.0f}\n")
print(f"{'variant':<22}{'H':<4}{'sig':<6}{'win':<6}{'rate':<7}{'rise%':<7}{'fall%':<7}{'streak'}")
for vn,fn in VS.items():
    for HZ in [1,3,5]:
        fired=win=0; rn=fn_=rw=fw=0; best=cur=0
        for i in range(WARM,n-HZ):
            s=fn(i)
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
            print(f"{vn:<22}{HZ:<4}{fired:<6}{win:<6}{r:<7.1f}{rpw:<7}{fpw:<7}{best}")
print("\nBreak-even @80% payout: 55.5%  |  random: 50%")
