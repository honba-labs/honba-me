//! Broker REST historical-candle loaders (Zerodha Kite, Upstox v2).
//!
//! Both brokers answer `{"status":"success","data":{"candles":[[ts,o,h,l,c,v,(oi)], …]}}`;
//! [`parse_candles`] decodes that shape into canonical [`Bar`]s (ascending — Upstox returns
//! newest-first) and is pure, so the wire handling is unit-tested without a network.
//!
//! Both loaders implement [`HistoricalBarSource`], the abstraction the gap reconciler in
//! [`crate::gaps`] backfills through. For both, the broker instrument key is the
//! [`InstrumentId::token`] of the instrument being loaded (Kite: numeric instrument token,
//! Upstox: instrument key such as `NSE_EQ|INE009A01021`).

use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use honba_core::rust_decimal::Decimal;
use honba_core::{Bar, InstrumentId, Price, Quantity};
use serde_json::Value;

use crate::{LoaderError, LoaderResult};

/// A source of historical bars that the gap reconciler can backfill from.
#[async_trait]
pub trait HistoricalBarSource: Send + Sync {
    /// Daily bars for `from..=to` (IST dates).
    async fn daily_bars(
        &self,
        instrument: &InstrumentId,
        from: NaiveDate,
        to: NaiveDate,
    ) -> LoaderResult<Vec<Bar>>;

    /// Intraday bars of `interval_minutes` for the single session `date`.
    async fn intraday_bars(
        &self,
        instrument: &InstrumentId,
        date: NaiveDate,
        interval_minutes: u32,
    ) -> LoaderResult<Vec<Bar>>;
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z"))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn value_dec(v: &Value) -> Option<Decimal> {
    let s = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    Decimal::from_str(&s)
        .or_else(|_| Decimal::from_scientific(&s))
        .ok()
}

/// Decodes a Kite/Upstox candle response body into ascending canonical bars.
///
/// A candle that is not `[timestamp, open, high, low, close, volume, ..]` is an error rather
/// than a silent skip: a short history that looks complete is worse than a loud failure.
pub fn parse_candles(body: &str, instrument: &InstrumentId) -> LoaderResult<Vec<Bar>> {
    let v: Value = serde_json::from_str(body).map_err(|e| LoaderError::Json(e.to_string()))?;
    if v.get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| s != "success")
    {
        return Err(LoaderError::Http(format!(
            "broker reported failure: {}",
            v.get("message")
                .and_then(Value::as_str)
                .unwrap_or("no message")
        )));
    }
    let candles = v
        .pointer("/data/candles")
        .and_then(Value::as_array)
        .ok_or_else(|| LoaderError::Json("response has no data.candles array".into()))?;
    let mut bars = Vec::with_capacity(candles.len());
    for (i, c) in candles.iter().enumerate() {
        let bad = |what: &str| LoaderError::Json(format!("candle {i}: {what}"));
        let a = c
            .as_array()
            .filter(|a| a.len() >= 6)
            .ok_or_else(|| bad("expected >= 6 fields"))?;
        let ts = a[0]
            .as_str()
            .and_then(parse_ts)
            .ok_or_else(|| bad("unparseable timestamp"))?;
        let f = |k: usize, n: &str| value_dec(&a[k]).ok_or_else(|| bad(&format!("bad {n}")));
        bars.push(Bar {
            instrument: instrument.clone(),
            open: Price(f(1, "open")?),
            high: Price(f(2, "high")?),
            low: Price(f(3, "low")?),
            close: Price(f(4, "close")?),
            volume: Quantity(f(5, "volume")?),
            timestamp: ts,
        });
    }
    bars.sort_by_key(|b| b.timestamp);
    Ok(bars)
}

