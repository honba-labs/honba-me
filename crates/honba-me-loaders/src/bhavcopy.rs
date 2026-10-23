//! NSE daily Bhavcopy and delivery-position loaders.
//!
//! * **Bhavcopy** — `sec_bhavdata_full_DDMMYYYY.csv` (a.k.a. `SEC_BHAVDATA_V2`): per-scrip OHLC,
//!   previous close, average (VWAP), total traded quantity, trade count, and — in the full
//!   file — deliverable quantity and percentage.
//! * **Delivery report** — `MTO_DDMMYYYY.DAT`: the standalone delivery-position file, kept
//!   separate because it covers more series and can be fetched/merged independently.
//!
//! Parsing is pure and header-driven (column order may change; names are what NSE guarantees),
//! and all money/quantities are exact [`Decimal`]s.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use honba_core::rust_decimal::Decimal;
use honba_core::rust_decimal::prelude::ToPrimitive;
use honba_core::{Bar, Exchange, InstrumentId, Price, Quantity};

use crate::{LoaderError, LoaderResult, ist};

/// One scrip's Bhavcopy row.
#[derive(Debug, Clone, PartialEq)]
pub struct BhavRecord {
    /// Trading date of the row.
    pub date: NaiveDate,
    /// Exchange series (`EQ`, `BE`, `SM`, …).
    pub series: String,
    /// The daily bar; `timestamp` is the session close (15:30 IST).
    pub bar: Bar,
    /// Previous session close.
    pub prev_close: Option<Decimal>,
    /// Volume-weighted average price for the day (`AVG_PRICE`).
    pub vwap: Option<Decimal>,
    /// Number of trades.
    pub trades: Option<u64>,
    /// Deliverable quantity.
    pub deliverable_qty: Option<Decimal>,
    /// Deliverable percentage of traded quantity (0–100).
    pub deliverable_pct: Option<Decimal>,
}

/// One row of the `MTO_*.DAT` delivery-position report.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryRecord {
    /// Trading symbol.
    pub symbol: String,
    /// Series (`EQ`, …).
    pub series: String,
    /// Quantity traded.
    pub traded_qty: Decimal,
    /// Deliverable quantity (gross across client level).
    pub deliverable_qty: Decimal,
    /// Deliverable % of traded quantity.
    pub deliverable_pct: Decimal,
}

/// Session-close instant (15:30 IST) of `date`, in UTC.
pub fn session_close_utc(date: NaiveDate) -> DateTime<Utc> {
    let t = NaiveTime::from_hms_opt(15, 30, 0).expect("valid time");
    ist()
        .from_local_datetime(&date.and_time(t))
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc)
}

/// URL of the full Bhavcopy for `date`.
pub fn bhavcopy_url(date: NaiveDate) -> String {
    format!(
        "https://nsearchives.nseindia.com/products/content/sec_bhavdata_full_{}.csv",
        date.format("%d%m%Y")
    )
}

/// URL of the delivery-position report for `date`.
pub fn delivery_url(date: NaiveDate) -> String {
    format!(
        "https://nsearchives.nseindia.com/archives/equities/mto/MTO_{}.DAT",
        date.format("%d%m%Y")
    )
}

/// Parses an NSE numeric cell; empty and `-` (NSE's "n/a") are `None`.
fn num(s: &str) -> Option<Decimal> {
    let s = s.trim();
    if s.is_empty() || s == "-" {
        return None;
    }
    Decimal::from_str(s).ok()
}

/// Parses Bhavcopy CSV text. Rows lacking any OHLC/volume value (suspended scrips print `-`)
/// are skipped; a missing required *column* is an error.
pub fn parse_bhavcopy_csv(
    csv_text: &str,
    fallback_date: NaiveDate,
) -> LoaderResult<Vec<BhavRecord>> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(csv_text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| LoaderError::Csv(e.to_string()))?
        .clone();
    let idx: HashMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_uppercase(), i))
        .collect();
    let need = |name: &str| {
        idx.get(name)
            .copied()
            .ok_or_else(|| LoaderError::Csv(format!("Bhavcopy is missing column {name}")))
    };
    let (c_sym, c_series) = (need("SYMBOL")?, need("SERIES")?);
    let (c_open, c_high, c_low, c_close, c_qty) = (
        need("OPEN_PRICE")?,
        need("HIGH_PRICE")?,
        need("LOW_PRICE")?,
        need("CLOSE_PRICE")?,
        need("TTL_TRD_QNTY")?,
    );
    let opt = |name: &str| idx.get(name).copied();
    let (c_date, c_prev, c_avg, c_trades, c_dq, c_dp) = (
        opt("DATE1"),
        opt("PREV_CLOSE"),
        opt("AVG_PRICE"),
        opt("NO_OF_TRADES"),
        opt("DELIV_QTY"),
        opt("DELIV_PER"),
    );

    let mut out = Vec::new();
    for rec in reader.records() {
        let rec = rec.map_err(|e| LoaderError::Csv(e.to_string()))?;
        let cell = |i: usize| rec.get(i).unwrap_or("");
        let symbol = cell(c_sym);
        if symbol.is_empty() {
            continue;
        }
        let (Some(open), Some(high), Some(low), Some(close), Some(qty)) = (
            num(cell(c_open)),
            num(cell(c_high)),
            num(cell(c_low)),
            num(cell(c_close)),
            num(cell(c_qty)),
        ) else {
            continue;
        };
        let date = c_date
            .and_then(|i| NaiveDate::parse_from_str(cell(i), "%d-%b-%Y").ok())
            .unwrap_or(fallback_date);
        let get = |c: Option<usize>| c.and_then(|i| num(cell(i)));
        out.push(BhavRecord {
            date,
            series: cell(c_series).to_string(),
            bar: Bar {
                instrument: InstrumentId::new(Exchange::Nse, symbol),
                open: Price(open),
                high: Price(high),
                low: Price(low),
                close: Price(close),
                volume: Quantity(qty),
                timestamp: session_close_utc(date),
            },
            prev_close: get(c_prev),
            vwap: get(c_avg),
            trades: get(c_trades).and_then(|d| d.trunc().to_u64()),
            deliverable_qty: get(c_dq),
            deliverable_pct: get(c_dp),
        });
    }
    Ok(out)
}

