-- SQLite schema (no CREATE TYPE; enums stored as TEXT with CHECK)

CREATE TABLE IF NOT EXISTS accounts (
    id           TEXT PRIMARY KEY,
    label        TEXT NOT NULL,
    broker       TEXT NOT NULL,
    account_ref  TEXT NOT NULL,
    mode         TEXT NOT NULL DEFAULT 'paper' CHECK(mode IN ('paper','signals','live')),
    balance      REAL NOT NULL DEFAULT 0,
    currency     TEXT NOT NULL DEFAULT 'USD',
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS strategies (
    id             TEXT PRIMARY KEY,
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    description    TEXT,
    asset_class    TEXT NOT NULL CHECK(asset_class IN ('forex','derivindex')),
    symbols        TEXT NOT NULL DEFAULT '[]',
    stop_loss      REAL,
    take_profit    REAL,
    risk_per_trade REAL NOT NULL DEFAULT 0.01,
    enabled        INTEGER NOT NULL DEFAULT 1,
    source         TEXT NOT NULL DEFAULT 'manual' CHECK(source IN ('manual','llm')),
    created_at     TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at     TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_strategies_account ON strategies(account_id);

CREATE TABLE IF NOT EXISTS rules (
    id           TEXT PRIMARY KEY,
    strategy_id  TEXT NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    expr         TEXT NOT NULL,
    weight       REAL NOT NULL DEFAULT 1.0,
    enabled      INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_rules_strategy ON rules(strategy_id);

CREATE TABLE IF NOT EXISTS notes (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    content      TEXT NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'markdown',
    status       TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','extracted','failed')),
    error        TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    processed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_notes_account ON notes(account_id);

CREATE TABLE IF NOT EXISTS signals (
    id           TEXT PRIMARY KEY,
    strategy_id  TEXT NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    symbol       TEXT NOT NULL,
    side         TEXT NOT NULL CHECK(side IN ('buy','sell')),
    price        REAL NOT NULL,
    strength     REAL NOT NULL,
    rationale    TEXT NOT NULL,
    mode         TEXT NOT NULL CHECK(mode IN ('paper','signals','live')),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_signals_account ON signals(account_id);

CREATE TABLE IF NOT EXISTS trades (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    strategy_id  TEXT NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
    signal_id    TEXT REFERENCES signals(id) ON DELETE SET NULL,
    symbol       TEXT NOT NULL,
    side         TEXT NOT NULL CHECK(side IN ('buy','sell')),
    order_type   TEXT NOT NULL DEFAULT 'market' CHECK(order_type IN ('market','limit','stop')),
    mode         TEXT NOT NULL CHECK(mode IN ('paper','signals','live')),
    size         REAL NOT NULL,
    entry_price  REAL NOT NULL,
    exit_price   REAL,
    stop_loss    REAL,
    take_profit  REAL,
    pnl          REAL,
    status       TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','closed','rejected','cancelled')),
    opened_at    TEXT NOT NULL DEFAULT (datetime('now')),
    closed_at    TEXT
);
CREATE INDEX IF NOT EXISTS idx_trades_account ON trades(account_id);
CREATE INDEX IF NOT EXISTS idx_trades_status ON trades(status);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT ''
);
