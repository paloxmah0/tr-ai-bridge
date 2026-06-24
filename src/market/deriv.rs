//! Deriv (deriv.com) WebSocket API client.
//!
//! Implements market data (candles + quotes) and live order placement for forex
//! and synthetic/derived indices. Deriv's API is a single bidirectional JSON
//! stream: each request carries a `req_id` and responses echo it, enabling
//! request/response multiplexing over one socket.

use crate::domain::{AssetClass, Candle, Side};
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::market::{Broker, BrokerOrder, MarketProvider, OrderRequest, Quote};

type WsStream = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WsSink = futures::stream::SplitSink<WsStream, Message>;

/// Connection state for the Deriv socket.
pub struct DerivClient {
    config: crate::dynamic_config::SharedConfig,
    /// Write half of the socket, guarded so only one writer at a time.
    sink: Arc<Mutex<Option<WsSink>>>,
    /// Pending request-response channels keyed by req_id.
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    req_id: AtomicU64,
    /// Serialize (re)connection attempts.
    connect_lock: Mutex<()>,
    authorized: Arc<Mutex<bool>>,
    /// Token used for the current connection; if it changes, reconnect.
    current_token: Mutex<String>,
    granularity: u32,
}

impl DerivClient {
    pub fn new(config: crate::dynamic_config::SharedConfig, granularity: u32) -> Self {
        Self {
            config,
            sink: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            req_id: AtomicU64::new(1),
            connect_lock: Mutex::new(()),
            authorized: Arc::new(Mutex::new(false)),
            current_token: Mutex::new(String::new()),
            granularity,
        }
    }

    async fn ws_url(&self) -> String {
        let app_id = self.config.read().await
            .get(crate::dynamic_config::keys::DERIV_APP_ID)
            .cloned()
            .unwrap_or_default();
        let app_id = if app_id.is_empty() { "1089" } else { &app_id };
        format!("wss://ws.derivws.com/websockets/v3?app_id={app_id}")
    }

    async fn token(&self) -> String {
        self.config.read().await
            .get(crate::dynamic_config::keys::DERIV_API_TOKEN)
            .cloned()
            .unwrap_or_default()
    }

    /// Ensure a connected + authorized socket exists; reconnect if dropped
    /// or if the token has changed since the last connection.
    async fn ensure_connected(&self) -> AppResult<()> {
        let token = self.token().await;

        // If token changed since last connect, force reconnect.
        let token_changed = *self.current_token.lock().await != token;
        if token_changed {
            self.disconnect().await;
        }

        if self.sink.lock().await.is_some() && *self.authorized.lock().await {
            return Ok(());
        }
        let _g = self.connect_lock.lock().await;

        // Double-check after acquiring lock.
        if self.sink.lock().await.is_some() && *self.authorized.lock().await {
            return Ok(());
        }

        let ws_url = self.ws_url().await;
        tracing::info!(url = %ws_url, "connecting to Deriv WebSocket");
        let (stream, _resp) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .map_err(|e| AppError::Market(format!("deriv ws connect: {e}")))?;

        let (sink, reader) = stream.split();
        *self.sink.lock().await = Some(sink);
        *self.authorized.lock().await = false;
        *self.current_token.lock().await = token.clone();

        // Spawn the reader task that demuxes responses by req_id.
        {
            let pending = self.pending.clone();
            let sink_slot = self.sink.clone();
            let authorized = self.authorized.clone();
            tokio::spawn(async move {
                reader_loop(reader, sink_slot, pending, authorized).await;
            });
        }

        // Authorize if a token is configured.
        if !token.is_empty() {
            let auth = self
                .send_raw(serde_json::json!({ "authorize": token }))
                .await?;
            if auth.get("error").is_some() {
                let msg = auth["error"]["message"].as_str().unwrap_or("authorize failed");
                self.disconnect().await;
                return Err(AppError::Unauthorized(format!("deriv authorize: {msg}")));
            }
            *self.authorized.lock().await = true;
            tracing::info!("deriv authorized");
        } else {
            *self.authorized.lock().await = true; // anonymous (market data only)
        }
        Ok(())
    }