/// Parses an `MTO_*.DAT` report: header lines are skipped; data lines start with record type
/// `20` and read `20,SrNo,Symbol,Series,TradedQty,DeliverableQty,Deliverable%`.
pub fn parse_delivery_dat(text: &str) -> Vec<DeliveryRecord> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 7 || f[0] != "20" {
                return None;
            }
            Some(DeliveryRecord {
                symbol: f[2].to_string(),
                series: f[3].to_string(),
                traded_qty: num(f[4])?,
                deliverable_qty: num(f[5])?,
                deliverable_pct: num(f[6])?,
            })
        })
        .collect()
}

/// Fills `deliverable_qty` / `deliverable_pct` on records that lack them, matching on
/// `(symbol, series)`. Returns how many records were filled.
pub fn merge_delivery(records: &mut [BhavRecord], delivery: &[DeliveryRecord]) -> usize {
    let by_key: HashMap<(&str, &str), &DeliveryRecord> = delivery
        .iter()
        .map(|d| ((d.symbol.as_str(), d.series.as_str()), d))
        .collect();
    let mut filled = 0;
    for r in records {
        if r.deliverable_qty.is_some() && r.deliverable_pct.is_some() {
            continue;
        }
        if let Some(d) = by_key.get(&(r.bar.instrument.symbol.as_str(), r.series.as_str())) {
            r.deliverable_qty = Some(d.deliverable_qty);
            r.deliverable_pct = Some(d.deliverable_pct);
            filled += 1;
        }
    }
    filled
}

/// Downloads NSE Bhavcopy and delivery files.
///
/// NSE rejects clients without a browser-like `User-Agent`, which this sets.
#[derive(Debug, Clone)]
pub struct BhavcopyDownloader {
    /// HTTP client.
    pub client: reqwest::Client,
    /// Overrides [`bhavcopy_url`] (tests / mirrors). `{ddmmyyyy}` is substituted.
    pub bhavcopy_template: Option<String>,
    /// Overrides [`delivery_url`]. `{ddmmyyyy}` is substituted.
    pub delivery_template: Option<String>,
}

impl Default for BhavcopyDownloader {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) honba-me-loaders/0.1")
                .build()
                .unwrap_or_default(),
            bhavcopy_template: None,
            delivery_template: None,
        }
    }
}

impl BhavcopyDownloader {
    /// A downloader pointed at NSE.
    pub fn new() -> Self {
        Self::default()
    }

    async fn fetch(&self, url: &str, what: &str) -> LoaderResult<String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| LoaderError::Http(format!("{what}: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(LoaderError::NotFound(format!(
                "{what} not published at {url} (holiday or not yet available)"
            )));
        }
        if !status.is_success() {
            return Err(LoaderError::Http(format!("{what}: HTTP {status}")));
        }
        resp.text()
            .await
            .map_err(|e| LoaderError::Http(format!("{what}: {e}")))
    }

    fn url(template: &Option<String>, default: String, date: NaiveDate) -> String {
        match template {
            Some(t) => t.replace("{ddmmyyyy}", &date.format("%d%m%Y").to_string()),
            None => default,
        }
    }

    /// Downloads and parses the Bhavcopy for `date`.
    pub async fn download_daily(&self, date: NaiveDate) -> LoaderResult<Vec<BhavRecord>> {
        let url = Self::url(&self.bhavcopy_template, bhavcopy_url(date), date);
        parse_bhavcopy_csv(&self.fetch(&url, "Bhavcopy").await?, date)
    }

    /// Downloads and parses the delivery-position report for `date`.
    pub async fn download_delivery(&self, date: NaiveDate) -> LoaderResult<Vec<DeliveryRecord>> {
        let url = Self::url(&self.delivery_template, delivery_url(date), date);
        Ok(parse_delivery_dat(
            &self.fetch(&url, "delivery report").await?,
        ))
    }

    /// Bhavcopy with delivery data merged in from the MTO report (a missing MTO file is not an
    /// error: the Bhavcopy's own delivery columns are kept).
    pub async fn download_daily_with_delivery(
        &self,
        date: NaiveDate,
    ) -> LoaderResult<Vec<BhavRecord>> {
        let mut records = self.download_daily(date).await?;
        match self.download_delivery(date).await {
            Ok(d) => {
                merge_delivery(&mut records, &d);
            }
            Err(LoaderError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        Ok(records)
    }
}

/// Just the bars of a set of records (`EQ` series only when `eq_only`).
pub fn bars_of(records: &[BhavRecord], eq_only: bool) -> Vec<Bar> {
    records
        .iter()
        .filter(|r| !eq_only || r.series == "EQ")
        .map(|r| r.bar.clone())
        .collect()
}
