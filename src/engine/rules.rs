use crate::domain::Candle;
use crate::error::{AppError, AppResult};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Indicator set computed once per evaluation window.
#[derive(Debug, Clone, Default)]
pub struct Indicators {
    pub rsi: HashMap<usize, Decimal>,
    pub ema: HashMap<usize, Decimal>,
    pub sma: HashMap<usize, Decimal>,
    pub atr: HashMap<usize, Decimal>,
    pub macd: Option<Decimal>,
    pub price: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub open: Decimal,
    pub volume: Decimal,
    pub pct_change: Decimal,
    /// Candlestick pattern signals (1.0 = pattern present, 0.0 = not).
    pub patterns: HashMap<String, Decimal>,
    /// Previous candle data for multi-candle patterns.
    pub prev_open: Decimal,
    pub prev_close: Decimal,
    pub prev_high: Decimal,
    pub prev_low: Decimal,
}

impl Indicators {
    pub fn compute(candles: &[Candle]) -> AppResult<Self> {
        let last = candles.last().ok_or_else(|| AppError::Internal("no candles".into()))?;
        let mut ind = Indicators {
            price: last.close,
            high: last.high,
            low: last.low,
            close: last.close,
            open: last.open,
            volume: last.volume,
            ..Default::default()
        };

        let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
        if closes.len() >= 2 {
            let prev = closes[closes.len() - 2];
            if prev != Decimal::ZERO {
                let pct = (last.close - prev) / prev * Decimal::from(100);
                ind.pct_change = pct.round_dp(4);
            }
        }
        ind.rsi.insert(14, rsi(&closes, 14).unwrap_or(Decimal::from(50)));
        ind.ema.insert(50, ema(&closes, 50).unwrap_or(last.close));
        ind.ema.insert(200, ema(&closes, 200).unwrap_or(last.close));
        ind.sma.insert(50, sma(&closes, 50).unwrap_or(last.close));
        ind.sma.insert(200, sma(&closes, 200).unwrap_or(last.close));
        ind.atr.insert(14, atr(candles, 14).unwrap_or(Decimal::ZERO));
        ind.macd = Some(macd(&closes));

        // Store previous candle for multi-candle patterns.
        if candles.len() >= 2 {
            let prev = &candles[candles.len() - 2];
            ind.prev_open = prev.open;
            ind.prev_close = prev.close;
            ind.prev_high = prev.high;
            ind.prev_low = prev.low;
        }

        // Compute candlestick patterns.
        ind.patterns = detect_patterns(candles);
        Ok(ind)
    }

    fn lookup(&self, name: &str) -> Option<Decimal> {
        match name {
            "price" | "close" => Some(self.price),
            "high" => Some(self.high),
            "low" => Some(self.low),
            "open" => Some(self.open),
            "volume" => Some(self.volume),
            "pct_change" => Some(self.pct_change),
            "macd" => self.macd,
            "prev_open" => Some(self.prev_open),
            "prev_close" => Some(self.prev_close),
            "prev_high" => Some(self.prev_high),
            "prev_low" => Some(self.prev_low),
            _ => self.patterns.get(name).copied(),
        }
    }
}

fn sma(closes: &[Decimal], period: usize) -> Option<Decimal> {
    if closes.len() < period { return None; }
    let n = Decimal::from(period);
    Some((closes[closes.len() - period..].iter().sum::<Decimal>() / n).round_dp(10))
}

fn ema(closes: &[Decimal], period: usize) -> Option<Decimal> {
    if closes.len() < period { return None; }
    let k = Decimal::from(2) / Decimal::from(period + 1);
    let k = k.round_dp(10);
    let mut ema = sma(closes, period)?;
    for &c in closes.iter().skip(period) {
        ema = (c * k + ema * (Decimal::ONE - k)).round_dp(10);
    }
    Some(ema)
}

