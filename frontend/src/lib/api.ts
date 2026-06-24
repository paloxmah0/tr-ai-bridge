const BASE = "/api";

async function req<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { "Content-Type": "application/json", ...(opts?.headers || {}) },
    ...opts,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `HTTP ${res.status}`);
  }
  return res.json();
}

// ---- Types ----
export type TradingMode = "paper" | "signals" | "live";
export type Side = "buy" | "sell";
export type AssetClass = "forex" | "derivindex";
export type StrategySource = "manual" | "llm";
export type NoteStatus = "pending" | "extracted" | "failed";
export type TradeStatus = "open" | "closed" | "rejected" | "cancelled";

export interface Account {
  id: string; label: string; broker: string; account_ref: string;
  mode: TradingMode; balance: number; currency: string; created_at: string;
}
export interface Rule { id: string; strategy_id: string; name: string; expr: string; weight: number; enabled: boolean; }
export interface Strategy {
  id: string; account_id: string; name: string; description: string | null;
  asset_class: AssetClass; symbols: string[]; stop_loss: number | null;
  take_profit: number | null; risk_per_trade: number; enabled: boolean;
  source: StrategySource; created_at: string; updated_at: string;
}
export interface StrategyWithRules extends Strategy { rules: Rule[]; }
export interface Note {
  id: string; account_id: string; title: string; content: string;
  content_type: string; status: NoteStatus; error: string | null;
  created_at: string; processed_at: string | null;
}
export interface Signal {
  id: string; strategy_id: string; account_id: string; symbol: string;
  side: Side; price: number; strength: number; rationale: string;
  mode: TradingMode; created_at: string;
}
export interface Trade {
  id: string; account_id: string; strategy_id: string; signal_id: string | null;
  symbol: string; side: Side; order_type: string; mode: TradingMode;
  size: number; entry_price: number; exit_price: number | null;
  stop_loss: number | null; take_profit: number | null; pnl: number | null;
  status: TradeStatus; opened_at: string; closed_at: string | null;
}
export interface AnalyticsSummary {
  account_id: string; total_trades: number; open_trades: number; closed_trades: number;
  winning_trades: number; losing_trades: number; win_rate: number; total_pnl: number;
  avg_pnl: number; best_trade: number | null; worst_trade: number | null;
}
export interface StrategyPerf { strategy_id: string; trades: number; win_rate: number; total_pnl: number; }
export interface AnalyticsResp { summary: AnalyticsSummary; per_strategy: StrategyPerf[]; }
export interface Insight {
  summary: AnalyticsSummary; recent_signals: number; open_exposure: number; notes: string[];
}
export interface EquityPoint { ts: string; equity: number; }
export interface BacktestTrade {
  side: Side; entry_ts: string; entry_price: number; exit_ts: string;
  exit_price: number; pnl: number; exit_reason: string; strength: number;
}
export interface BacktestResult {
  symbol: string; initial_balance: number; final_equity: number; total_return_pct: number;
  trades: BacktestTrade[]; closed_trades: number; winning_trades: number; losing_trades: number;
  win_rate: number; total_pnl: number; avg_pnl: number; max_drawdown_pct: number;
  sharpe_ratio: number; equity_curve: EquityPoint[];
}

