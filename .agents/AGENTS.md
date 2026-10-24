# `honba-me` Agent Guidelines & Architecture Invariants

**Subsystem**: Eye (Market Data & Indicators)
**Language**: Rust
**Primary Responsibility**: Multi-broker binary WebSockets, tick coalescing, 100+ StockSharp indicators, and Parquet Lakehouse ingestion.

---

## 1. Role & Architectural Hints
* `honba-me` ingests, normalizes, and calculates real-time market data:
  1. **Broker Feeds**: High-throughput binary WebSockets for Zerodha Kite, Upstox, Fyers, Angel One, and Dhan.
  2. **Anti-Corruption Layer**: Maps heterogeneous broker tokens and tick formats to canonical `honba-core::Tick` and `honba-core::Bar`.
  3. **Tick Coalescing & Backpressure**: Bounded `tokio::sync::mpsc` channels with non-blocking coalescing to prevent buffer bloat.
  4. **100+ StockSharp Indicators**: Streaming calculation of moving averages, oscillators, volatility bands, and volume profiles with zero heap allocations in hot paths.
* **Idea Inheritance**:
  - **StockSharp**: Comprehensive indicator catalog (100+ indicators) and streaming tick architecture.
  - **NautilusTrader**: High-performance market data caching and bar aggregation.

---

## 2. Coding Guidelines (DDD & TDD)
* **Interface-First**: Feeds implement `MarketDataFeed` trait; indicators implement streaming calculation traits.
* **Zero Floats Policy**: Price and volume use `Decimal`.
* **TDD & Parallel Test Directory Rule (MANDATORY)**:
  - All tests must live in `tests/` parallel to `src/`.
  - **STRICTLY FORBIDDEN**: In-file `#[test]` inside `src/`.
  - Golden indicator verification tests against StockSharp reference vectors.

---

## 3. Retargetability & Portability
* Compute indicators compile cleanly to `wasm32-unknown-unknown` for browser visualization.

---

## 4. Development & CI Workflow
* **Linting & Formatting**:
  ```bash
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  ```
* **Testing**:
  ```bash
  cargo test --all
  ```