    async fn disconnect(&self) {
        let mut sink = self.sink.lock().await;
        if let Some(mut s) = sink.take() {
            let _ = s.close().await;
        }
        *self.authorized.lock().await = false;
    }

    /// Send a JSON request and await the matching response (by req_id).
    pub async fn request(&self, msg: serde_json::Value) -> AppResult<serde_json::Value> {
        self.ensure_connected().await?;
        self.send_raw(msg).await
    }

    /// Low-level send: assumes the socket is already connected. Used internally
    /// by `ensure_connected` for authorize to avoid async recursion.
    async fn send_raw(&self, mut msg: serde_json::Value) -> AppResult<serde_json::Value> {
        let id = self.req_id.fetch_add(1, Ordering::SeqCst);
        if let Some(m) = msg.as_object_mut() {
            m.insert("req_id".into(), serde_json::Value::from(id));
        }

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let text = serde_json::to_string(&msg).map_err(|e| AppError::Market(e.to_string()))?;
        let frame = Message::Text(text.into());
        {
            let mut sink_guard = self.sink.lock().await;
            let sink = sink_guard
                .as_mut()
                .ok_or_else(|| AppError::Market("deriv socket closed".into()))?;
            if let Err(e) = sink.send(frame).await {
                self.pending.lock().await.remove(&id);
                self.disconnect().await;
                return Err(AppError::Market(format!("deriv send: {e}")));
            }
        }

        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                Err(AppError::Market("deriv response channel dropped".into()))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(AppError::Market("deriv request timed out".into()))
            }
        }
    }
}

/// Reader loop: owns the read half of the socket, routes responses by req_id.
/// On any read error/close, clears the shared sink so `ensure_connected`
/// reconnects on the next request.
async fn reader_loop(
    mut reader: futures::stream::SplitStream<WsStream>,
    sink_slot: Arc<Mutex<Option<WsSink>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    authorized: Arc<Mutex<bool>>,
) {
    loop {
        match reader.next().await {
            Some(Ok(Message::Text(txt))) => {
                let parsed: serde_json::Value = match serde_json::from_str(&txt) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let req_id = parsed.get("req_id").and_then(|v| v.as_u64()).unwrap_or(0);
                if req_id == 0 {
                    continue; // unsolicited stream update
                }
                if let Some(tx) = pending.lock().await.remove(&req_id) {
                    let _ = tx.send(parsed);
                }
            }
            Some(Ok(Message::Ping(p))) => {
                // Echo pong via the shared sink.
                let mut guard = sink_slot.lock().await;
                if let Some(s) = guard.as_mut() {
                    let _ = s.send(Message::Pong(p)).await;
                }
            }
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                let mut guard = sink_slot.lock().await;
                if let Some(mut s) = guard.take() {
                    let _ = s.close().await;
                }
                *authorized.lock().await = false;
                return;
            }
            _ => {}
        }
    }
}

/// Map a human symbol (e.g. "EUR/USD", "R_100") to a Deriv symbol.
/// Forex: "EUR/USD" -> "frxEURUSD". Known index tokens pass through.
pub fn to_deriv_symbol(symbol: &str) -> String {
    let trimmed = symbol.trim();
    // Already a valid Deriv symbol — pass through.
    if trimmed.starts_with("frx") || trimmed.starts_with("R_") || trimmed.starts_with("BOOM")
        || trimmed.starts_with("CRASH") || trimmed.starts_with("stp") || trimmed.starts_with("JD")
        || trimmed.starts_with("OTC") || trimmed.starts_with("1HZ") || trimmed.starts_with("jump")
    {
        return trimmed.to_string();
    }
    // Forex pair like "EUR/USD" or "EURUSD" -> "frxEURUSD"
    let alnum: String = trimmed.chars().filter(|c| c.is_alphanumeric()).collect();
    format!("frx{alnum}")
}

