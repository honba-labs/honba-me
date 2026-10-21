# Master Plan: 本場・目 honba-me

> **The eye of the real market.**  
> High-performance, institutional-grade market data engine for Indian equities and derivatives (NSE/BSE/NFO), decoupled from execution so traders can observe on one broker and act on another. Provides canonical real-time WebSocket feeds, resilient multi-broker ingestion (Zerodha Kite, Upstox v2, Fyers v3, Angel One SmartAPI), historical Parquet lakehouse storage, symbol & corporate actions normalization, incremental technical indicators, and explainable signal confluence.

---

## 0. Executive Summary & Architectural Audit

### 0.1 Current State vs Target Architecture

`honba-me` was originally sketched as a multi-broker feed and indicators layer. An in-depth audit of the workspace reveals major architectural divergences, half-implemented stubs, and legacy debt that must be resolved:

| Area | Current State in `honba-me` | Target State (Latest Architecture) | Severity |
| :--- | :--- | :--- | :--- |
| **Domain Types** | Split split between `honba-core` (in `me-core`), legacy `honba-messages`/`entities` with `f64` (in `feed`, `market`, `signals`), and raw `f64` (in `indicators`). | **100% unified on `honba-core`** (`InstrumentId`, `Price`, `Quantity`, `Tick`, `Bar`, `PriceBand`) with `rust_decimal::Decimal`. | **P0 (Critical)** |
| **Broker WebSockets** | **Stubs only**: `subscribe()` creates an empty buffer and drops it. No real WebSocket client (`tokio-tungstenite`), ping/pong, or reconnect loop. | **Production async WebSocket client** with TLS (`rustls`), heartbeat monitors, exponential backoff with full jitter, and resubscription state machines. | **P0 (Critical)** |
| **Wire Protocol Parsing** | Zerodha only parses 8B LTP and 28B Quote (missing 44B Quote & 184B Full Depth); Upstox parses dummy JSON (real Upstox v2 is binary Protobuf); Fyers parses dummy JSON. | Full binary decoders: **Zerodha Kite Ticker** (LTP, Quote, Full 5-depth, zlib decompression), **Upstox v2 Protobuf**, **Fyers v3 binary Data Socket**, **Angel One SmartAPI 2.0**. | **P0 (Critical)** |
| **Stream Resilience** | `CoalescingTickBuffer` simply drops ticks when full; `ReconnectionManager` tracks state in-memory with no REST snapshot reconciliation. | **True tick coalescing** (last-value-cache per instrument during backpressure bursts) + REST snapshot reconciliation on reconnect (Kite `/quote`, Upstox `/market-quote`). | **P1 (High)** |
| **Price Band Enrichment** | Incoming ticks carry `band: None`. Ticks are not enriched before dispatching to `honba-mon::RiskEngine`. | Real-time enrichment pipeline attaching scrip-wise circuit bands (2%, 5%, 10%, 20%) and VWAP to quotes for pre-trade risk evaluation. | **P0 (Critical)** |
| **Normalizer & Master Data** | `honba-me-normalizer` is 20 lines of heuristic symbol string parsing. No instrument master database or corporate actions. | Daily automated master ingestion (NSE bhavcopy, Zerodha `instruments.csv`, Upstox `complete.json.gz`), ISIN lookup, split/bonus/dividend adjustments. | **P1 (High)** |
| **Historical & Feed Store** | `honba-me-feed` is coupled to legacy `honba-ports` / `honba-messages`, uses `Float64` Arrow columns, and lacks partitioned live capture. | **Parquet Lakehouse v2**: Arrow 53 with `Decimal128` exact financial types, Hive partitioning (`date=.../exchange=...`), and a live `TickRecorder` daemon. | **P1 (High)** |
| **Technical Indicators** | 9 basic indicators in `f64`. Missing Indian retail/institutional staples: Supertrend, VWAP, CPR, ADX, RMA, PSAR, Ichimoku. | Expanded suite with **Supertrend, VWAP + bands, CPR, ADX, RMA, PSAR, OBV**, dual-mode API (`honba_core::Bar` vs `f64`), vectorized Arrow batch compute, and PyO3 exports. | **P1 (High)** |
| **Signal Engine** | Day & Swing horizons using legacy bars. Missing intraday momentum & opening range breakout detectors. | Modernized on `honba_core::Bar` with **Intraday detectors** (VWAP breakout, CPR breakout, ORB 15/30m, volume spikes), ATR-based stop-loss/target tagging. | **P2 (Medium)** |