async fn get_body(req: reqwest::RequestBuilder, what: &str) -> LoaderResult<String> {
    let resp = req
        .send()
        .await
        .map_err(|e| LoaderError::Http(format!("{what}: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| LoaderError::Http(format!("{what}: {e}")))?;
    if !status.is_success() {
        return Err(LoaderError::Http(format!("{what}: HTTP {status}: {body}")));
    }
    Ok(body)
}

fn token_of(instrument: &InstrumentId) -> LoaderResult<&str> {
    instrument.token.as_deref().ok_or_else(|| {
        LoaderError::NotFound(format!("{instrument} has no broker token / instrument key"))
    })
}

/// Zerodha Kite historical-candle loader.
#[derive(Debug, Clone)]
pub struct KiteCandlesLoader {
    /// Kite Connect API key.
    pub api_key: String,
    /// Session access token.
    pub access_token: String,
    /// REST base URL.
    pub base_url: String,
    /// HTTP client.
    pub client: reqwest::Client,
}

impl KiteCandlesLoader {
    /// A loader against `https://api.kite.trade`.
    pub fn new(api_key: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            access_token: access_token.into(),
            base_url: "https://api.kite.trade".into(),
            client: reqwest::Client::new(),
        }
    }

    /// Kite interval name for a minute count (`minute`, `5minute`, …, `day`).
    pub fn interval_name(minutes: u32) -> LoaderResult<&'static str> {
        Ok(match minutes {
            1 => "minute",
            3 => "3minute",
            5 => "5minute",
            10 => "10minute",
            15 => "15minute",
            30 => "30minute",
            60 => "60minute",
            1440 => "day",
            m => {
                return Err(LoaderError::Unsupported(format!(
                    "Kite has no {m}-minute interval"
                )));
            }
        })
    }

    /// Downloads candles for `interval` (`minute`, `5minute`, `day`, …) over `from..=to`.
    pub async fn download_candles(
        &self,
        instrument: &InstrumentId,
        interval: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> LoaderResult<Vec<Bar>> {
        let token = token_of(instrument)?;
        let url = format!(
            "{}/instruments/historical/{token}/{interval}",
            self.base_url
        );
        let req = self
            .client
            .get(&url)
            .header("X-Kite-Version", "3")
            .header(
                "Authorization",
                format!("token {}:{}", self.api_key, self.access_token),
            )
            .query(&[
                ("from", format!("{from} 00:00:00")),
                ("to", format!("{to} 23:59:59")),
            ]);
        parse_candles(&get_body(req, "Kite candles").await?, instrument)
    }
}

#[async_trait]
impl HistoricalBarSource for KiteCandlesLoader {
    async fn daily_bars(
        &self,
        i: &InstrumentId,
        from: NaiveDate,
        to: NaiveDate,
    ) -> LoaderResult<Vec<Bar>> {
        self.download_candles(i, "day", from, to).await
    }

    async fn intraday_bars(
        &self,
        i: &InstrumentId,
        date: NaiveDate,
        minutes: u32,
    ) -> LoaderResult<Vec<Bar>> {
        self.download_candles(i, Self::interval_name(minutes)?, date, date)
            .await
    }
}

/// Upstox v2 historical-candle loader.
#[derive(Debug, Clone)]
pub struct UpstoxCandlesLoader {
    /// OAuth access token.
    pub access_token: String,
    /// REST base URL.
    pub base_url: String,
    /// HTTP client.
    pub client: reqwest::Client,
}

impl UpstoxCandlesLoader {
    /// A loader against `https://api.upstox.com`.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            base_url: "https://api.upstox.com".into(),
            client: reqwest::Client::new(),
        }
    }

    /// Upstox v2 interval name: only `1minute`, `30minute`, `day` are available intraday/daily.
    pub fn interval_name(minutes: u32) -> LoaderResult<&'static str> {
        Ok(match minutes {
            1 => "1minute",
            30 => "30minute",
            1440 => "day",
            m => {
                return Err(LoaderError::Unsupported(format!(
                    "Upstox v2 has no {m}-minute interval"
                )));
            }
        })
    }

    /// URL for a request. Upstox takes `to_date` **before** `from_date`, and the instrument
    /// key's `|` must be percent-encoded.
    pub fn candles_url(
        &self,
        instrument_key: &str,
        interval: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> String {
        format!(
            "{}/v2/historical-candle/{}/{interval}/{to}/{from}",
            self.base_url,
            instrument_key.replace('|', "%7C")
        )
    }

    /// Downloads candles for `interval` over `from..=to`.
    pub async fn download_candles(
        &self,
        instrument: &InstrumentId,
        interval: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> LoaderResult<Vec<Bar>> {
        let url = self.candles_url(token_of(instrument)?, interval, from, to);
        let req = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .header("Accept", "application/json");
        parse_candles(&get_body(req, "Upstox candles").await?, instrument)
    }
}

#[async_trait]
impl HistoricalBarSource for UpstoxCandlesLoader {
    async fn daily_bars(
        &self,
        i: &InstrumentId,
        from: NaiveDate,
        to: NaiveDate,
    ) -> LoaderResult<Vec<Bar>> {
        self.download_candles(i, "day", from, to).await
    }

    async fn intraday_bars(
        &self,
        i: &InstrumentId,
        date: NaiveDate,
        minutes: u32,
    ) -> LoaderResult<Vec<Bar>> {
        self.download_candles(i, Self::interval_name(minutes)?, date, date)
            .await
    }
}