fn parse_dec(v: &serde_json::Value) -> Option<Decimal> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().and_then(|f| Decimal::try_from(f).ok()),
        serde_json::Value::String(s) => Decimal::from_str_exact(s).ok(),
        _ => None,
    }
}

#[async_trait]
impl MarketProvider for DerivClient {
    fn asset_class(&self) -> AssetClass {
        AssetClass::DerivIndex
    }

    async fn candles(&self, symbol: &str, count: usize) -> AppResult<Vec<Candle>> {
        let deriv_sym = to_deriv_symbol(symbol);
        let count = count.clamp(1, 5000) as u32;
        let req = serde_json::json!({
            "ticks_history": deriv_sym,
            "adjust_start_time": 1,
            "count": count,
            "end": "latest",
            "granularity": self.granularity,
            "style": "candles",
        });
        let resp = self.request(req).await?;
        if let Some(err) = resp.get("error") {
            return Err(AppError::Market(format!(
                "deriv candles: {}",
                err["message"].as_str().unwrap_or("unknown")
            )));
        }
        let Some(candles) = resp.get("candles").and_then(|c| c.as_array()) else {
            return Err(AppError::Market("deriv: no candles in response".into()));
        };
        let mut out = Vec::with_capacity(candles.len());
        for c in candles {
            let epoch = c.get("epoch").and_then(|v| v.as_i64()).unwrap_or(0);
            let ts = chrono::DateTime::from_timestamp(epoch, 0).unwrap_or_else(Utc::now);
            let open = parse_dec(c.get("open").unwrap_or(&serde_json::Value::Null))
                .ok_or_else(|| AppError::Market("deriv: bad open".into()))?;
            let high = parse_dec(c.get("high").unwrap_or(&serde_json::Value::Null))
                .ok_or_else(|| AppError::Market("deriv: bad high".into()))?;
            let low = parse_dec(c.get("low").unwrap_or(&serde_json::Value::Null))
                .ok_or_else(|| AppError::Market("deriv: bad low".into()))?;
            let close = parse_dec(c.get("close").unwrap_or(&serde_json::Value::Null))
                .ok_or_else(|| AppError::Market("deriv: bad close".into()))?;
            out.push(Candle {
                symbol: symbol.to_string(),
                ts,
                open,
                high,
                low,
                close,
                volume: Decimal::ZERO, // Deriv candle history has no volume.
            });
        }
        Ok(out)
    }

    async fn quote(&self, symbol: &str) -> AppResult<Quote> {
        let deriv_sym = to_deriv_symbol(symbol);
        let req = serde_json::json!({
            "ticks_history": deriv_sym,
            "count": 1,
            "end": "latest",
            "style": "ticks",
        });
        let resp = self.request(req).await?;
        if let Some(err) = resp.get("error") {
            return Err(AppError::Market(format!(
                "deriv quote: {}",
                err["message"].as_str().unwrap_or("unknown")
            )));
        }
        // Deriv returns { prices: [..], epochs: [..] } for tick style.
        let price = if let Some(prices) = resp.get("prices").and_then(|p| p.as_array()) {
            prices.last().and_then(parse_dec)
        } else if let Some(history) = resp.get("history") {
            history
                .get("prices")
                .and_then(|p| p.as_array())
                .and_then(|a| a.last().cloned())
                .and_then(|v| parse_dec(&v))
        } else {
            None
        }
        .ok_or_else(|| AppError::Market("deriv: no tick price".into()))?;

        let ts = if let Some(epochs) = resp.get("epochs").and_then(|p| p.as_array()) {
            epochs.last().and_then(|v| v.as_i64())
        } else {
            resp.get("history")
                .and_then(|h| h.get("times"))
                .and_then(|t| t.as_array())
                .and_then(|a| a.last().cloned())
                .and_then(|v| v.as_i64())
        }
        .and_then(|e| chrono::DateTime::from_timestamp(e, 0))
        .unwrap_or_else(Utc::now);

        // Deriv reports a single price; derive a synthetic spread for fill realism.
        let spread = price * Decimal::new(1, 6); // 0.0001% spread
        Ok(Quote {
            symbol: symbol.to_string(),
            bid: price - spread,
            ask: price + spread,
            ts,
        })
    }
}

