//! NSE bhavcopy connector (ADR 0028): the first Rust-native market-data source.
//!
//! A bhavcopy is a static end-of-day file, so the connector is deliberately
//! simple: it parses the whole CSV on construction into a `Vec` of
//! [`MarketEvent::Bar`]s, [`subscribe`](BhavcopyConnector::subscribe)s to a
//! set of instruments, and replays the bars in file order. There are no gaps,
//! no reconnects and no retriable failures — only construction can fail.

use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::path::Path;

use async_trait::async_trait;
use chrono::{FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};
use honba_ports::{
    ConnectorState, MarketDataConnector, MarketEvent, PortResult, Subscription, SubscriptionParams,
};
use thiserror::Error;

/// A failure opening, reading or decoding a bhavcopy file.
///
/// An unrecognised header shape is *not* an error: the connector constructs
/// successfully and simply delivers no bars.
#[derive(Debug, Error)]
pub enum BhavcopyError {
    /// The file could not be opened or read.
    #[error("I/O reading bhavcopy: {0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid UTF-8.
    #[error("bhavcopy UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// A [`MarketDataConnector`] that replays an NSE bhavcopy as daily bars.
///
/// Call [`from_csv`](BhavcopyConnector::from_csv) or
/// [`from_path`](BhavcopyConnector::from_path) to parse a file; both eagerly
/// parse every equity row (series `EQ`/`BE`/`BZ`) into one 1-day
/// [`Bar`] stamped at 09:15 IST on the row's trade date.
pub struct BhavcopyConnector {
    bars: Vec<MarketEvent>,
    queue: VecDeque<MarketEvent>,
    state: ConnectorState,
}

impl BhavcopyConnector {
    /// Parses a bhavcopy CSV into a connector, attributing every bar to `exchange`.
    ///
    /// Only I/O and UTF-8 failures are returned: an unknown header shape yields
    /// a connector that delivers no bars.
    pub fn from_csv(mut csv: impl BufRead, exchange: Exchange) -> Result<Self, BhavcopyError> {
        let bars = parse(&mut csv, &exchange)?;
        Ok(Self {
            bars,
            queue: VecDeque::new(),
            state: ConnectorState::Disconnected {
                reason: "not subscribed".into(),
            },
        })
    }

    /// Opens `path` and delegates to [`from_csv`](BhavcopyConnector::from_csv).
    pub fn from_path(path: impl AsRef<Path>, exchange: Exchange) -> Result<Self, BhavcopyError> {
        let file = std::fs::File::open(path)?;
        Self::from_csv(std::io::BufReader::new(file), exchange)
    }
}

#[async_trait]
impl MarketDataConnector for BhavcopyConnector {
    async fn subscribe(&mut self, params: SubscriptionParams) -> PortResult<Subscription> {
        let requested = params.symbols().to_vec();
        self.state = ConnectorState::Connected;
        if requested.is_empty() {
            self.queue = self.bars.iter().cloned().collect();
            let mut ids: Vec<InstrumentId> = Vec::new();
            for event in &self.bars {
                if let MarketEvent::Bar(bar) = event {
                    let id = bar.bar_type().instrument_id().clone();
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            Ok(Subscription::new(ids))
        } else {
            self.queue = self
                .bars
                .iter()
                .filter(|event| match event {
                    MarketEvent::Bar(bar) => requested.contains(bar.bar_type().instrument_id()),
                    _ => false,
                })
                .cloned()
                .collect();
            Ok(Subscription::new(requested))
        }
    }

    async fn next_event(&mut self) -> PortResult<Option<MarketEvent>> {
        Ok(self.queue.pop_front())
    }

    fn state(&self) -> ConnectorState {
        self.state.clone()
    }
}

fn parse(csv: &mut impl BufRead, exchange: &Exchange) -> Result<Vec<MarketEvent>, BhavcopyError> {
    let mut bytes = Vec::new();
    csv.read_to_end(&mut bytes)?;
    let text = String::from_utf8(bytes)?;

    let mut reader = ::csv::ReaderBuilder::new()
        .flexible(true)
        .trim(::csv::Trim::All)
        .from_reader(text.as_bytes());
    let headers = match reader.headers() {
        Ok(headers) => headers.clone(),
        Err(_) => return Ok(Vec::new()),
    };

    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, name) in headers.iter().enumerate() {
        index.insert(name.to_uppercase(), i);
    }

    let sym_col = column(&index, &["TCKRSYMB", "SYMBOL"]);
    let series_col = column(&index, &["SCTYSRS", "SERIES"]);
    let open_col = column(&index, &["OPNPRIC", "OPEN_PRICE", "OPEN"]);
    let high_col = column(&index, &["HGHPRIC", "HIGH_PRICE", "HIGH"]);
    let low_col = column(&index, &["LWPRIC", "LOW_PRICE", "LOW"]);
    let close_col = column(&index, &["CLSPRIC", "CLOSE_PRICE", "CLOSE"]);
    let qty_col = column(
        &index,
        &["TTLTRADGVOL", "TTLTRADQTY", "TTL_TRD_QNTY", "TOTTRDQTY"],
    );
    let date_col = column(&index, &["TRADDT", "DATE1", "TIMESTAMP"]);

    let (
        Some(sym_col),
        Some(open_col),
        Some(high_col),
        Some(low_col),
        Some(close_col),
        Some(qty_col),
    ) = (sym_col, open_col, high_col, low_col, close_col, qty_col)
    else {
        return Ok(Vec::new());
    };

    let mut bars: Vec<MarketEvent> = Vec::new();
    for record in reader.records().flatten() {
        let field = |c: usize| record.get(c).unwrap_or("").trim();

        let series = match series_col {
            Some(c) => field(c).to_uppercase(),
            None => "EQ".to_string(),
        };
        if !matches!(series.as_str(), "EQ" | "BE" | "BZ") {
            continue;
        }

        let symbol = field(sym_col);
        if symbol.is_empty() {
            continue;
        }

        let (Some(op), Some(hi), Some(lo), Some(cl)) = (
            price(field(open_col)),
            price(field(high_col)),
            price(field(low_col)),
            price(field(close_col)),
        ) else {
            continue;
        };

        let volume = field(qty_col)
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map_or(0.0, |v| v.max(0.0));

        let date_raw = match date_col {
            Some(c) => field(c),
            None => "",
        };
        let Some(ts) = open_ts(date_raw) else {
            continue;
        };

        let bar_type = BarType::new(
            InstrumentId::new(symbol, exchange.clone()),
            BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
        );
        let high = hi.max(op).max(cl);
        let low = lo.min(op).min(cl);
        bars.push(MarketEvent::Bar(Bar::new(
            bar_type, op, high, low, cl, volume, ts, ts,
        )));
    }

    Ok(bars)
}

fn column(index: &HashMap<String, usize>, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|name| index.get(*name).copied())
}

fn price(raw: &str) -> Option<f64> {
    raw.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn open_ts(raw: &str) -> Option<UnixNanos> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let date = ["%Y-%m-%d", "%d-%b-%Y", "%d-%b-%y"]
        .iter()
        .find_map(|fmt| NaiveDate::parse_from_str(raw, fmt).ok())?;
    let naive = NaiveDateTime::new(date, NaiveTime::from_hms_opt(9, 15, 0)?);
    let ist = FixedOffset::east_opt(330 * 60)?;
    let instant = naive.and_local_timezone(ist).single()?;
    let secs = u64::try_from(instant.timestamp()).ok()?;
    Some(UnixNanos::from_u64(secs * 1_000_000_000))
}
