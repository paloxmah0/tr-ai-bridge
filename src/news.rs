//! News & Economic Calendar integration.
//!
//! Fetches upcoming high-impact news events from the free Forex Factory
//! calendar (nfs.faireconomy.media). The AI checks if any high-impact news
//! is imminent (within 30 min) or just released (within 15 min) and adjusts
//! the trade bias accordingly — news creates volatility and uncertainty.

use crate::error::AppResult;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct NewsEvent {
    pub title: String,
    pub country: String,
    pub date: String,
    pub impact: String,
    #[serde(default)]
    pub forecast: String,
    #[serde(default)]
    pub previous: String,
}

/// News impact assessment for the AI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NewsAssessment {
    /// "danger" (high-impact news imminent/released), "caution" (medium impact), "clear" (no news)
    pub status: String,
    pub upcoming_high_impact: Vec<NewsEvent>,
    pub upcoming_medium_impact: Vec<NewsEvent>,
    pub recently_released: Vec<NewsEvent>,
    pub summary: String,
    pub recommendation: String,
}

/// Map a trading symbol to the relevant currency codes.
fn symbol_currencies(symbol: &str) -> Vec<String> {
    let s = symbol.to_uppercase();
    if s.starts_with("FRX") {
        // Forex pair: frxEURUSD -> EUR, USD
        let pair = s.trim_start_matches("FRX");
        if pair.len() >= 6 {
            return vec![pair[..3].to_string(), pair[3..6].to_string()];
        }
    }
    // Deriv synthetic indices are USD-denominated.
    vec!["USD".to_string()]
}

/// Fetch this week's news calendar.
pub async fn fetch_calendar() -> AppResult<Vec<NewsEvent>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| crate::error::AppError::Market(format!("news fetch: {e}")))?;

    let resp = client
        .get("https://nfs.faireconomy.media/ff_calendar_thisweek.json")
        .send()
        .await
        .map_err(|e| crate::error::AppError::Market(format!("news fetch: {e}")))?;

    if !resp.status().is_success() {
        return Ok(Vec::new()); // Don't fail — just return empty.
    }

    let events: Vec<NewsEvent> = resp.json().await.unwrap_or_default();
    Ok(events)
}

/// Assess news impact for a given symbol.
pub async fn assess_news(symbol: &str) -> AppResult<NewsAssessment> {
    let events = fetch_calendar().await.unwrap_or_default();
    let currencies = symbol_currencies(symbol);
    let now = Utc::now();

    let mut upcoming_high: Vec<NewsEvent> = Vec::new();
    let mut upcoming_medium: Vec<NewsEvent> = Vec::new();
    let mut recently_released: Vec<NewsEvent> = Vec::new();

    for event in &events {
        // Only care about events for the relevant currencies.
        if !currencies.iter().any(|c| event.country == *c) {
            continue;
        }

        // Parse the event time (format: "2026-06-24T08:30:00-04:00").
        let event_time = match DateTime::parse_from_rfc3339(&event.date) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => continue,
        };

        let delta = event_time - now;

        if delta > Duration::zero() && delta <= Duration::minutes(30) {
            // Upcoming within 30 minutes.
            if event.impact == "High" {
                upcoming_high.push(event.clone());
            } else if event.impact == "Medium" {
                upcoming_medium.push(event.clone());
            }
        } else if delta < Duration::zero() && delta >= Duration::minutes(-15) {
            // Released within the last 15 minutes.
            recently_released.push(event.clone());
        }
    }

    let status = if !upcoming_high.is_empty() || recently_released.iter().any(|e| e.impact == "High") {
        "danger".to_string()
    } else if !upcoming_medium.is_empty() || recently_released.iter().any(|e| e.impact == "Medium") {
        "caution".to_string()
    } else {
        "clear".to_string()
    };

    let summary = if !upcoming_high.is_empty() {
        format!("HIGH-IMPACT NEWS IMMINENT: {} event(s) in the next 30 min ({})",
            upcoming_high.len(),
            upcoming_high.iter().map(|e| format!("{} ({})", e.title, e.country)).collect::<Vec<_>>().join(", "))
    } else if !recently_released.is_empty() {
        format!("NEWS JUST RELEASED: {} event(s) in the last 15 min ({})",
            recently_released.len(),
            recently_released.iter().map(|e| format!("{} ({})", e.title, e.country)).collect::<Vec<_>>().join(", "))
    } else if !upcoming_medium.is_empty() {
        format!("Medium-impact news upcoming: {} event(s) ({})",
            upcoming_medium.len(),
            upcoming_medium.iter().map(|e| format!("{} ({})", e.title, e.country)).collect::<Vec<_>>().join(", "))
    } else {
        "No high-impact news in the next 30 minutes. Market conditions are clear of news volatility.".to_string()
    };

    let recommendation = match status.as_str() {
        "danger" => "HIGH RISK: High-impact news is imminent or just released. Expect extreme volatility, slippage, and spread widening. REDUCE position size by 50% or WAIT until 15 minutes after the news release. Do not enter new trades during the news window.".to_string(),
        "caution" => "CAUTION: Medium-impact news is upcoming. Expect increased volatility. Use smaller position sizes and wider stops.".to_string(),
        _ => "No news risk. Normal trading conditions. Proceed with standard risk management.".to_string(),
    };

    Ok(NewsAssessment {
        status,
        upcoming_high_impact: upcoming_high,
        upcoming_medium_impact: upcoming_medium,
        recently_released,
        summary,
        recommendation,
    })
}
