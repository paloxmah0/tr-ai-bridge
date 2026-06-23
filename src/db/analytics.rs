use crate::db::{read_dec, read_dec_opt, read_uuid, Db};
use crate::error::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsSummary {
    pub account_id: Uuid,
    pub total_trades: i64,
    pub open_trades: i64,
    pub closed_trades: i64,
    pub winning_trades: i64,
    pub losing_trades: i64,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub avg_pnl: Decimal,
    pub best_trade: Option<Decimal>,
    pub worst_trade: Option<Decimal>,
}

#[derive(Debug, Serialize)]
pub struct StrategyPerf {
    pub strategy_id: Uuid,
    pub trades: i64,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
}

impl Db {
    pub async fn analytics_summary(&self, account_id: Uuid) -> AppResult<AnalyticsSummary> {
        let row = sqlx::query(
            r#"SELECT
                 COUNT(*) AS total,
                 COUNT(*) FILTER (WHERE status = 'open') AS open_t,
                 COUNT(*) FILTER (WHERE status = 'closed') AS closed_t,
                 COUNT(*) FILTER (WHERE status = 'closed' AND pnl > 0) AS wins,
                 COUNT(*) FILTER (WHERE status = 'closed' AND pnl <= 0) AS losses,
                 COALESCE(SUM(pnl) FILTER (WHERE status = 'closed'), 0) AS total_pnl,
                 COALESCE(AVG(pnl) FILTER (WHERE status = 'closed'), 0) AS avg_pnl,
                 MAX(pnl) FILTER (WHERE status = 'closed') AS best,
                 MIN(pnl) FILTER (WHERE status = 'closed') AS worst
               FROM trades WHERE account_id = $1"#,
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?;

        let total: i64 = row.get("total");
        let closed: i64 = row.get("closed_t");
        let wins: i64 = row.get("wins");
        let win_rate = if closed > 0 {
            Decimal::from(wins) / Decimal::from(closed)
        } else {
            Decimal::ZERO
        };

        Ok(AnalyticsSummary {
            account_id,
            total_trades: total,
            open_trades: row.get("open_t"),
            closed_trades: closed,
            winning_trades: wins,
            losing_trades: row.get("losses"),
            win_rate,
            total_pnl: read_dec(&row, "total_pnl"),
            avg_pnl: read_dec(&row, "avg_pnl"),
            best_trade: read_dec_opt(&row, "best"),
            worst_trade: read_dec_opt(&row, "worst"),
        })
    }

    pub async fn per_strategy(&self, account_id: Uuid) -> AppResult<Vec<StrategyPerf>> {
        let rows = sqlx::query(
            r#"SELECT strategy_id,
                      COUNT(*) AS trades,
                      COALESCE(SUM(pnl) FILTER (WHERE status = 'closed'), 0) AS total_pnl,
                      COUNT(*) FILTER (WHERE status = 'closed') AS closed_t,
                      COUNT(*) FILTER (WHERE status = 'closed' AND pnl > 0) AS wins
               FROM trades WHERE account_id = $1
               GROUP BY strategy_id"#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let closed: i64 = r.get("closed_t");
                let wins: i64 = r.get("wins");
                let win_rate = if closed > 0 {
                    Decimal::from(wins) / Decimal::from(closed)
                } else {
                    Decimal::ZERO
                };
                StrategyPerf {
                    strategy_id: read_uuid(r, "strategy_id"),
                    trades: r.get("trades"),
                    win_rate,
                    total_pnl: read_dec(r, "total_pnl"),
                }
            })
            .collect())
    }
}