// ---- API ----
export const api = {
  // accounts
  listAccounts: () => req<Account[]>("/accounts"),
  getAccount: (id: string) => req<Account>(`/accounts/${id}`),
  createAccount: (body: { label: string; broker: string; account_ref: string; balance?: number; currency?: string; mode?: string }) =>
    req<Account>("/accounts", { method: "POST", body: JSON.stringify(body) }),
  setMode: (id: string, mode: TradingMode) =>
    req<Account>(`/accounts/${id}/mode`, { method: "POST", body: JSON.stringify({ mode }) }),

  // strategies
  listStrategies: (accountId: string) => req<StrategyWithRules[]>(`/accounts/${accountId}/strategies`),
  getStrategy: (id: string) => req<StrategyWithRules>(`/strategies/${id}`),
  createStrategy: (accountId: string, body: any) =>
    req<StrategyWithRules>(`/accounts/${accountId}/strategies`, { method: "POST", body: JSON.stringify(body) }),
  updateStrategy: (id: string, body: any) =>
    req<StrategyWithRules>(`/strategies/${id}`, { method: "PUT", body: JSON.stringify(body) }),
  deleteStrategy: (id: string) =>
    req<{ deleted: boolean }>(`/strategies/${id}`, { method: "DELETE" }),

  // notes
  listNotes: (accountId: string) => req<Note[]>(`/accounts/${accountId}/notes`),
  createNote: (accountId: string, body: { title: string; content: string; content_type?: string }) =>
    req<Note>(`/accounts/${accountId}/notes`, { method: "POST", body: JSON.stringify(body) }),
  processNote: (id: string) =>
    req<{ note: Note; strategy_id: string | null; error: string | null }>(`/notes/${id}`, { method: "POST" }),

  // signals & trades
  listSignals: (accountId: string) => req<Signal[]>(`/accounts/${accountId}/signals`),
  listTrades: (accountId: string) => req<Trade[]>(`/accounts/${accountId}/trades`),
  closeTrade: (id: string, exitPrice: number) =>
    req<Trade>(`/trades/${id}/close`, { method: "POST", body: JSON.stringify({ exit_price: exitPrice }) }),

  // analytics
  analytics: (accountId: string) => req<AnalyticsResp>(`/accounts/${accountId}/analytics`),
  insights: (accountId: string) => req<Insight>(`/accounts/${accountId}/insights`),

  // backtest
  backtest: (strategyId: string, body: { symbol: string; initial_balance?: number; candles?: number }) =>
    req<BacktestResult>(`/strategies/${strategyId}/backtest`, { method: "POST", body: JSON.stringify(body) }),

  // settings
  getSettings: () => req<{ values: Record<string, string>; masked: Record<string, string>; is_set: Record<string, boolean> }>(`/settings`),
  updateSettings: (values: Record<string, string>) =>
    req<{ updated: string[] }>(`/settings`, { method: "PUT", body: JSON.stringify(values) }),
  testService: (service: string) =>
    req<{ ok: boolean; message: string }>(`/settings/test`, { method: "POST", body: JSON.stringify({ service }) }),

  // AI trade
  analyze: (symbol: string, timeframe_minutes: number, asset_class?: string) =>
    req<Prediction>(`/analyze`, { method: "POST", body: JSON.stringify({ symbol, timeframe_minutes, asset_class }) }),
  placeTrade: (symbol: string, direction: string, timeframe_minutes: number, stake?: number, asset_class?: string) =>
    req<any>(`/trade`, { method: "POST", body: JSON.stringify({ symbol, direction, timeframe_minutes, stake, asset_class }) }),
};

export interface SignalFactor {
  source: string; name: string; direction: string; weight: number; detail: string;
}
export interface CandleSummary {
  direction: string; open: number; high: number; low: number; close: number;
  body: number; upper_wick: number; lower_wick: number; pattern: string;
}
export interface UpperTFContext {
  label: string; trend: string; last_candle_dir: string; rsi: number; adx: number;
  pattern: string; summary: string;
}
export interface Prediction {
  next_candle_direction: string; confidence: number;
  next_candle_open: number; next_candle_high: number; next_candle_low: number; next_candle_close: number;
  direction: string; entry_price: number; stop_loss: number; take_profit: number;
  expiry: string; reasoning: string; signals: SignalFactor[];
  timeframe_secs: number; symbol: string;
  analysis_time_utc: string; market_session: string; scientific_basis: string;
  current_candle_start: string; next_candle_start: string;
  seconds_to_next_candle: number; countdown: string;
  recent_candles: CandleSummary[];
  upper_timeframe_context: UpperTFContext[];
}