fn rsi(closes: &[Decimal], period: usize) -> Option<Decimal> {
    if closes.len() <= period { return None; }
    let mut gains = Decimal::ZERO;
    let mut losses = Decimal::ZERO;
    let window = &closes[closes.len() - period - 1..];
    for w in window.windows(2) {
        let diff = w[1] - w[0];
        if diff > Decimal::ZERO { gains += diff; } else { losses -= diff; }
    }
    let n = Decimal::from(period);
    let avg_gain = (gains / n).round_dp(10);
    let avg_loss = (losses / n).round_dp(10);
    if avg_loss == Decimal::ZERO {
        return Some(Decimal::from(100));
    }
    let rs = (avg_gain / avg_loss).round_dp(10);
    let hundred = Decimal::from(100);
    Some((hundred - hundred / (Decimal::ONE + rs)).round_dp(4))
}

fn atr(candles: &[Candle], period: usize) -> Option<Decimal> {
    if candles.len() <= period { return None; }
    let mut trs = Vec::with_capacity(period);
    let prev_close = candles[candles.len() - period - 1].close;
    for c in &candles[candles.len() - period..] {
        let h_l = c.high - c.low;
        let h_pc = (c.high - prev_close).abs();
        let l_pc = (c.low - prev_close).abs();
        let tr = h_l.max(h_pc).max(l_pc);
        trs.push(tr);
    }
    Some((trs.iter().sum::<Decimal>() / Decimal::from(period)).round_dp(10))
}

fn macd(closes: &[Decimal]) -> Decimal {
    let fast = ema(closes, 12).unwrap_or(Decimal::ZERO);
    let slow = ema(closes, 26).unwrap_or(Decimal::ZERO);
    (fast - slow).round_dp(10)
}

// ---- Candlestick pattern detection ----
// Based on classic Japanese candlestick analysis. Each pattern returns 1.0
// (present) or 0.0 (absent). Patterns use the last 1-3 candles.