---

## 1. Architectural Principles & System Integration

```text
                                  ┌────────────────────────────────┐
                                  │   Indian Market Live Feeds     │
                                  │ (Zerodha, Upstox, Fyers, Angel)│
                                  └───────────────┬────────────────┘
                                                  │ Binary WebSocket / Protobuf
                                                  ▼
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ 本場・目 honba-me Subsystem                                                                  │
│                                                                                              │
│  ┌───────────────────────┐   ┌───────────────────────┐   ┌────────────────────────────────┐  │
│  │ Wire Protocol Parsers │──▶│   Symbol Normalizer   │──▶│     Market Data Enricher       │  │
│  │ (LTP, Quote, 5-Depth) │   │ (Token ➔ InstrumentId)│   │ (Price Bands, Tick Size, VWAP) │  │
│  └───────────────────────┘   └───────────────────────┘   └───────────────┬────────────────┘  │
│                                                                          │                   │
│  ┌───────────────────────────────────────────────────────────────────────┴────────────────┐  │
│  │                     Resilient Coalescing Pipeline (Bounded MPSC + LVC)                 │  │
│  └───────────────────────────────────────┬────────────────────────────────────────────────┘  │
│                                          │                                                   │
│                 ┌────────────────────────┴────────────────────────┬────────────────────────┐ │
│                 ▼                                                 ▼                        ▼ │
│  ┌───────────────────────────────┐               ┌─────────────────────────┐  ┌────────────┴┐│
│  │ Live Stream Broadcast (Tokio) │               │   Tick & Bar Recorder   │  │ Incremental ││
│  │  ➔ honba-ya MarketDataActor   │               │   (Hive Parquet / Arrow)│  │ Indicators &││
│  │  ➔ honba-kao Web Terminal WS  │               │   ➔ honba-data Lakehouse│  │ Signals     ││
│  └───────────────────────────────┘               └─────────────────────────┘  └─────────────┘│
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Key Tenets
1. **Observation-Execution Decoupling**: Market data ingestion runs strictly isolated from order routing (`honba-te`). A strategy can subscribe to a low-latency Kite WebSocket, run alpha calculations in `honba-me`, and route resulting orders via Upstox or Angel One.
2. **Canonical Type Purity**: Zero floating point values for prices and quantities across external and internal domain interfaces (`rust_decimal::Decimal`). Timestamps strictly in `DateTime<Utc>`, formatted to IST (`Asia/Kolkata`) only at the presentation boundary.
3. **Research-Live Parity**: The same indicator algorithms, signal detectors, and Bhavcopy historical readers used in backtesting are executed in the live trading loop.
4. **Deterministic Ingestion & Monotonicity**: Out-of-sequence or stale ticks are dropped at the door. Top-of-book quotes coalesce under backpressure so consumers never lag behind the market.
5. **Regulatory Alignment (SEBI)**: Scrip price bands (2%, 5%, 10%, 20%) and index cooling-off circuit halt triggers are enriched into market events before reaching `honba-mon::RiskEngine`.

---

## 2. Crate Topology & Dependency Rules

The target workspace for `honba-me` organizes into 10 specialized crates:

```text
honba-me/
├── Cargo.toml
├── README.md
├── plan.md
└── crates/
    ├── honba-me-core/          # MarketDataFeed trait, SymbolMapper, CoalescingBuffer, Reconnection
    ├── honba-me-zerodha/       # Kite Ticker binary WebSocket parser (8B, 28B, 44B, 184B) & client
    ├── honba-me-upstox/        # Upstox v2 binary Protobuf WebSocket parser & client
    ├── honba-me-fyers/         # Fyers v3 Data Socket binary/JSON parser & client
    ├── honba-me-angelone/      # Angel One SmartAPI WebSocket 2.0 binary parser & client [NEW]
    ├── honba-me-normalizer/    # Master instrument catalogs, token mapping, corporate actions engine
    ├── honba-me-feed/          # Parquet tick & bar lakehouse, Arrow 53 datasets, report exporters
    ├── honba-me-loaders/       # NSE Bhavcopy ingest, Yahoo/AlphaVantage loaders, gap reconciler [NEW]
    ├── honba-me-indicators/    # Incremental & batch technical indicators, screener predicate engine
    └── honba-me-signals/       # Confluence detector, intraday/swing rulesets, trade target tagger
