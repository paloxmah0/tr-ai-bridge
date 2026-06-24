import { useState } from "react";
import { api } from "../lib/api";
import type { Prediction, SignalFactor } from "../lib/api";
import { fmt, fmtPct } from "../lib/fmt";
import { Brain, Loader2, TrendingUp, TrendingDown, Minus, Zap, Activity, BarChart3, Clock, Globe, Target, CandlestickChart, Layers } from "lucide-react";

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

export default function AITrade() {
  const [symbol, setSymbol] = useState("R_100");
  const [timeframe, setTimeframe] = useState(15);
  const [analyzing, setAnalyzing] = useState(false);
  const [prediction, setPrediction] = useState<Prediction | null>(null);
  const [trading, setTrading] = useState(false);
  const [tradeResult, setTradeResult] = useState<string | null>(null);
  const [error, setError] = useState("");

  async function runAnalysis() {
    setAnalyzing(true); setError(""); setPrediction(null); setTradeResult(null);
    const m = MARKETS.find(m => m.symbol === symbol);
    try { setPrediction(await api.analyze(symbol, timeframe, m?.class)); }
    catch (e: any) { setError(e.message); } finally { setAnalyzing(false); }
  }

  async function placeTrade() {
    if (!prediction || prediction.direction === "hold") return;
    setTrading(true); setError("");
    try {
      const m = MARKETS.find(m => m.symbol === symbol);
      const r = await api.placeTrade(symbol, prediction.direction, timeframe, undefined, m?.class);
      setTradeResult(r.message || "Trade placed!");
    } catch (e: any) { setError(e.message); } finally { setTrading(false); }
  }

  const dir = prediction?.next_candle_direction;
  const DirIcon = dir === "bullish" ? TrendingUp : dir === "bearish" ? TrendingDown : Minus;
  const dirColor = dir === "bullish" ? "text-ok" : dir === "bearish" ? "text-bad" : "text-muted";
  const dirBg = dir === "bullish" ? "bg-ok/10 border-ok/30" : dir === "bearish" ? "bg-bad/10 border-bad/30" : "bg-ink-800 border-ink-700";

  return (
    <div>
      <div className="mb-6">
        <h2 className="text-xl font-bold text-white flex items-center gap-2"><Brain size={22} className="text-accent" /> AI Trade</h2>
        <p className="text-sm text-muted">Pick a market and timeframe — the AI predicts the NEXT candle</p>
      </div>

      {error && <div className="card border-bad/50 text-bad text-sm mb-4">{error}</div>}
      {tradeResult && <div className="card border-ok/50 text-ok text-sm mb-4">{tradeResult}</div>}

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
        <button onClick={runAnalysis} disabled={analyzing} className="btn-primary mt-4 w-full text-base py-2.5">
          {analyzing ? <><Loader2 size={18} className="inline mr-2 animate-spin" />Predicting next candle…</> : <><Brain size={18} className="inline mr-2" />Predict Next Candle</>}
        </button>
      </div>

      {prediction && (
        <div className="space-y-4">
          {/* Time + session + countdown */}
          <div className="card flex items-center justify-between text-sm">
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-1.5 text-muted"><Clock size={14} /><span className="font-mono text-xs">{fmtUTC(prediction.analysis_time_utc)}</span></div>
              <div className="flex items-center gap-1.5 text-accent"><Globe size={14} />{prediction.market_session}</div>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted">Next candle in:</span>
              <span className="font-mono text-sm font-bold text-warn bg-warn/10 px-2 py-0.5 rounded">{prediction.countdown}</span>
            </div>
          </div>

          {/* THE ANSWER */}
          <div className={`card border-2 ${dirBg}`}>
            <div className="text-center">
              <div className="label mb-2">What will the next {prediction.timeframe_secs / 60}min candle be?</div>
              <div className="flex items-center justify-center gap-3 mb-4">
                <DirIcon size={40} className={dirColor} />
                <span className={`text-3xl font-bold ${dirColor} uppercase`}>{prediction.next_candle_direction}</span>
              </div>
              <div className={`text-5xl font-bold ${dirColor} mb-2`}>{fmtPct(prediction.confidence)}</div>
              <div className="text-xs text-muted">confidence</div>
              <div className="mt-3 text-xs text-muted">
                Next candle starts in <span className="font-mono font-bold text-warn">{prediction.countdown}</span>
              </div>
            </div>
            {/* Projected OHLC */}
            <div className="grid grid-cols-4 gap-2 mt-4 pt-4 border-t border-ink-700 text-center">
              <div><div className="label">Open</div><div className="font-mono text-sm text-gray-200">{fmt(prediction.next_candle_open, 5)}</div></div>
              <div><div className="label">High</div><div className="font-mono text-sm text-gray-200">{fmt(prediction.next_candle_high, 5)}</div></div>
              <div><div className="label">Low</div><div className="font-mono text-sm text-gray-200">{fmt(prediction.next_candle_low, 5)}</div></div>
              <div><div className="label">Close</div><div className={`font-mono text-sm ${dir === "bullish" ? "text-ok" : dir === "bearish" ? "text-bad" : "text-gray-200"}`}>{fmt(prediction.next_candle_close, 5)}</div></div>
            </div>
          </div>

          {/* Trade levels */}
          <div className="grid grid-cols-3 gap-3">
            <div className="card text-center"><div className="label">Entry</div><div className="text-lg font-mono text-gray-200">{fmt(prediction.entry_price, 5)}</div></div>
            <div className="card text-center"><div className="label">Stop Loss</div><div className="text-lg font-mono text-bad">{fmt(prediction.stop_loss, 5)}</div></div>
            <div className="card text-center"><div className="label">Take Profit</div><div className="text-lg font-mono text-ok">{fmt(prediction.take_profit, 5)}</div></div>
          </div>

          {/* Recent candles */}
          {prediction.recent_candles && prediction.recent_candles.length > 0 && (
            <div className="card">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><CandlestickChart size={15} className="text-accent" /> Last {prediction.recent_candles.length} Candles (what just happened)</h3>
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead><tr className="text-left text-muted border-b border-ink-700">
                    <th className="py-1 px-2">#</th><th className="px-2">Dir</th><th className="px-2">Open</th>
                    <th className="px-2">High</th><th className="px-2">Low</th><th className="px-2">Close</th>
                    <th className="px-2">Body</th><th className="px-2">Pattern</th>
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
                        <td className="px-2 font-mono text-gray-400">{fmt(c.body, 5)}</td>
                        <td className="px-2 text-accent">{c.pattern}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Upper timeframe context */}
          {prediction.upper_timeframe_context && prediction.upper_timeframe_context.length > 0 && (
            <div className="card">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><Layers size={15} className="text-accent" /> Upper Timeframe Context (macro)</h3>
              <div className="space-y-2">
                {prediction.upper_timeframe_context.map((u, i) => (
                  <div key={i} className="flex items-center gap-3 bg-ink-900 rounded px-3 py-2">
                    <span className="font-bold text-white text-sm w-12">{u.label}</span>
                    <span className={u.trend === "bullish" ? "text-ok text-sm" : u.trend === "bearish" ? "text-bad text-sm" : "text-muted text-sm"}>{u.trend}</span>
                    <span className="text-xs text-muted">RSI {String(u.rsi)} | ADX {String(u.adx)} | {u.pattern}</span>
                    <span className="text-xs text-accent ml-auto">{u.summary.split("—")[1]?.trim() || ""}</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Scientific basis */}
          {prediction.scientific_basis && (
            <div className="card border-accent/20">
              <h3 className="text-sm font-semibold text-white mb-2 flex items-center gap-2"><Target size={15} className="text-accent" /> Scientific Basis</h3>
              <p className="text-sm text-gray-300">{prediction.scientific_basis}</p>
            </div>
          )}

          {/* Full report */}
          <div className="card">
            <h3 className="text-sm font-semibold text-white mb-2 flex items-center gap-2"><Activity size={15} className="text-accent" /> Full Analysis Report</h3>
            <pre className="text-xs text-gray-300 whitespace-pre-wrap font-mono leading-relaxed">{prediction.reasoning}</pre>
          </div>

          {/* Evidence */}
          {prediction.signals && prediction.signals.filter(s => s.weight > 0).length > 0 && (
            <div className="card">
              <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><BarChart3 size={15} className="text-accent" /> Evidence ({prediction.signals.filter(s => s.weight > 0).length})</h3>
              <div className="space-y-1">
                {prediction.signals.filter(s => s.weight > 0).map((s, i) => <FactorRow key={i} factor={s} />)}
              </div>
            </div>
          )}

          {/* Trade button */}
          {prediction.direction !== "hold" ? (
            <button onClick={placeTrade} disabled={trading} className={`btn w-full text-base py-3 ${prediction.direction === "buy" ? "bg-ok text-white hover:bg-ok/80" : "bg-bad text-white hover:bg-bad/80"}`}>
              {trading ? <><Loader2 size={18} className="inline mr-2 animate-spin" />Placing…</> : <><Zap size={18} className="inline mr-2" />Place {prediction.direction.toUpperCase()} Trade</>}
            </button>
          ) : (
            <div className="card text-center text-muted py-6"><Minus size={24} className="inline mb-2" /><p>Insufficient evidence — wait for a clearer setup.</p></div>
          )}
        </div>
      )}
    </div>
  );
}

function FactorRow({ factor }: { factor: SignalFactor }) {
  const Icon = factor.source.includes("candlestick") ? CandlestickChart : factor.source.includes("note") ? Brain : factor.source.includes("upper") ? Layers : factor.source.includes("momentum") ? Zap : BarChart3;
  const color = factor.direction === "bullish" ? "text-ok" : factor.direction === "bearish" ? "text-bad" : "text-muted";
  return (
    <div className="flex items-center gap-3 bg-ink-900 rounded px-3 py-2">
      <Icon size={14} className="text-muted shrink-0" />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-gray-200">{factor.name}</span>
          <span className={`badge ${factor.direction === "bullish" ? "bg-ok/20 text-ok" : factor.direction === "bearish" ? "bg-bad/20 text-bad" : "bg-ink-700 text-muted"}`}>{factor.direction}</span>
        </div>
        <div className="text-xs text-muted truncate">{factor.detail}</div>
      </div>
      <span className={`text-xs ${color} shrink-0`}>×{String(factor.weight)}</span>
    </div>
  );
}
