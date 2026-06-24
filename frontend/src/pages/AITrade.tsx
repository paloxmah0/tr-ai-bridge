import { useState, useEffect, useRef } from "react";
import { api } from "../lib/api";
import type { Prediction, Evidence, Account } from "../lib/api";
import { fmt, fmtPct } from "../lib/fmt";
import { Brain, Loader2, TrendingUp, TrendingDown, Minus, Zap, Clock, Globe, Target, Activity, BarChart3, Eye, Layers, Wallet } from "lucide-react";

// TradingView symbol mapping
const TV_SYMBOLS: Record<string, string> = {
  "R_100": "DERIV:VOLATILITY100",
  "R_75": "DERIV:VOLATILITY75",
  "R_50": "DERIV:VOLATILITY50",
  "R_25": "DERIV:VOLATILITY25",
  "frxEURUSD": "FX:EURUSD",
  "frxGBPUSD": "FX:GBPUSD",
  "frxUSDJPY": "FX:USDJPY",
  "frxAUDUSD": "FX:AUDUSD",
};

const MARKETS = [
  { symbol: "R_100", label: "Volatility 100 Index", class: "derivindex" },
  { symbol: "R_75", label: "Volatility 75 Index", class: "derivindex" },
  { symbol: "R_50", label: "Volatility 50 Index", class: "derivindex" },
  { symbol: "R_25", label: "Volatility 25 Index", class: "derivindex" },
  { symbol: "frxEURUSD", label: "EUR/USD", class: "forex" },
  { symbol: "frxGBPUSD", label: "GBP/USD", class: "forex" },
  { symbol: "frxUSDJPY", label: "USD/JPY", class: "forex" },
  { symbol: "frxAUDUSD", label: "AUD/USD", class: "forex" },
];

const TIMEFRAMES = [
  { mins: 1, label: "1 min" }, { mins: 5, label: "5 min" }, { mins: 10, label: "10 min" },
  { mins: 15, label: "15 min" }, { mins: 30, label: "30 min" }, { mins: 60, label: "1 hour" },
];

function fmtUTC(s: string): string {
  if (!s) return "—";
  return new Date(s).toISOString().replace("T", " ").slice(0, 19) + " UTC";
}

const STATE_LABELS: Record<string, string> = {
  trending_up: "Trending Up", trending_down: "Trending Down",
  ranging: "Ranging", reversing_up: "Reversing Up", reversing_down: "Reversing Down",
  squeeze: "Volatility Squeeze", mixed: "Mixed Signals",
};

