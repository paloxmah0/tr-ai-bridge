import { useState } from "react";
import { api } from "../lib/api";
import type { Prediction, SignalFactor } from "../lib/api";
import { fmt, fmtPct } from "../lib/fmt";
import { Brain, Loader2, TrendingUp, TrendingDown, Minus, Zap, Activity, BookOpen, BarChart3 } from "lucide-react";

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
  { mins: 1, label: "1 min" },
  { mins: 5, label: "5 min" },
  { mins: 15, label: "15 min" },
  { mins: 30, label: "30 min" },
  { mins: 60, label: "1 hour" },
  { mins: 240, label: "4 hours" },
];

export default function AITrade() {
  const [symbol, setSymbol] = useState("R_100");
  const [timeframe, setTimeframe] = useState(5);
  const [analyzing, setAnalyzing] = useState(false);
  const [prediction, setPrediction] = useState<Prediction | null>(null);
  const [trading, setTrading] = useState(false);
  const [tradeResult, setTradeResult] = useState<string | null>(null);
  const [error, setError] = useState("");

  async function runAnalysis() {
    setAnalyzing(true); setError(""); setPrediction(null); setTradeResult(null);
    const market = MARKETS.find(m => m.symbol === symbol);
    try {
      const pred = await api.analyze(symbol, timeframe, market?.class);
      setPrediction(pred);
    } catch (e: any) { setError(e.message); } finally { setAnalyzing(false); }
  }

  async function placeTrade() {
    if (!prediction || prediction.direction === "hold") return;
    setTrading(true); setError("");
    try {
      const market = MARKETS.find(m => m.symbol === symbol);
      const result = await api.placeTrade(symbol, prediction.direction, timeframe, undefined, market?.class);
      setTradeResult(result.message || "Trade placed successfully!");
    } catch (e: any) { setError(e.message); } finally { setTrading(false); }
  }

  const dirColor = prediction?.direction === "buy" ? "text-ok" : prediction?.direction === "sell" ? "text-bad" : "text-muted";
  const dirBg = prediction?.direction === "buy" ? "bg-ok/10 border-ok/30" : prediction?.direction === "sell" ? "bg-bad/10 border-bad/30" : "bg-ink-800 border-ink-700";
  const DirIcon = prediction?.direction === "buy" ? TrendingUp : prediction?.direction === "sell" ? TrendingDown : Minus;

  return (
    <div>
      <div className="mb-6">
        <h2 className="text-xl font-bold text-white flex items-center gap-2"><Brain size={22} className="text-accent" /> AI Trade</h2>
        <p className="text-sm text-muted">Pick a market and timeframe — the AI analyzes everything it knows and predicts BUY or SELL</p>
      </div>

      {error && <div className="card border-bad/50 text-bad text-sm mb-4">{error}</div>}
      {tradeResult && <div className="card border-ok/50 text-ok text-sm mb-4">{tradeResult}</div>}

      {/* Controls */}
      <div className="card mb-6">
        <div className="grid grid-cols-2 gap-4">
          <div>
            <div className="label mb-2">Market</div>
            <select className="input w-full" value={symbol} onChange={e => { setSymbol(e.target.value); setPrediction(null); }}>
              {MARKETS.map(m => <option key={m.symbol} value={m.symbol}>{m.label}</option>)}
            </select>
          </div>
          <div>
            <div className="label mb-2">Timeframe</div>
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
          {analyzing ? <><Loader2 size={18} className="inline mr-2 animate-spin" />Analyzing market…</> : <><Brain size={18} className="inline mr-2" />Analyze Market</>}
        </button>
      </div>

      {/* Prediction */}
      {prediction && (
        <div className="space-y-4">
          {/* Direction card */}
          <div className={`card border-2 ${dirBg}`}>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                {prediction.direction !== "hold" && (
                  <div className={`w-16 h-16 rounded-full flex items-center justify-center ${prediction.direction === "buy" ? "bg-ok/20" : "bg-bad/20"}`}>
                    <DirIcon size={32} className={dirColor} />
                  </div>
                )}
                <div>
                  <div className={`text-2xl font-bold ${dirColor} uppercase`}>{prediction.direction}</div>
                  <div className="text-sm text-muted">{prediction.symbol} · {prediction.timeframe_secs / 60} min</div>
                </div>
              </div>
              <div className="text-right">
                <div className="label">Confidence</div>
                <div className={`text-3xl font-bold ${dirColor}`}>{fmtPct(prediction.confidence)}</div>
              </div>
            </div>

            {/* Entry / SL / TP */}
            <div className="grid grid-cols-3 gap-3 mt-4 pt-4 border-t border-ink-700">
              <div><div className="label">Entry</div><div className="text-lg font-mono text-gray-200">{fmt(prediction.entry_price, 5)}</div></div>
              <div><div className="label">Stop Loss</div><div className="text-lg font-mono text-bad">{fmt(prediction.stop_loss, 5)}</div></div>
              <div><div className="label">Take Profit</div><div className="text-lg font-mono text-ok">{fmt(prediction.take_profit, 5)}</div></div>
            </div>
          </div>

          {/* Reasoning */}
          <div className="card">
            <h3 className="text-sm font-semibold text-white mb-2 flex items-center gap-2"><Activity size={15} className="text-accent" /> AI Reasoning</h3>
            <p className="text-sm text-gray-300 whitespace-pre-wrap">{prediction.reasoning}</p>
          </div>

          {/* Signal factors */}
          <div className="card">
            <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><BarChart3 size={15} className="text-accent" /> Signal Breakdown ({prediction.signals.length})</h3>
            <div className="space-y-1">
              {prediction.signals.filter(s => s.weight > 0).map((s, i) => (
                <FactorRow key={i} factor={s} />
              ))}
            </div>
          </div>

          {/* Place trade */}
          {prediction.direction !== "hold" && (
            <button onClick={placeTrade} disabled={trading} className={`btn w-full text-base py-3 ${prediction.direction === "buy" ? "bg-ok text-white hover:bg-ok/80" : "bg-bad text-white hover:bg-bad/80"}`}>
              {trading ? <><Loader2 size={18} className="inline mr-2 animate-spin" />Placing trade…</> : <><Zap size={18} className="inline mr-2" />Place {prediction.direction.toUpperCase()} Trade on {prediction.symbol}</>}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function FactorRow({ factor }: { factor: SignalFactor }) {
  const icon = factor.source === "candlestick" ? Activity : factor.source === "note" ? BookOpen : factor.source === "momentum" ? Zap : BarChart3;
  const color = factor.direction === "bullish" ? "text-ok" : factor.direction === "bearish" ? "text-bad" : "text-muted";
  const Icon = icon;
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
      <span className={`text-xs ${color} shrink-0`}>×{factor.weight}</span>
    </div>
  );
}
