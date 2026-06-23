use sqlx::{Row, SqlitePool};

pub mod accounts;
pub mod analytics;
pub mod notes;
pub mod settings;
pub mod strategies;
pub mod trades;

#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
}

impl Db {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

/// Read a Decimal from a SQLite column (may be REAL, TEXT, or INTEGER).
pub(crate) fn read_dec(row: &sqlx::sqlite::SqliteRow, col: &str) -> rust_decimal::Decimal {
    use rust_decimal::prelude::FromStr;
    // Try f64 (REAL columns).
    if let Ok(f) = row.try_get::<f64, _>(col) {
        return rust_decimal::Decimal::try_from(f).unwrap_or(rust_decimal::Decimal::ZERO);
    }
    // Try i64 (INTEGER columns — e.g. COALESCE(SUM(...), 0) returns 0 as INTEGER).
    if let Ok(i) = row.try_get::<i64, _>(col) {
        return rust_decimal::Decimal::from(i);
    }
    // Try String (TEXT columns).
    if let Ok(s) = row.try_get::<String, _>(col) {
        return rust_decimal::Decimal::from_str(&s).unwrap_or(rust_decimal::Decimal::ZERO);
    }
    rust_decimal::Decimal::ZERO
}

/// Read an optional Decimal from a SQLite column.
pub(crate) fn read_dec_opt(row: &sqlx::sqlite::SqliteRow, col: &str) -> Option<rust_decimal::Decimal> {
    use rust_decimal::prelude::FromStr;
    if let Ok(f) = row.try_get::<Option<f64>, _>(col) {
        return f.and_then(|v| rust_decimal::Decimal::try_from(v).ok());
    }
    if let Ok(i) = row.try_get::<Option<i64>, _>(col) {
        return i.map(rust_decimal::Decimal::from);
    }
    let s: Option<String> = row.get(col);
    s.and_then(|s| rust_decimal::Decimal::from_str(&s).ok())
}

/// Read a Uuid from a SQLite column (may be TEXT or BLOB).
pub(crate) fn read_uuid(row: &sqlx::sqlite::SqliteRow, col: &str) -> uuid::Uuid {
    // Try as String first.
    if let Ok(s) = row.try_get::<String, _>(col) {
        return uuid::Uuid::parse_str(&s).unwrap_or_else(|_| uuid::Uuid::nil());
    }
    // Try as Vec<u8> (BLOB — 16 raw bytes).
    if let Ok(b) = row.try_get::<Vec<u8>, _>(col) {
        if b.len() == 16 {
            return uuid::Uuid::from_slice(&b).unwrap_or_else(|_| uuid::Uuid::nil());
        }
        // Might be a UTF-8 string stored as BLOB.
        if let Ok(s) = String::from_utf8(b) {
            return uuid::Uuid::parse_str(&s).unwrap_or_else(|_| uuid::Uuid::nil());
        }
    }
    uuid::Uuid::nil()
}

/// Read an optional Uuid from a SQLite column.
pub(crate) fn read_uuid_opt(row: &sqlx::sqlite::SqliteRow, col: &str) -> Option<uuid::Uuid> {
    // Try as Option<String>.
    if let Ok(s) = row.try_get::<Option<String>, _>(col) {
        return s.and_then(|v| uuid::Uuid::parse_str(&v).ok());
    }
    // Try as Option<Vec<u8>>.
    if let Ok(b) = row.try_get::<Option<Vec<u8>>, _>(col) {
        return b.and_then(|v| {
            if v.len() == 16 {
                uuid::Uuid::from_slice(&v).ok()
            } else {
                String::from_utf8(v).ok().and_then(|s| uuid::Uuid::parse_str(&s).ok())
            }
        });
    }
    None
}

/// Read a bool from a SQLite INTEGER column.
pub(crate) fn read_bool(row: &sqlx::sqlite::SqliteRow, col: &str) -> bool {
    let i: i64 = row.get(col);
    i != 0
}