fn detect_patterns(candles: &[Candle]) -> HashMap<String, Decimal> {
    let mut p = HashMap::new();
    if candles.is_empty() { return p; }
    let c = &candles[candles.len() - 1];
    let o = c.open; let h = c.high; let l = c.low; let cl = c.close;
    let body = ((cl - o).abs()).round_dp(10);
    let range = ((h - l).abs()).round_dp(10);
    let upper_shadow = ((h - o.max(cl)).abs()).round_dp(10);
    let lower_shadow = ((o.min(cl) - l).abs()).round_dp(10);
    let two = Decimal::from(2);

    // Guard against zero range.
    if range == Decimal::ZERO {
        p.insert("doji".into(), Decimal::ONE);
        return p;
    }

    let body_pct = (body / range).round_dp(6);
    let upper_pct = (upper_shadow / range).round_dp(6);
    let lower_pct = (lower_shadow / range).round_dp(6);

    // Hammer: long lower shadow (2x+ body), small upper shadow.
    let is_hammer = lower_shadow >= body * two && upper_shadow <= body * Decimal::new(1, 1) && body > Decimal::ZERO;
    p.insert("hammer".into(), bool_dec(is_hammer));

    // Inverted Hammer: long upper shadow, small lower shadow.
    let is_inv_hammer = upper_shadow >= body * two && lower_shadow <= body * Decimal::new(1, 1) && body > Decimal::ZERO;
    p.insert("inverted_hammer".into(), bool_dec(is_inv_hammer));

    // Doji: body is tiny (<=5% of range).
    let is_doji = body_pct <= Decimal::new(5, 2);
    p.insert("doji".into(), bool_dec(is_doji));

    // Dragonfly Doji: doji with long lower shadow (60%+ of range).
    let is_dragonfly = is_doji && lower_pct >= Decimal::new(60, 2);
    p.insert("dragonfly_doji".into(), bool_dec(is_dragonfly));

    // Gravestone Doji: doji with long upper shadow.
    let is_gravestone = is_doji && upper_pct >= Decimal::new(60, 2);
    p.insert("gravestone_doji".into(), bool_dec(is_gravestone));

    // Bullish/Bearish candle.
    p.insert("bullish_candle".into(), bool_dec(cl > o));
    p.insert("bearish_candle".into(), bool_dec(cl < o));

    // Marubozu: body fills 95%+ of range.
    let is_marubozu = body_pct >= Decimal::new(95, 2);
    p.insert("marubozu".into(), bool_dec(is_marubozu));

    // Spinning Top: small body (<=30%), long shadows both sides.
    let is_spinning_top = body_pct <= Decimal::new(30, 2) && upper_shadow > body && lower_shadow > body;
    p.insert("spinning_top".into(), bool_dec(is_spinning_top));

    // Shooting Star: long upper shadow (2x+ body), small lower shadow.
    let is_shooting_star = upper_shadow >= body * two && lower_shadow <= body * Decimal::new(1, 1) && body > Decimal::ZERO;
    p.insert("shooting_star".into(), bool_dec(is_shooting_star));

    p.insert("hanging_man".into(), bool_dec(is_hammer));

    // Long shadows (2/3+ of range).
    p.insert("long_upper_shadow".into(), bool_dec(upper_pct >= Decimal::new(66, 2)));
    p.insert("long_lower_shadow".into(), bool_dec(lower_pct >= Decimal::new(66, 2)));

    // Two-candle patterns.
    if candles.len() >= 2 {
        let prev = &candles[candles.len() - 2];
        let po = prev.open; let pc = prev.close; let ph = prev.high; let pl = prev.low;
        let prev_bullish = pc > po;
        let prev_bearish = pc < po;

        // Bullish Engulfing: prev bearish, curr bullish, curr engulfs prev body.
        let is_bull_engulf = prev_bearish && cl > o && o <= pc && cl >= po;
        p.insert("bullish_engulfing".into(), bool_dec(is_bull_engulf));

        // Bearish Engulfing: prev bullish, curr bearish, curr engulfs prev body.
        let is_bear_engulf = prev_bullish && cl < o && o >= pc && cl <= po;
        p.insert("bearish_engulfing".into(), bool_dec(is_bear_engulf));

        // Bullish Harami: prev big bearish, curr small bullish inside prev.
        let prev_body = ((pc - po).abs()).round_dp(10);
        let is_bull_harami = prev_bearish && prev_body > body * two && cl > o && o >= pc && cl <= po;
        p.insert("bullish_harami".into(), bool_dec(is_bull_harami));

        // Bearish Harami: prev big bullish, curr small bearish inside prev.
        let is_bear_harami = prev_bullish && prev_body > body * two && cl < o && o <= pc && cl >= po;
        p.insert("bearish_harami".into(), bool_dec(is_bear_harami));

        // Piercing Line: prev bearish, curr opens below prev low, closes above prev midpoint.
        let prev_mid = ((po + pc) / two).round_dp(10);
        let is_piercing = prev_bearish && o < pl && cl > prev_mid && cl < po;
        p.insert("piercing_line".into(), bool_dec(is_piercing));

        // Dark Cloud Cover: prev bullish, curr opens above prev high, closes below midpoint.
        let is_dark_cloud = prev_bullish && o > ph && cl < prev_mid && cl > po;
        p.insert("dark_cloud_cover".into(), bool_dec(is_dark_cloud));

        // Tweezer Bottom/Top: matching lows/highs (within 1% of range).
        p.insert("tweezer_bottom".into(), bool_dec(((l - pl).abs() <= range * Decimal::new(1, 2))));
        p.insert("tweezer_top".into(), bool_dec(((h - ph).abs() <= range * Decimal::new(1, 2))));
    }

    // Three-candle patterns.
    if candles.len() >= 3 {
        let prev2 = &candles[candles.len() - 3];
        let prev = &candles[candles.len() - 2];
        let po2 = prev2.open; let pc2 = prev2.close;
        let po = prev.open; let pc = prev.close;

        // Morning Star: bearish → small body → bullish closing into first body.
        let prev_mid = ((po2 + pc2) / two).round_dp(10);
        let is_morning_star = pc2 < po2 && (pc - po).abs() < (pc2 - po2).abs() * Decimal::new(5, 10) && cl > o && cl > prev_mid;
        p.insert("morning_star".into(), bool_dec(is_morning_star));

        // Evening Star: bullish → small body → bearish closing into first body.
        let is_evening_star = pc2 > po2 && (pc - po).abs() < (pc2 - po2).abs() * Decimal::new(5, 10) && cl < o && cl < prev_mid;
        p.insert("evening_star".into(), bool_dec(is_evening_star));

        // Three White Soldiers.
        let is_three_soldiers = pc2 > po2 && pc > po && cl > o && pc > pc2 && cl > pc;
        p.insert("three_white_soldiers".into(), bool_dec(is_three_soldiers));

        // Three Black Crows.
        let is_three_crows = pc2 < po2 && pc < po && cl < o && pc < pc2 && cl < pc;
        p.insert("three_black_crows".into(), bool_dec(is_three_crows));
    }

    p
}

