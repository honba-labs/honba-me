//! End-to-End Integration Example for Honba Phase 6.2.
//!
//! This example demonstrates the market data ingestion + pre-trade risk pipeline:
//! honba-me-core::CoalescingTickBuffer → MarketDataEnricher → honba-mon::RiskEngine
//!
//! Usage: cargo run --package honba-me-core --example phase6_integration

use std::collections::HashMap;
use std::time::Duration;

use honba_core::{
    Exchange, InstrumentId, OrderRequest, OrderSide, OrderType, Price, PriceBand, Quantity, Tick,
};
use honba_me_core::{CoalescingTickBuffer, MarketDataEnricher, SymbolMapper};
use honba_mon::{OrderRateLimit, OtrConfig, RiskEngine, RiskLimits};
use rust_decimal::Decimal;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    info!("=== Honba Phase 6.2: End-to-End Integration Example ===");

    // ========================================================================
    // Step 1: Configure Risk Engine (honba-mon)
    // ========================================================================
    let limits = RiskLimits {
        max_notional_per_order: Some(Decimal::from(2_500_000)),
        max_quantity_per_order: Some(Quantity::new(Decimal::from(1800))), // NIFTY freeze limit
        max_daily_loss: Some(Decimal::from(100_000)),
        order_rate_limit: Some(OrderRateLimit {
            max_orders: 100,
            window_ms: 60_000,
        }),
        price_collar_bps: Some(500),
        freeze_limits: HashMap::new(),
        max_position_concentration_pct: Some(Decimal::from(10)),
        max_mis_leverage: Some(Decimal::from(5)),
    };

    let otr_config = OtrConfig {
        max_otr: Decimal::from(50),
        min_orders_before_enforcement: 10,
        warning_threshold: Decimal::from(40),
        window_1min_ms: 60_000,
        window_5min_ms: 300_000,
        halt_cooldown_ms: 60_000, // 1 minute cooldown
    };

    let mut risk_engine = RiskEngine::new(limits, otr_config);

    // Register algo IDs
    risk_engine.register_algo("momentum_strat_01");
    risk_engine.register_algo("mean_reversion_01");

    // Set price bands for NIFTY (2% circuit)
    let nifty = InstrumentId::new(Exchange::Nse, "NIFTY");
    let nifty_band = PriceBand::new(
        Price::from_rupees("22000.00")?,
        Price::from_rupees("22880.00")?,
        Price::from_rupees("22400.00")?,
    );
    risk_engine.set_price_band(nifty.clone(), nifty_band);

    info!("Risk engine configured with NIFTY price band: 22000-22880 (ref: 22400)");

    // ========================================================================
    // Step 2: Create Market Data Pipeline (SymbolMapper + Enricher + Buffer)
    // ========================================================================
    let mut mapper = SymbolMapper::new();
    mapper.register("NSE:NIFTY", nifty.clone());

    let mut enricher = MarketDataEnricher::new();
    enricher.set_price_band(nifty.clone(), nifty_band);
    enricher.set_prev_close(nifty.clone(), Price::from_rupees("22400.00")?);

    let (tick_buf, mut tick_rx) = CoalescingTickBuffer::new(100);

    // ========================================================================
    // Step 3: Simulate market data flow through the pipeline
    // ========================================================================
    info!("Starting end-to-end simulation...");

    let now = chrono::Utc::now();

    let test_ticks = vec![
        // Normal trade within price band
        Tick::Ltp {
            instrument: nifty.clone(),
            price: Price::from_rupees("22400.00")?,
            volume: Quantity::new(Decimal::from(100)),
            exchange_ts: now,
            receive_ts: now,
        },
        // Price approaching upper circuit
        Tick::Ltp {
            instrument: nifty.clone(),
            price: Price::from_rupees("22750.00")?,
            volume: Quantity::new(Decimal::from(200)),
            exchange_ts: now + chrono::Duration::seconds(1),
            receive_ts: now + chrono::Duration::seconds(1),
        },
        // Price at upper circuit limit
        Tick::Ltp {
            instrument: nifty.clone(),
            price: Price::from_rupees("22880.00")?,
            volume: Quantity::new(Decimal::from(300)),
            exchange_ts: now + chrono::Duration::seconds(2),
            receive_ts: now + chrono::Duration::seconds(2),
        },
    ];

    for mut tick in test_ticks {
        enricher.enrich(&mut tick);
        info!(
            "FEED: Tick received - Instrument: {}, Price: {}",
            tick.instrument().symbol,
            tick.last_price().unwrap_or(Price::ZERO),
        );

        tick_buf.send(tick).await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    while let Ok(received) = tick_rx.try_recv() {
        info!(
            "CONSUMER: Processed tick for {} at {}",
            received.instrument().symbol,
            received.last_price().unwrap_or(Price::ZERO),
        );
    }

    // ========================================================================
    // Step 4: Demonstrate risk check directly
    // ========================================================================
    info!("\n=== Direct Risk Engine Validation ===");

    let order_req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "NIFTY"),
        OrderSide::Buy,
        OrderType::Limit(Price::from_rupees("22850.00")?),
        Quantity::new(Decimal::from(100)),
        "momentum_strat_01",
    );

    match risk_engine.check_order(&order_req, now + chrono::Duration::seconds(3)) {
        Ok(()) => info!(
            "RISK: Order APPROVED - instrument: {}, price: {}, qty: {}",
            order_req.instrument.symbol,
            order_req.order_type.limit_price().unwrap_or(Price::ZERO),
            order_req.quantity.0
        ),
        Err(refusal) => {
            warn!("RISK: Order REJECTED - reason: {}", refusal);
        }
    }

    // Test an order that exceeds price band
    let over_limit_order = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "NIFTY"),
        OrderSide::Buy,
        OrderType::Limit(Price::from_rupees("23000.00")?),
        Quantity::new(Decimal::from(100)),
        "momentum_strat_01",
    );

    match risk_engine.check_order(&over_limit_order, now + chrono::Duration::seconds(4)) {
        Ok(()) => info!("RISK: Order APPROVED (unexpected)"),
        Err(refusal) => warn!("RISK: Order REJECTED (expected) - reason: {}", refusal),
    }

    info!("=== Phase 6.2 Integration Complete ===");
    Ok(())
}