```

### Layering & Dependency Hierarchy
```text
Layer 4: [honba-me-signals]
             │
Layer 3: [honba-me-indicators]
             │
Layer 2: [honba-me-feed] ──▶ [honba-me-loaders]
             │
Layer 1: [honba-me-normalizer] ──▶ [honba-me-zerodha, upstox, fyers, angelone]
             │
Layer 0: [honba-me-core] ──▶ depends on external `honba-core`, `honba-mon`
```

---

## 3. Deep-Dive Specification & Missing Features

### 3.1 `honba-me-core`: Feed Trait, Resilient Buffers & Price Band Enrichment
- **`MarketDataFeed` Trait Evolution**:
  - Add `fetch_snapshot(&self, instruments: &[InstrumentId]) -> Result<Vec<Tick>, MdError>` for broker REST quote reconciliation.
  - Add `feed_health(&self) -> FeedHealth` with heartbeat timestamps, round-trip latency (RTT), dropped packet counts, and reconnection attempts.
  - Add dynamic subscription mode upgrades (`set_mode(&self, inst: &InstrumentId, mode: Mode)`).
- **True Coalescing Last-Value-Cache (LVC) Buffer**:
  - Replace the current drop-on-full buffer with a hybrid structure:
    - Bounded `tokio::sync::mpsc` queue for ordinary flow.
    - Eviction overflow hash map (`HashMap<InstrumentId, Tick>`) that coalesces LTP and Quote updates per scrip when the queue is saturated, preserving chronological monotonicity while ensuring zero loss of the latest quote.
- **Price Band & Microstructure Enrichment Pipeline**:
  - Implement `MarketDataEnricher`: wraps incoming raw broker ticks and enriches them with:
    - Dynamic scrip price bands (queried from `honba-me-normalizer` / daily Bhavcopy).
    - Prev close, 52-week high/low, and day open.
    - Monotonically calculated intraday VWAP.

### 3.2 Broker Adapters: Production Wire Protocols & WebSocket Clients
Each broker crate must provide a live async WebSocket connection (`tokio-tungstenite` with `rustls-tls-native-roots`), automatic heartbeat handler, reconnection loop with exponential backoff (1s, 2s, 4s... capped at 60s + random jitter), and complete binary packet decoders:

#### A. `honba-me-zerodha` (Kite Ticker WebSocket)
- **Binary Wire Decoders**:
  - 8-byte LTP mode: `[0..4] token`, `[4..8] ltp` (divide by 100).
  - 28-byte Index Quote: token, ltp, high, low, open, close, net change.
  - 44-byte Full Equity Quote: token, ltp, last traded qty, avg traded price, volume, buy/sell qty, open, high, low, close.
  - 184-byte Full Market Depth: All quote fields + 5 depth levels for buy and sell (bid/ask price, quantity, order count) + Open Interest (OI) and day high/low OI.
  - Multi-packet binary frame unwrapping (Kite frames bundle multiple packets prefixed with 2-byte packet length).
  - Decompression: Handle gzip/zlib compressed binary payloads delivered by Kite Ticker when subscribing to large instrument sets.
- **Subscription Protocol**:
  - Text JSON messages: `{"a": "subscribe", "v": [tokens]}`, `{"a": "mode", "v": ["full", [tokens]]}`.
- **REST Snapshot Reconciliation**:
  - Integrate with `GET https://api.kite.trade/quote` on reconnect to bridge missing ticks.

#### B. `honba-me-upstox` (Upstox v2 Market Data Feed)
- **Protobuf Wire Decoders**:
  - Implement `MarketDataFeed.proto` decoding using `prost`:
    - `FeedResponse` wrapper containing `Type` (initial feed vs update).
    - `LTPFeed`: last price, close price, timestamp.
    - `MarketFullFeed`: LTP, OHLC, 5-depth order book, total buy/sell quantity, OI, and 52-week limits.
    - `OptionGreeksFeed`: IV, Delta, Gamma, Theta, Vega, Rho, spot price for NFO contracts.