#[async_trait]
impl Broker for DerivClient {
    fn name(&self) -> &'static str { "deriv" }

    /// Place a real Deriv contract (CALL=buy, PUT=sell). Uses `stake`,
    /// `duration_secs`, and monetary `stop_loss_amount`/`take_profit_amount`
    /// (passed as Deriv limit orders when set).
    async fn place_order(&self, req: OrderRequest) -> AppResult<BrokerOrder> {
        let deriv_sym = to_deriv_symbol(&req.symbol);
        let contract_type = match req.side {
            Side::Buy => "CALL",
            Side::Sell => "PUT",
        };
        let duration = req.duration_secs.unwrap_or(300).max(15);

        let mut proposal = serde_json::json!({
            "proposal": 1,
            "amount": req.stake,
            "basis": "stake",
            "contract_type": contract_type,
            "currency": "USD",
            "duration": duration,
            "duration_unit": "s",
            "symbol": deriv_sym,
        });
        if let Some(obj) = proposal.as_object_mut() {
            let mut limits = serde_json::Map::new();
            if let Some(sl) = req.stop_loss_amount {
                limits.insert("stop_loss".into(), serde_json::json!(sl));
            }
            if let Some(tp) = req.take_profit_amount {
                limits.insert("take_profit".into(), serde_json::json!(tp));
            }
            if !limits.is_empty() {
                obj.insert("limit_order".into(), serde_json::Value::Object(limits));
            }
        }

        let prop_resp = self.request(proposal).await?;
        if let Some(err) = prop_resp.get("error") {
            return Err(AppError::Execution(format!(
                "deriv proposal: {}",
                err["message"].as_str().unwrap_or("unknown")
            )));
        }
        let proposal_id = prop_resp
            .get("proposal")
            .and_then(|p| p.get("id"))
            .and_then(|i| i.as_str())
            .ok_or_else(|| AppError::Execution("deriv: missing proposal id".into()))?;
        let ask_price = prop_resp
            .get("proposal")
            .and_then(|p| p.get("ask_price"))
            .and_then(parse_dec)
            .unwrap_or(req.stake);

        let buy_resp = self
            .request(serde_json::json!({ "buy": proposal_id, "price": ask_price }))
            .await?;
        if let Some(err) = buy_resp.get("error") {
            return Err(AppError::Execution(format!(
                "deriv buy: {}",
                err["message"].as_str().unwrap_or("unknown")
            )));
        }
        let buy = buy_resp
            .get("buy")
            .ok_or_else(|| AppError::Execution("deriv: missing buy object".into()))?;
        Ok(BrokerOrder {
            broker_ref: buy
                .get("contract_id")
                .and_then(|c| c.as_u64())
                .map(|c| c.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            filled_price: buy.get("price").and_then(parse_dec).unwrap_or(ask_price),
            balance_after: buy_resp.get("balance").and_then(parse_dec).unwrap_or(req.stake),
        })
    }

    async fn balance(&self) -> AppResult<Decimal> {
        let resp = self
            .request(serde_json::json!({ "balance": 1, "subscribe": 0 }))
            .await?;
        if let Some(err) = resp.get("error") {
            return Err(AppError::Market(format!(
                "deriv balance: {}",
                err["message"].as_str().unwrap_or("unknown")
            )));
        }
        resp.get("balance")
            .and_then(|b| b.get("balance"))
            .and_then(parse_dec)
            .ok_or_else(|| AppError::Market("deriv: missing balance".into()))
    }
}