fn bool_dec(b: bool) -> Decimal {
    if b { Decimal::ONE } else { Decimal::ZERO }
}

// ---- Mini expression evaluator ----
// Supports: numbers, function(...), +/-/*//, < <= > >= == !=, and/or/not, parens.

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(Decimal),
    Ident(String),
    LParen, RParen, Comma,
    Lt, Le, Gt, Ge, Eq, Ne,
    And, Or, Not,
    Plus, Minus, Star, Slash,
}

fn tokenize(s: &str) -> AppResult<Vec<Tok>> {
    let mut toks = Vec::new();
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_whitespace() { i += 1; continue; }
        match c {
            '(' => { toks.push(Tok::LParen); i += 1; }
            ')' => { toks.push(Tok::RParen); i += 1; }
            ',' => { toks.push(Tok::Comma); i += 1; }
            '<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == '=' { toks.push(Tok::Le); i += 2; }
                else { toks.push(Tok::Lt); i += 1; }
            }
            '>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == '=' { toks.push(Tok::Ge); i += 2; }
                else { toks.push(Tok::Gt); i += 1; }
            }
            '=' => {
                if i + 1 < bytes.len() && bytes[i + 1] == '=' { toks.push(Tok::Eq); i += 2; }
                else { return Err(AppError::BadRequest("expected '=='".into())); }
            }
            '!' => {
                if i + 1 < bytes.len() && bytes[i + 1] == '=' { toks.push(Tok::Ne); i += 2; }
                else { return Err(AppError::BadRequest("expected '!='".into())); }
            }
            '+' => { toks.push(Tok::Plus); i += 1; }
            '-' => { toks.push(Tok::Minus); i += 1; }
            '*' => { toks.push(Tok::Star); i += 1; }
            '/' => { toks.push(Tok::Slash); i += 1; }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_alphanumeric() || bytes[i] == '_' || bytes[i] == '.') {
                    i += 1;
                }
                let word: String = bytes[start..i].iter().collect();
                match word.as_str() {
                    "and" | "AND" => toks.push(Tok::And),
                    "or" | "OR" => toks.push(Tok::Or),
                    "not" | "NOT" => toks.push(Tok::Not),
                    _ => toks.push(Tok::Ident(word)),
                }
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                    i += 1;
                }
                let num: String = bytes[start..i].iter().collect();
                let d = Decimal::from_str_exact(&num)
                    .map_err(|e| AppError::BadRequest(format!("bad number {num}: {e}")))?;
                toks.push(Tok::Num(d));
            }
            _ => return Err(AppError::BadRequest(format!("unexpected char '{c}'"))),
        }
    }
    Ok(toks)
}