- **Authentication & Handshake**:
  - WebSocket URL generated dynamically via `GET https://api.upstox.com/v2/feed/market-data-feed/authorize`.
  - Authorized WebSocket connection with Protobuf message exchange.

#### C. `honba-me-fyers` (Fyers v3 Data Socket)
- **Binary & JSON Wire Decoders**:
  - Binary 72-byte packet (Lite mode) and 200-byte packet (Full depth mode with 5 bid/ask levels).
  - 7-byte ping/pong heartbeat protocol.
  - Subscriptions via binary/JSON command format: `{"sub_lst": ["NSE:SBIN-EQ"], "sub_type": 1}`.

#### D. `honba-me-angelone` (Angel One SmartAPI WebSocket 2.0) [NEW CRATE]
- **Binary Wire Decoders**:
  - Binary packet structure: 1-byte message type (1 = Quote, 2 = Best 5 Depth, 3 = Status), 25-byte token, 8-byte LTP, 8-byte exchange timestamp, followed by 5 bid/ask depth structures.
  - Heartbeat ping every 30 seconds.
  - Authorization via JWT auth token and client code in request headers.

### 3.3 `honba-me-normalizer`: Catalogs, Token Mappings & Corporate Actions
- **Automated Instrument Master Ingestion**:
  - Download, decompress, and index daily instrument masters:
    - Zerodha: `https://api.kite.trade/instruments` (CSV)
    - Upstox: `https://assets.upstox.com/market-quote/instruments/exchange/complete.json.gz`
    - Fyers: `https://public.fyers.in/sym_details/NSE_CM.csv`, `NSE_FO.csv`
    - NSE: `https://archives.nseindia.com/content/equities/EQUITY_L.csv`
  - In-memory trie and hash index mapping: `Broker Token <==> Exchange Symbol <==> ISIN <==> Canonical InstrumentId`.
- **Corporate Actions Engine**:
  - Track corporate events: Stock splits (e.g. 1:10), Bonus issues (e.g. 1:1), Cash dividends, Demergers, Consolidations, Symbol renames.
  - Calculate adjustment factors:
    $$\text{Split Factor} = \frac{\text{New Shares}}{\text{Old Shares}}, \quad \text{Dividend Factor} = \frac{P_{\text{cum}} - D}{P_{\text{cum}}}$$
  - Generate split/bonus-adjusted OHLCV series alongside unadjusted raw series for point-in-time backtesting.
- **Trading Rules & Lot Sizes**:
  - Dynamic tick size lookup based on price thresholds (NSE tick size 0.05 vs 0.01 for sub-penny stocks).
  - Derivatives contract specifications: Monthly/weekly expiry date resolver, F&O lot sizes (NIFTY, BANKNIFTY, FINNIFTY, equity stock options).

### 3.4 `honba-me-feed` & `honba-me-loaders`: Parquet Lakehouse v2 & Historical Ingestion
- **Modernize `honba-me-feed`**:
  - Remove all direct dependencies on legacy `honba-messages` and `honba-entities`.
  - Replace `Float64` price/size columns in Arrow/Parquet schemas with exact `Decimal128(18, 4)` or integer minor units (paisa).
  - Standardize `Bar` and `Tick` storage formats on Arrow RecordBatches.
- **Hive-Partitioned Lakehouse Directory**:
  - Standard directory layout:
    ```text
    honba-data-store/
    ├── ticks/
    │   └── exchange=NSE/
    │       └── date=2026-10-20/
    │           ├── INFY.parquet
    │           └── RELIANCE.parquet
    └── bars/
        └── timeframe=1m/
            └── exchange=NSE/
                └── year=2026/
                    └── month=10/
                        └── INFY.parquet
    ```
- **`TickRecorder` Daemon**:
  - High-throughput asynchronous background actor that subscribes to `honba-me-core` broadcast stream.
  - Batches incoming ticks into memory chunks (e.g. 100,000 ticks or 60 seconds).
  - Flushes to partitioned Parquet files with Snappy/Zstd compression.
