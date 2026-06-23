use crate::db::Db;
use crate::db::{read_dec, read_uuid};
use crate::domain::{Account, TradingMode};
use crate::error::AppResult;
use rust_decimal::Decimal;
use sqlx::Row;
use uuid::Uuid;

impl Db {
    pub async fn create_account(
        &self,
        label: &str,
        broker: &str,
        account_ref: &str,
        balance: Decimal,
        currency: &str,
        mode: TradingMode,
    ) -> AppResult<Account> {
        let id = Uuid::new_v4();
        let row = sqlx::query(
            r#"INSERT INTO accounts (id, label, broker, account_ref, balance, currency, mode)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id, label, broker, account_ref, mode, balance, currency, created_at"#,
        )
        .bind(id)
        .bind(label)
        .bind(broker)
        .bind(account_ref)
        .bind(balance.to_string())
        .bind(currency)
        .bind(mode as TradingMode)
        .fetch_one(&self.pool)
        .await?;
        Ok(map_account(&row))
    }

    pub async fn get_account(&self, id: Uuid) -> AppResult<Option<Account>> {
        let row = sqlx::query(
            r#"SELECT id, label, broker, account_ref, mode, balance, currency, created_at
               FROM accounts WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(map_account))
    }

    pub async fn list_accounts(&self) -> AppResult<Vec<Account>> {
        let rows = sqlx::query(
            r#"SELECT id, label, broker, account_ref, mode, balance, currency, created_at
               FROM accounts ORDER BY created_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(map_account).collect())
    }

    pub async fn set_account_mode(&self, id: Uuid, mode: TradingMode) -> AppResult<Option<Account>> {
        let row = sqlx::query(
            r#"UPDATE accounts SET mode = $2 WHERE id = $1
               RETURNING id, label, broker, account_ref, mode, balance, currency, created_at"#,
        )
        .bind(id)
        .bind(mode as TradingMode)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(map_account))
    }

    pub async fn adjust_balance(&self, id: Uuid, delta: Decimal) -> AppResult<Decimal> {
        let row = sqlx::query("UPDATE accounts SET balance = balance + $2 WHERE id = $1 RETURNING balance")
            .bind(id)
            .bind(delta.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(read_dec(&row, "balance"))
    }
}

fn map_account(row: &sqlx::sqlite::SqliteRow) -> Account {
    Account {
        id: read_uuid(row, "id"),
        label: row.get("label"),
        broker: row.get("broker"),
        account_ref: row.get("account_ref"),
        mode: row.get("mode"),
        balance: read_dec(row, "balance"),
        currency: row.get("currency"),
        created_at: row.get("created_at"),
    }
}