#[derive(Debug, Clone)]
enum Val { Num(Decimal), Bool(bool) }

fn bool_op(v: Val) -> bool {
    match v { Val::Bool(b) => b, Val::Num(n) => n != Decimal::ZERO }
}
fn num_op(v: Val) -> AppResult<Decimal> {
    match v {
        Val::Num(n) => Ok(n),
        Val::Bool(b) => Ok(if b { Decimal::ONE } else { Decimal::ZERO }),
    }
}

pub fn evaluate(expr: &str, ind: &Indicators) -> AppResult<bool> {
    let toks = tokenize(expr)?;
    let mut p = EvalParser { toks: &toks, pos: 0, ind };
    let v = p.parse_or()?;
    Ok(bool_op(v))
}

struct EvalParser<'a> { toks: &'a [Tok], pos: usize, ind: &'a Indicators }

impl<'a> EvalParser<'a> {
    fn peek(&self) -> Option<&Tok> { self.toks.get(self.pos) }
    fn next(&mut self) -> Option<&Tok> {
        let t = self.toks.get(self.pos);
        if t.is_some() { self.pos += 1; }
        t
    }

    fn parse_or(&mut self) -> AppResult<Val> {
        let mut left = self.parse_and()?;
        while let Some(Tok::Or) = self.peek() {
            self.next();
            let right = self.parse_and()?;
            left = Val::Bool(bool_op(left) || bool_op(right));
        }
        Ok(left)
    }
    fn parse_and(&mut self) -> AppResult<Val> {
        let mut left = self.parse_not()?;
        while let Some(Tok::And) = self.peek() {
            self.next();
            let right = self.parse_not()?;
            left = Val::Bool(bool_op(left) && bool_op(right));
        }
        Ok(left)
    }
    fn parse_not(&mut self) -> AppResult<Val> {
        if let Some(Tok::Not) = self.peek() {
            self.next();
            let v = self.parse_not()?;
            return Ok(Val::Bool(!bool_op(v)));
        }
        self.parse_cmp()
    }
    fn parse_cmp(&mut self) -> AppResult<Val> {
        let left = self.parse_add()?;
        let op = match self.peek() {
            Some(t @ (Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge | Tok::Eq | Tok::Ne)) => {
                let t = t.clone(); self.next(); Some(t)
            }
            _ => None,
        };
        let Some(op) = op else { return Ok(left) };
        let right = self.parse_add()?;
        let (a, b) = (num_op(left)?, num_op(right)?);
        let res = match op {
            Tok::Lt => a < b, Tok::Le => a <= b, Tok::Gt => a > b,
            Tok::Ge => a >= b, Tok::Eq => a == b, Tok::Ne => a != b,
            _ => unreachable!(),
        };
        Ok(Val::Bool(res))
    }
    fn parse_add(&mut self) -> AppResult<Val> {
        let mut left = self.parse_mul()?;
        while matches!(self.peek(), Some(Tok::Plus) | Some(Tok::Minus)) {
            let op = self.peek().cloned().unwrap(); self.next();
            let right = self.parse_mul()?;
            let (a, b) = (num_op(left)?, num_op(right)?);
            left = Val::Num(match op { Tok::Plus => a + b, _ => a - b });
        }
        Ok(left)
    }
    fn parse_mul(&mut self) -> AppResult<Val> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek(), Some(Tok::Star) | Some(Tok::Slash)) {
            let op = self.peek().cloned().unwrap(); self.next();
            let right = self.parse_unary()?;
            let (a, b) = (num_op(left)?, num_op(right)?);
            left = Val::Num(match op {
                Tok::Star => a * b,
                _ => { if b == Decimal::ZERO { return Err(AppError::BadRequest("div by zero".into())); } a / b }
            });
        }
        Ok(left)
    }
    fn parse_unary(&mut self) -> AppResult<Val> {
        if let Some(Tok::Minus) = self.peek() {
            self.next();
            let v = self.parse_unary()?;
            return Ok(Val::Num(-num_op(v)?));
        }
        self.parse_atom()
    }
    fn parse_atom(&mut self) -> AppResult<Val> {
        match self.next().cloned() {
            Some(Tok::Num(n)) => Ok(Val::Num(n)),
            Some(Tok::LParen) => {
                let v = self.parse_or()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(v),
                    _ => Err(AppError::BadRequest("expected ')'".into())),
                }
            }
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.next();
                    let mut args: Vec<Decimal> = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            let a = num_op(self.parse_or()?)?;
                            args.push(a);
                            match self.peek() {
                                Some(Tok::Comma) => { self.next(); }
                                Some(Tok::RParen) => break,
                                _ => return Err(AppError::BadRequest("expected ',' or ')'".into())),
                            }
                        }
                    }
                    match self.next() {
                        Some(Tok::RParen) => Ok(Val::Num(resolve_fn(&name, &args, self.ind)?)),
                        _ => Err(AppError::BadRequest("expected ')'".into())),
                    }
                } else {
                    self.ind
                        .lookup(&name)
                        .map(Val::Num)
                        .ok_or_else(|| AppError::BadRequest(format!("unknown identifier: {name}")))
                }
            }
            other => Err(AppError::BadRequest(format!("unexpected token: {other:?}"))),
        }
    }
}

