use crate::db::Db;
use crate::db::{read_dec, read_dec_opt, read_uuid, read_uuid_opt};
use crate::domain::signal::Signal;
use crate::domain::trade::*;
use crate::domain::{OrderType, Side, TradingMode};
use crate::error::AppResult;
use rust_decimal::Decimal;
use sqlx::Row;
use uuid::Uuid;

impl Db {
    pub async fn insert_signal(&self, s: &Signal) -> AppResult<Uuid> {
        let row = sqlx::query(
            r#"INSERT INTO signals (id, strategy_id, account_id, symbol, side, price, strength, rationale, mode)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id"#,
        )
        .bind(s.id)
        .bind(s.strategy_id)
        .bind(s.account_id)
        .bind(&s.symbol)
        .bind(s.side as Side)
        .bind(s.price.to_string())
        .bind(s.strength.to_string())
        .bind(&s.rationale)
        .bind(s.mode as TradingMode)
        .fetch_one(&self.pool)
        .await?;
        Ok(read_uuid(&row, "id"))
    }

    pub async fn list_signals(&self, account_id: Uuid, limit: i64) -> AppResult<Vec<Signal>> {
        let rows = sqlx::query(
            r#"SELECT id, strategy_id, account_id, symbol, side, price, strength, rationale, mode, created_at
               FROM signals WHERE account_id = $1 ORDER BY created_at DESC LIMIT $2"#,
        )
        .bind(account_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(map_signal).collect())
    }

    pub async fn insert_trade(&self, t: &Trade) -> AppResult<Uuid> {
        let row = sqlx::query(
            r#"INSERT INTO trades (id, account_id, strategy_id, signal_id, symbol, side, order_type, mode,
                                  size, entry_price, exit_price, stop_loss, take_profit, pnl, status, opened_at, closed_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) RETURNING id"#,
        )
        .bind(t.id)
        .bind(t.account_id)
        .bind(t.strategy_id)
        .bind(t.signal_id)
        .bind(&t.symbol)
        .bind(t.side as Side)
        .bind(t.order_type as OrderType)
        .bind(t.mode as TradingMode)
        .bind(t.size.to_string())
        .bind(t.entry_price.to_string())
        .bind(t.exit_price.map(|d| d.to_string()))
        .bind(t.stop_loss.map(|d| d.to_string()))
        .bind(t.take_profit.map(|d| d.to_string()))
        .bind(t.pnl.map(|d| d.to_string()))
        .bind(t.status as TradeStatus)
        .bind(t.opened_at)
        .bind(t.closed_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(read_uuid(&row, "id"))
    }

    pub async fn close_trade(&self, id: Uuid, exit_price: Decimal, pnl: Decimal) -> AppResult<Option<Trade>> {
        let row = sqlx::query(
            r#"UPDATE trades SET status = 'closed', exit_price = $2, pnl = $3, closed_at = datetime('now')
               WHERE id = $1 AND status = 'open'
               RETURNING id, account_id, strategy_id, signal_id, symbol, side, order_type, mode,
                         size, entry_price, exit_price, stop_loss, take_profit, pnl, status, opened_at, closed_at"#,
        )
        .bind(id)
        .bind(exit_price.to_string())
        .bind(pnl.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(map_trade))
    }

    pub async fn list_trades(&self, account_id: Uuid, limit: i64) -> AppResult<Vec<Trade>> {
        let rows = sqlx::query(
            r#"SELECT id, account_id, strategy_id, signal_id, symbol, side, order_type, mode,
                      size, entry_price, exit_price, stop_loss, take_profit, pnl, status, opened_at, closed_at
               FROM trades WHERE account_id = $1 ORDER BY opened_at DESC LIMIT $2"#,
        )
        .bind(account_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(map_trade).collect())
    }

    pub async fn list_open_trades(&self) -> AppResult<Vec<Trade>> {
        let rows = sqlx::query(
            r#"SELECT id, account_id, strategy_id, signal_id, symbol, side, order_type, mode,
                      size, entry_price, exit_price, stop_loss, take_profit, pnl, status, opened_at, closed_at
               FROM trades WHERE status = 'open' ORDER BY opened_at"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(map_trade).collect())
    }

    /// List closed trades for a specific symbol (used by self-learning).
    pub async fn list_trades_by_symbol(&self, symbol: &str) -> AppResult<Vec<Trade>> {
        let rows = sqlx::query(
            r#"SELECT id, account_id, strategy_id, signal_id, symbol, side, order_type, mode,
                      size, entry_price, exit_price, stop_loss, take_profit, pnl, status, opened_at, closed_at
               FROM trades WHERE symbol = $1 AND status = 'closed' ORDER BY opened_at DESC LIMIT 50"#,
        )
        .bind(symbol)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(map_trade).collect())
    }
}

fn map_signal(row: &sqlx::sqlite::SqliteRow) -> Signal {
    Signal {
        id: read_uuid(row, "id"),
        strategy_id: read_uuid(row, "strategy_id"),
        account_id: read_uuid(row, "account_id"),
        symbol: row.get("symbol"),
        side: row.get("side"),
        price: read_dec(row, "price"),
        strength: read_dec(row, "strength"),
        rationale: row.get("rationale"),
        mode: row.get("mode"),
        created_at: row.get("created_at"),
    }
}

fn map_trade(row: &sqlx::sqlite::SqliteRow) -> Trade {
    Trade {
        id: read_uuid(row, "id"),
        account_id: read_uuid(row, "account_id"),
        strategy_id: read_uuid(row, "strategy_id"),
        signal_id: read_uuid_opt(row, "signal_id"),
        symbol: row.get("symbol"),
        side: row.get("side"),
        order_type: row.get("order_type"),
        mode: row.get("mode"),
        size: read_dec(row, "size"),
        entry_price: read_dec(row, "entry_price"),
        exit_price: read_dec_opt(row, "exit_price"),
        stop_loss: read_dec_opt(row, "stop_loss"),
        take_profit: read_dec_opt(row, "take_profit"),
        pnl: read_dec_opt(row, "pnl"),
        status: row.get("status"),
        opened_at: row.get("opened_at"),
        closed_at: row.get("closed_at"),
    }
}