- **`honba-me-loaders` Crate [NEW]**:
  - **NSE Daily Bhavcopy Downloader**:
    - Automated fetch of `SEC_BHAVDATA_V2.csv` and delivery position report (`MTO_*.DAT`).
    - Parse daily OHLC, VWAP, total volume, total trades, deliverable volume, and deliverable percentage.
  - **Historical Gap Reconciliation**:
    - Query local parquet store for missing bars (e.g. due to system downtime).
    - Query broker historical REST endpoints (Kite Historical API, Upstox V2 Candles) to backfill missing intraday bars.
  - **DuckDB Lakehouse Connector**:
    - Provide zero-copy DuckDB table views over the Parquet directories for research and Python interop (`honba-data`).

### 3.5 `honba-me-indicators`: Expanded Suite & Vectorized Batch Computing
- **Dual-Mode API**:
  - `Streaming`: Incremental, single-pass O(1) stateful updates implementing `Indicator` trait.
  - `Batch`: High-speed vectorized computation over Arrow `Float64Array` / `Decimal128Array` using SIMD instructions for historical backtesting and chart rendering in `honba-kao`.
- **Target Indicator Portfolio**:
  | Indicator | Description | Status | Target |
  | :--- | :--- | :--- | :--- |
  | `Sma`, `Ema`, `Rsi`, `Macd`, `BollingerBands`, `Donchian`, `Atr`, `Stochastic`, `WilliamsR` | Core technical indicators | Implemented (f64) | Add dual-mode & Arrow batch support |
  | `Supertrend` | ATR-based trend following (standard for Indian index/equity trading) | Missing | Implement incremental + batch |
  | `Vwap` | Volume Weighted Average Price with +1σ, +2σ, +3σ standard deviation bands | Missing | Implement session-anchored streaming |
  | `Cpr` | Central Pivot Range (Pivot, TC, BC) with daily/weekly floor pivots | Missing | Implement daily candle anchor |
  | `Adx` | Average Directional Index with +DI and -DI directional movement | Missing | Implement Wilder smoothing |
  | `Rma` | Wilder's Running Moving Average (foundation for RSI/ATR parity) | Missing | Implement incremental |
  | `Psar` | Parabolic Stop and Reverse with acceleration factor and max step | Missing | Implement stateful trailing stop |
  | `Cci` | Commodity Channel Index with Mean Absolute Deviation | Missing | Implement ring-buffer MAD |
  | `Obv` | On-Balance Volume with cumulative volume flow | Missing | Implement streaming |
  | `IchimokuCloud` | Tenkan-sen, Kijun-sen, Senkou Span A/B, Chikou Span | Missing | Implement multi-window ring buffer |
  | `HullMovingAverage` | Weighted moving average responsive filter (HMA) | Missing | Implement WMA-based cascade |
- **PyO3 Python Bindings**:
  - Export all indicators as Python C-extensions in `honba-tamashii` / `honba.indicators`.

### 3.6 `honba-me-signals`: Multi-Horizon Confluence & Trade Setup Tagging
- **Modernize Domain Types**:
  - Refactor from legacy `honba_messages::Bar` to `honba_core::Bar`.
- **Intraday Signal Horizon**:
  - Add detectors for 1m, 5m, and 15m timeframes:
    - `VwapCross`: Price crossing above/below session VWAP with volume confirmation.
    - `CprBreakout`: Breakout above Virgin CPR or top central pivot.
    - `OrbBreakout`: Opening Range Breakout (first 15-minute or 30-minute high/low break).
    - `VolumeSurge`: Bar volume exceeding 3x 20-period volume SMA during price expansion.
- **Trade Setup Tagging**:
  - Annotate every generated `Signal` with:
    - Calculated Stop-Loss price (based on swing low or $2 \times \text{ATR}$).
    - Take-Profit targets (Target 1 at 1.5R, Target 2 at 3R).
    - Confidence score (confluence weight $0.0 - 1.0$).

---

## 4. Phase-by-Phase Implementation Roadmap