fn resolve_fn(name: &str, args: &[Decimal], ind: &Indicators) -> AppResult<Decimal> {
    match name {
        "rsi" => {
            let p = args.first().map(|d| d.to_string()).unwrap_or_else(|| "14".into());
            ind.rsi.get(&p.parse::<usize>().unwrap_or(14))
                .copied()
                .ok_or_else(|| AppError::BadRequest("rsi not available".into()))
        }
        "ema" => {
            let p = args.first().map(|d| d.to_string()).unwrap_or_else(|| "50".into());
            ind.ema.get(&p.parse::<usize>().unwrap_or(50))
                .copied()
                .ok_or_else(|| AppError::BadRequest("ema not available".into()))
        }
        "sma" => {
            let p = args.first().map(|d| d.to_string()).unwrap_or_else(|| "50".into());
            ind.sma.get(&p.parse::<usize>().unwrap_or(50))
                .copied()
                .ok_or_else(|| AppError::BadRequest("sma not available".into()))
        }
        "atr" => {
            let p = args.first().map(|d| d.to_string()).unwrap_or_else(|| "14".into());
            ind.atr.get(&p.parse::<usize>().unwrap_or(14))
                .copied()
                .ok_or_else(|| AppError::BadRequest("atr not available".into()))
        }
        "macd" => ind.macd.ok_or_else(|| AppError::BadRequest("macd not available".into())),
        "price" | "close" => Ok(ind.close),
        "high" => Ok(ind.high),
        "low" => Ok(ind.low),
        "open" => Ok(ind.open),
        "volume" => Ok(ind.volume),
        "pct_change" => Ok(ind.pct_change),
        "cross" | "crossup" | "crossdown" => {
            // Without historical series per-call we approximate cross as sign comparison.
            let a = args.get(0).copied().unwrap_or(Decimal::ZERO);
            let b = args.get(1).copied().unwrap_or(Decimal::ZERO);
            if name == "crossdown" {
                Ok(if a < b { Decimal::ONE } else { Decimal::ZERO })
            } else {
                Ok(if a > b { Decimal::ONE } else { Decimal::ZERO })
            }
        }
        // Candlestick patterns can be called as functions too: hammer(), doji(), etc.
        _ => {
            if let Some(v) = ind.patterns.get(name) {
                Ok(*v)
            } else {
                Err(AppError::BadRequest(format!("unknown function or pattern: {name}")))
            }
        }
    }
}