export default function AITrade() {
  const [symbol, setSymbol] = useState("R_100");
  const [timeframe, setTimeframe] = useState(15);
  const [analyzing, setAnalyzing] = useState(false);
  const [prediction, setPrediction] = useState<Prediction | null>(null);
  const [trading, setTrading] = useState(false);
  const [tradeResult, setTradeResult] = useState<any>(null);
  const [error, setError] = useState("");
  const [account, setAccount] = useState<Account | null>(null);
  const [stake, setStake] = useState("10");
  const [recentTrades, setRecentTrades] = useState<any[]>([]);

  // Auto-load account on mount
  useEffect(() => {
    api.listAccounts().then(accs => {
      if (accs.length > 0) {
        setAccount(accs[0]);
        api.listTrades(accs[0].id).then(trades => {
          setRecentTrades(trades.slice(0, 5));
        }).catch(() => {});
      }
    }).catch(() => {});
  }, []);

  async function runAnalysis() {
    setAnalyzing(true); setError(""); setPrediction(null); setTradeResult(null);
    const m = MARKETS.find(m => m.symbol === symbol);
    try { setPrediction(await api.analyze(symbol, timeframe, m?.class)); }
    catch (e: any) { setError(e.message); } finally { setAnalyzing(false); }
  }

  async function placeTrade() {
    if (!prediction || prediction.direction === "wait") return;
    setTrading(true); setError(""); setTradeResult(null);
    try {
      const m = MARKETS.find(m => m.symbol === symbol);
      const r = await api.placeTrade(symbol, prediction.direction, timeframe, Number(stake) || 10, m?.class);
      setTradeResult(r);
      // Refresh trades
      if (account) {
        api.listTrades(account.id).then(trades => setRecentTrades(trades.slice(0, 5))).catch(() => {});
      }
    } catch (e: any) { setError(e.message); } finally { setTrading(false); }
  }

  const DirIcon = prediction?.direction === "buy" ? TrendingUp : prediction?.direction === "sell" ? TrendingDown : Minus;
  const dirColor = prediction?.direction === "buy" ? "text-ok" : prediction?.direction === "sell" ? "text-bad" : "text-muted";
  const dirBg = prediction?.direction === "buy" ? "bg-ok/10 border-ok/30" : prediction?.direction === "sell" ? "bg-bad/10 border-bad/30" : "bg-ink-800 border-ink-700";
  const bullCount = prediction?.evidence.filter(e => e.confirms === "buy" && e.weight > 0).length || 0;
  const bearCount = prediction?.evidence.filter(e => e.confirms === "sell" && e.weight > 0).length || 0;

  return (
    <div>
      <div className="mb-6">
        <h2 className="text-xl font-bold text-white flex items-center gap-2"><Brain size={22} className="text-accent" /> AI Market Reader</h2>
        <p className="text-sm text-muted">Reads the market with tools + candlestick knowledge. Evidence-based, not prediction.</p>
      </div>

      {error && <div className="card border-bad/50 text-bad text-sm mb-4">{error}</div>}

      {/* Account info bar */}
      {account && (
        <div className="card mb-4 flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-1.5"><Wallet size={14} className="text-accent" /><span className="text-sm text-gray-200">{account.label}</span></div>
            <span className={`badge ${account.mode === "live" ? "bg-bad/20 text-bad" : account.mode === "signals" ? "bg-accent/20 text-accent" : "bg-warn/20 text-warn"}`}>{account.mode}</span>
          </div>
          <div className="text-sm">
            <span className="text-muted">Balance: </span>
            <span className="font-bold text-white">{fmt(account.balance)} {account.currency}</span>
          </div>
        </div>
      )}

      {/* Trade result */}
      {tradeResult && (
        <div className="card border-ok/50 mb-4">
          <div className="flex items-center gap-2 mb-2">
            <Zap size={16} className="text-ok" />
            <span className="font-bold text-ok">Trade Placed!</span>
            <span className={`badge ${tradeResult.mode === "live" ? "bg-bad/20 text-bad" : "bg-warn/20 text-warn"}`}>{tradeResult.mode?.toUpperCase()}</span>
          </div>
          <div className="grid grid-cols-4 gap-3 text-sm">
            <div><span className="text-muted">Direction:</span> <span className={`font-bold ${tradeResult.direction === "buy" ? "text-ok" : "text-bad"}`}>{tradeResult.direction?.toUpperCase()}</span></div>
            <div><span className="text-muted">Symbol:</span> <span className="text-gray-200">{tradeResult.symbol}</span></div>
            <div><span className="text-muted">Entry:</span> <span className="font-mono text-gray-200">{tradeResult.entry_price}</span></div>
            <div><span className="text-muted">Stake:</span> <span className="font-mono text-gray-200">{tradeResult.stake}</span></div>
            <div><span className="text-muted">Stop:</span> <span className="font-mono text-bad">{tradeResult.stop_loss}</span></div>
            <div><span className="text-muted">Target:</span> <span className="font-mono text-ok">{tradeResult.take_profit}</span></div>
            {tradeResult.broker_ref && <div><span className="text-muted">Broker Ref:</span> <span className="font-mono text-accent text-xs">{tradeResult.broker_ref}</span></div>}
            {tradeResult.balance_after && <div><span className="text-muted">Balance After:</span> <span className="font-mono text-gray-200">{tradeResult.balance_after}</span></div>}
          </div>
          <p className="text-xs text-muted mt-2">{tradeResult.message}</p>
        </div>
      )}

      {/* TradingView Live Chart */}
      <TradingViewChart symbol={TV_SYMBOLS[symbol] || "DERIV:VOLATILITY100"} timeframe={timeframe} />

      {/* Controls */}
      <div className="card mb-6">
        <div className="grid grid-cols-2 gap-4">
          <div>
            <div className="label mb-2"><Globe size={12} className="inline mr-1" />Market</div>
            <select className="input w-full" value={symbol} onChange={e => { setSymbol(e.target.value); setPrediction(null); }}>
              {MARKETS.map(m => <option key={m.symbol} value={m.symbol}>{m.label}</option>)}
            </select>
          </div>
          <div>
            <div className="label mb-2"><Clock size={12} className="inline mr-1" />Timeframe</div>
            <div className="flex flex-wrap gap-1">
              {TIMEFRAMES.map(tf => (
                <button key={tf.mins} onClick={() => { setTimeframe(tf.mins); setPrediction(null); }}
                  className={`btn text-xs ${timeframe === tf.mins ? "bg-accent-dim text-white" : "bg-ink-700 text-gray-400 hover:bg-ink-600"}`}>
                  {tf.label}
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="grid grid-cols-2 gap-4 mt-3">
          <div>
            <div className="label mb-1">Stake (USD)</div>
            <input className="input w-full" type="number" value={stake} onChange={e => setStake(e.target.value)} />
          </div>
          <div className="flex items-end">
            <button onClick={runAnalysis} disabled={analyzing} className="btn-primary w-full text-base py-2.5">
              {analyzing ? <><Loader2 size={18} className="inline mr-2 animate-spin" />Reading market…</> : <><Brain size={18} className="inline mr-2" />Read Market</>}
            </button>
          </div>
        </div>
      </div>

      {prediction && (
        <div className="space-y-4">
          {/* Header */}
          <div className="card flex items-center justify-between text-sm">
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-1.5 text-muted"><Clock size={14} /><span className="font-mono text-xs">{fmtUTC(prediction.analysis_time_utc)}</span></div>
              <div className="flex items-center gap-1.5 text-accent"><Globe size={14} />{prediction.market_session}</div>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted">Next candle:</span>
              <span className="font-mono text-sm font-bold text-warn bg-warn/10 px-2 py-0.5 rounded">{prediction.countdown}</span>
            </div>
          </div>

          {/* THE READING — bold and big */}
          <div className={`card border-2 ${dirBg} py-8`}>
            <div className="text-center">
              <div className="label mb-3 text-sm">Current Market State</div>
              <div className="text-2xl font-bold text-white mb-4">{STATE_LABELS[prediction.market_state] || prediction.market_state}</div>
              <div className="flex items-center justify-center gap-4 mb-4">
                <DirIcon size={48} className={dirColor} />
                <span className={`text-4xl font-bold ${dirColor} uppercase`}>{prediction.direction === "wait" ? "WAIT" : prediction.direction}</span>
              </div>
              <div className={`text-6xl font-bold ${dirColor} mb-2`}>{fmtPct(prediction.evidence_score)}</div>
              <div className="text-sm text-muted">evidence score</div>
              <div className="flex items-center justify-center gap-8 mt-4 text-lg">
                <span className="text-ok font-bold">{bullCount} BUY</span>
                <span className="text-muted text-sm">vs</span>
                <span className="text-bad font-bold">{bearCount} SELL</span>
              </div>
            </div>
          </div>

          {/* Trade levels + trade button */}
          {prediction.direction !== "wait" && (
            <>
              <div className="grid grid-cols-3 gap-3">
                <div className="card text-center"><div className="label">Entry</div><div className="text-lg font-mono text-gray-200">{fmt(prediction.entry_price, 5)}</div></div>
                <div className="card text-center"><div className="label">Stop Loss</div><div className="text-lg font-mono text-bad">{fmt(prediction.stop_loss, 5)}</div></div>
                <div className="card text-center"><div className="label">Take Profit</div><div className="text-lg font-mono text-ok">{fmt(prediction.take_profit, 5)}</div></div>
              </div>
              <button onClick={placeTrade} disabled={trading} className={`btn w-full text-lg py-4 font-bold ${prediction.direction === "buy" ? "bg-ok text-white hover:bg-ok/80" : "bg-bad text-white hover:bg-bad/80"}`}>
                {trading ? <><Loader2 size={20} className="inline mr-2 animate-spin" />Placing trade…</> : <><Zap size={20} className="inline mr-2" />Place {prediction.direction.toUpperCase()} — {stake} USD on {prediction.symbol}</>}
              </button>
            </>
          )}
          {prediction.direction === "wait" && (
            <div className="card text-center text-muted py-8"><Minus size={32} className="inline mb-3" /><p className="text-lg">Evidence inconclusive. WAIT for clearer signals.</p></div>
          )}

          {/* Evidence */}
          <div className="card">
            <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><BarChart3 size={15} className="text-accent" /> Evidence ({prediction.evidence.length} readings)</h3>
            <div className="space-y-1">
              {prediction.evidence.map((e, i) => <EvidenceRow key={i} evidence={e} />)}
            </div>
          </div>

          {/* Recent candles */}
          {prediction.recent_candles && prediction.recent_candles.length > 0 && (
            <div className="card">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><Activity size={15} className="text-accent" /> Last {prediction.recent_candles.length} Candles</h3>
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead><tr className="text-left text-muted border-b border-ink-700">
                    <th className="py-1 px-2">#</th><th className="px-2">Dir</th><th className="px-2">Open</th><th className="px-2">High</th><th className="px-2">Low</th><th className="px-2">Close</th><th className="px-2">Pattern</th>
                  </tr></thead>
                  <tbody>
                    {prediction.recent_candles.map((c, i) => (
                      <tr key={i} className="border-b border-ink-700/40">
                        <td className="py-1 px-2 text-muted">-{prediction.recent_candles.length - i}</td>
                        <td className="px-2"><span className={c.direction === "bullish" ? "text-ok" : c.direction === "bearish" ? "text-bad" : "text-muted"}>{c.direction}</span></td>
                        <td className="px-2 font-mono text-gray-400">{fmt(c.open, 5)}</td>
                        <td className="px-2 font-mono text-gray-400">{fmt(c.high, 5)}</td>
                        <td className="px-2 font-mono text-gray-400">{fmt(c.low, 5)}</td>
                        <td className="px-2 font-mono text-gray-400">{fmt(c.close, 5)}</td>
                        <td className="px-2 text-accent">{c.pattern}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Upper TF */}
          {prediction.upper_timeframe_context && prediction.upper_timeframe_context.length > 0 && (
            <div className="card">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><Layers size={15} className="text-accent" /> Upper Timeframe State</h3>
              <div className="space-y-2">
                {prediction.upper_timeframe_context.map((u, i) => (
                  <div key={i} className="flex items-center gap-3 bg-ink-900 rounded px-3 py-2">
                    <span className="font-bold text-white text-sm w-12">{u.label}</span>
                    <span className={u.trend === "bullish" ? "text-ok text-sm" : "text-bad text-sm"}>{u.trend}</span>
                    <span className="text-xs text-muted">RSI {String(u.rsi)} | ADX {String(u.adx)}</span>
                    <span className="text-xs text-accent ml-auto">{u.summary.split("—")[1]?.trim() || ""}</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* News impact */}
          {prediction.news && (
            <div className={`card ${prediction.news.status === "danger" ? "border-bad/40" : prediction.news.status === "caution" ? "border-warn/30" : "border-ink-700"}`}>
              <h3 className="text-sm font-semibold text-white mb-2 flex items-center gap-2">
                {prediction.news.status === "danger" && <span className="badge bg-bad/20 text-bad">NEWS ALERT</span>}
                {prediction.news.status === "caution" && <span className="badge bg-warn/20 text-warn">NEWS CAUTION</span>}
                {prediction.news.status === "clear" && <span className="badge bg-ok/20 text-ok">NO NEWS RISK</span>}
              </h3>
              <p className="text-sm text-gray-300 mb-2">{prediction.news.summary}</p>
              <p className="text-xs text-muted">{prediction.news.recommendation}</p>
              {prediction.news.upcoming_high_impact.length > 0 && (
                <div className="mt-2 space-y-1">
                  {prediction.news.upcoming_high_impact.map((e, i) => (
                    <div key={i} className="text-xs bg-bad/5 rounded px-2 py-1 border border-bad/20">
                      <span className="badge bg-bad/20 text-bad mr-2">{e.country}</span>
                      <span className="text-gray-300">{e.title}</span>
                      {e.forecast && <span className="text-muted ml-2">Forecast: {e.forecast}</span>}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* What to watch */}
          {prediction.what_to_watch && prediction.what_to_watch.length > 0 && (
            <div className="card border-warn/20">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><Eye size={15} className="text-warn" /> What to Watch</h3>
              <ul className="space-y-1.5">
                {prediction.what_to_watch.map((w, i) => (
                  <li key={i} className="text-sm text-gray-300 flex items-start gap-2"><span className="text-warn mt-0.5">▸</span> {w}</li>
                ))}
              </ul>
            </div>
          )}

          {/* Full report */}
          <div className="card">
            <h3 className="text-sm font-semibold text-white mb-2 flex items-center gap-2"><Target size={15} className="text-accent" /> Full Reading Report</h3>
            <pre className="text-xs text-gray-300 whitespace-pre-wrap font-mono leading-relaxed">{prediction.reasoning}</pre>
          </div>

          {/* Recent trades */}
          {recentTrades.length > 0 && (
            <div className="card">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><Activity size={15} className="text-accent" /> Recent Trades</h3>
              <div className="space-y-1">
                {recentTrades.map((t, i) => (
                  <div key={i} className="flex items-center gap-3 bg-ink-900 rounded px-3 py-2 text-sm">
                    <span className={t.side === "buy" ? "text-ok" : "text-bad"}>{t.side?.toUpperCase()}</span>
                    <span className="text-gray-200">{t.symbol}</span>
                    <span className="font-mono text-gray-400">@ {String(t.entry_price)?.slice(0, 10)}</span>
                    {t.pnl != null && <span className={`font-mono ml-auto ${Number(t.pnl) > 0 ? "text-ok" : "text-bad"}`}>{Number(t.pnl) > 0 ? "+" : ""}{String(t.pnl)?.slice(0, 8)}</span>}
                    <span className="text-xs text-muted">{t.status}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function EvidenceRow({ evidence }: { evidence: Evidence }) {
  const color = evidence.confirms === "buy" ? "text-ok" : evidence.confirms === "sell" ? "text-bad" : "text-muted";
  const bg = evidence.confirms === "buy" ? "bg-ok/5" : evidence.confirms === "sell" ? "bg-bad/5" : "bg-ink-900";
  const Icon = evidence.source.includes("candlestick") ? Activity : evidence.source.includes("note") ? Brain : evidence.source.includes("upper") ? Layers : evidence.source.includes("reversal") ? Eye : BarChart3;
  return (
    <div className={`flex items-start gap-3 rounded px-3 py-2 ${bg}`}>
      <Icon size={14} className="text-muted shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <span className="text-sm text-gray-300">{evidence.finding}</span>
      </div>
      <span className={`text-xs font-bold shrink-0 mt-0.5 ${color}`}>
        {evidence.confirms === "buy" ? "-> BUY" : evidence.confirms === "sell" ? "-> SELL" : ""}
      </span>
    </div>
  );
}

function TradingViewChart({ symbol, timeframe }: { symbol: string; timeframe: number }) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    containerRef.current.innerHTML = "";

    const tvInterval = timeframe <= 1 ? "1" :
      timeframe <= 5 ? "5" :
      timeframe <= 15 ? "15" :
      timeframe <= 30 ? "30" :
      timeframe <= 60 ? "60" :
      timeframe <= 240 ? "240" : "D";

    const script = document.createElement("script");
    script.src = "https://s3.tradingview.com/external-embedding/embed-widget-advanced-chart.js";
    script.async = true;
    script.innerHTML = JSON.stringify({
      autosize: true,
      symbol: symbol,
      interval: tvInterval,
      timezone: "UTC",
      theme: "dark",
      style: "1",
      locale: "en",
      enable_publishing: false,
      hide_side_toolbar: false,
      allow_symbol_change: true,
      studies: ["STD;RSI", "STD;MACD", "STD;Stochastic"],
      backgroundColor: "#0d1117",
      gridColor: "#21262d",
      support_host: "https://www.tradingview.com",
    });

    containerRef.current.appendChild(script);
  }, [symbol, timeframe]);

  return (
    <div className="card mb-6 p-0 overflow-hidden" style={{ height: "500px" }}>
      <div className="flex items-center gap-2 px-4 py-2 border-b border-ink-700">
        <BarChart3 size={15} className="text-accent" />
        <span className="text-sm font-semibold text-white">Live Chart — {symbol}</span>
        <span className="text-xs text-muted ml-auto">Powered by TradingView</span>
      </div>
      <div ref={containerRef} className="tradingview-widget-container" style={{ height: "calc(100% - 37px)", width: "100%" }} />
    </div>
  );
}
