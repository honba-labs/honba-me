# 本場・目 honba-me

> *The eye of the real market.*

Real-time and historical market data for NSE/BSE, decoupled from execution so you can observe on one broker and act on another.

## Crates
- `honba-me-core` — MarketDataFeed, Tick, Subscription, WebSocket client, coalescing buffer
- `honba-me-{zerodha,upstox,fyers,angelone}` — broker wire decoders and live feeds
- `honba-me-normalizer` — instrument master, symbol mapping, corporate actions
- `honba-me-feed` — Parquet **lakehouse v2** (`Lakehouse`), live `TickRecorder`, legacy readers/exporters
- `honba-me-loaders` — NSE Bhavcopy/delivery, Kite & Upstox historical candles, gap backfill, DuckDB views
- `honba-me-indicators`, `honba-me-signals`, `honba-me-market`

## Parquet lakehouse (Phase 4)

Canonical `honba_core::Tick` / `Bar` stored with exact `Decimal128(18, 4)` prices in a Hive-partitioned tree
(`date` / `year` / `month` are IST):

```text
{root}/ticks/exchange=NSE/date=2026-10-20/INFY.parquet        (+ INFY.part-0001.parquet, …)
{root}/bars/timeframe=1m/exchange=NSE/year=2026/month=10/INFY.parquet
```

```rust
use honba_feed::{Lakehouse, RecorderConfig, TickRecorder};

let lake = Lakehouse::new("honba-data-store");

// Live capture: flushes every 100k ticks or 60 s, Zstd-compressed.
let rec = TickRecorder::spawn(lake.clone(), RecorderConfig::default());
rec.record(tick).await;
let stats = rec.shutdown().await?;

// Canonical reads (also implements honba_core::{TickReader, BarReader}).
let ticks = lake.read_ticks_range(&instrument, from, to)?;
```

Backfill gaps from a broker and query with DuckDB:

```rust
use honba_loaders::{reconcile_daily, DuckDbViews, KiteCandlesLoader};

let kite = KiteCandlesLoader::new(api_key, access_token); // instrument.token = Kite token
reconcile_daily(&lake, &kite, &instrument, from, to, &holidays).await?;

DuckDbViews::new("honba-data-store").write_script("views.sql".as_ref())?;
// duckdb -init views.sql -c "SELECT symbol, avg(price) FROM ticks WHERE date = '2026-10-20' GROUP BY 1"
```

Legacy `f64` tick stores migrate with `honba_feed::canonical::migrate_tick_store`.

License: AGPL-3.0.