### Phase 1: Canonical Type Unification & Legacy Decoupling (M0) — Week 1–2
- [ ] **1.1 Workspace Dependency Clean-up**
  - [ ] Add `honba-core` and `honba-mon` as dependencies in `honba-me-feed`, `honba-me-market`, and `honba-me-signals`.
  - [ ] Deprecate legacy dependencies (`honba-messages`, `honba-entities`, `honba-ports`) in `honba-me`.
  - [ ] Replace `f64` in `honba-me-market` (`PriceBand`, `QuantityViolation`) with `honba_core::Price` and `Quantity`.
- [ ] **1.2 Feed Trait Evolution**
  - [ ] Update `honba_me_core::MarketDataFeed` to include `fetch_snapshot` and `feed_health`.
  - [ ] Implement true LVC (Last-Value-Cache) in `CoalescingTickBuffer` with eviction map under backpressure.
  - [ ] Implement `MarketDataEnricher` to attach dynamic `PriceBand` and intraday VWAP to incoming ticks.

### Phase 2: Production WebSocket Clients & Multi-Broker Binary Decoders (M1) — Week 3–5
- [ ] **2.1 Shared WebSocket Transport (`honba-me-core`)**
  - [ ] Build reusable `WsClient` wrapping `tokio-tungstenite` with `rustls-tls-native-roots`.
  - [ ] Implement automated heartbeat ping/pong timer and dead-connection detection.
  - [ ] Implement exponential backoff reconnection loop with full jitter (1s to 60s).
  - [ ] Implement automatic subscription replay on reconnect.
- [ ] **2.2 Zerodha Kite Ticker (`honba-me-zerodha`)**
  - [ ] Implement full binary decoders: 8B LTP, 28B Index, 44B Quote, and 184B 5-Depth.
  - [ ] Add zlib decompression for bundled binary frames.
  - [ ] Implement REST quote snapshot fetch on reconnect via Kite REST API.
  - [ ] Wire live WebSocket connection to `subscribe()` stream.
- [ ] **2.3 Upstox v2 Feed (`honba-me-upstox`)**
  - [ ] Add `prost` Protobuf compilation for `MarketDataFeed.proto`.
  - [ ] Implement binary Protobuf parsing for `LTPFeed`, `MarketFullFeed`, and `OptionGreeks`.
  - [ ] Wire dynamic authorized WebSocket handshake and streaming.
- [ ] **2.4 Fyers v3 Data Socket (`honba-me-fyers`)**
  - [ ] Implement 72B Lite and 200B Depth binary decoders and ping/pong protocol.
  - [ ] Wire live WebSocket connection.
- [ ] **2.5 Angel One SmartAPI 2.0 (`honba-me-angelone`) [NEW]**
  - [ ] Scaffold `honba-me-angelone` crate and add to workspace.
  - [ ] Implement SmartAPI binary WebSocket 2.0 parser and JWT auth handshake.

### Phase 3: Instrument Master & Corporate Actions Engine (M2) — Week 6–7
- [ ] **3.1 Instrument Master Ingestion (`honba-me-normalizer`)**
  - [ ] Implement automated downloader for Zerodha `instruments.csv`, Upstox `complete.json.gz`, and NSE `EQUITY_L.csv`.
  - [ ] Build fast in-memory trie and multi-key index for Token $\leftrightarrow$ Symbol $\leftrightarrow$ ISIN $\leftrightarrow$ `InstrumentId`.
  - [ ] Implement dynamic tick size and lot size resolver.
- [ ] **3.2 Corporate Actions Engine (`honba-me-normalizer`)**
  - [ ] Build corporate action event ledger (splits, bonuses, dividends, symbol changes).
  - [ ] Implement backward/forward price adjustment calculation for OHLCV bars.
  - [ ] Expose adjusted series generation to backtester and feed readers.

### Phase 4: Parquet Lakehouse v2, Bhavcopy & Live Tick Recorder (M3) — Week 8–10
- [ ] **4.1 Parquet Lakehouse v2 (`honba-me-feed`)**
  - [ ] Redesign Parquet schema using Arrow 53 with `Decimal128(18, 4)` precision.
  - [ ] Implement Hive-partitioned file structure (`exchange=.../date=...`).
  - [ ] Update `ParquetTickStore` and `DatasetReader` to return canonical `honba_core::Tick` and `Bar`.
- [ ] **4.2 Data Loaders Crate (`honba-me-loaders`) [NEW]**
  - [ ] Scaffold `honba-me-loaders` crate.
  - [ ] Implement daily NSE Bhavcopy downloader (`SEC_BHAVDATA_V2` & delivery files).
  - [ ] Implement broker REST historical bar loaders (Kite & Upstox historical candles) for gap backfill.
  - [ ] Implement DuckDB query views over Parquet directories.
- [ ] **4.3 Live Tick Recorder Daemon (`honba-me-feed`)**
  - [ ] Implement `TickRecorder` actor with rolling memory buffers and Snappy/Zstd compressed Parquet flushing.

### Phase 5: Advanced Indian Technical Indicators & Vectorized Compute (M4) — Week 11–12
- [ ] **5.1 Expanded Indicator Suite (`honba-me-indicators`)**
  - [ ] Implement `Supertrend` (ATR multiplier + upper/lower bands).
  - [ ] Implement `Vwap` with session anchoring and $\pm 1\sigma, \pm 2\sigma, \pm 3\sigma$ bands.
  - [ ] Implement `Cpr` (Central Pivot Range: Pivot, TC, BC) with standard floor pivots.
  - [ ] Implement `Adx` (+DI, -DI, ADX) with Wilder smoothing.
  - [ ] Implement `Rma` (Wilder's Moving Average), `Psar`, `Cci`, `Obv`, `IchimokuCloud`.
- [ ] **5.2 Dual-Mode API & Vectorized Arrow Compute**
  - [ ] Provide high-level ergonomic methods taking `&honba_core::Bar` and returning `honba_core::Price`.
  - [ ] Implement batch computation over Arrow arrays for instant backtest evaluations.
- [ ] **5.3 Screener Engine & PyO3 Bindings**
  - [ ] Extend screener metrics to support Supertrend, VWAP, and CPR predicates.
  - [ ] Expose indicators to Python in `honba-tamashii`.

### Phase 6: Intraday Signal Detectors & End-to-End Runtime Integration (M5) — Week 13–14
- [ ] **6.1 Intraday Signal Detectors (`honba-me-signals`)**
  - [ ] Refactor signal engine to consume `honba_core::Bar`.
  - [ ] Implement Intraday detectors: VWAP Cross, CPR Breakout, ORB (15m/30m), and Volume Surge.
  - [ ] Implement trade setup tagging (ATR-based Stop-Loss, Target 1, Target 2, Risk/Reward ratio).
- [ ] **6.2 End-to-End System Integration**
  - [ ] Connect `honba-me` live feeds directly to `honba-ya-actors::MarketDataActor`.
  - [ ] Validate live tick ingestion $\rightarrow$ price band enrichment $\rightarrow$ `honba-mon::RiskEngine` evaluation $\rightarrow$ broker execution.
  - [ ] Verify zero-lag streaming to `honba-kao` Web terminal chart via WebSocket.

---

## 5. Verification, Conformance & Benchmarks

1. **Wire Protocol Golden Tests**:
   - Replay recorded raw binary WebSocket dumps from Zerodha Kite, Upstox Protobuf, and Fyers against parsers and verify bit-for-bit canonical `Tick` equivalence.
2. **Backpressure & Monotonicity Stress Tests**:
   - Bombard `CoalescingTickBuffer` with 500,000 synthetic ticks/second across 1,000 scrips with artificial consumer delays. Verify zero memory unbounded growth, timestamp monotonicity, and retention of latest quotes.
3. **Indicator Conformance**:
   - Validate outputs of `Supertrend`, `Vwap`, `Cpr`, `Adx` against TradingView and Python TA-Lib reference datasets with absolute tolerance $< 10^{-6}$.
4. **Latency Benchmarks (`criterion`)**:
   - Binary tick decode to canonical `Tick`: **$< 150 \text{ ns}$**.
   - Price band enrichment & LVC cache update: **$< 250 \text{ ns}$**.
   - Incremental indicator update (e.g. Supertrend / VWAP): **$< 50 \text{ ns}$**.
5. **Regulatory & Market Microstructure Tests**:
   - Verify out-of-band price limit rejection across NSE scrip circuit limits (2%, 5%, 10%, 20%).
   - Verify T+1 settlement calendar handling across Indian exchange holidays.
