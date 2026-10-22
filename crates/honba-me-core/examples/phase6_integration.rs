//! End-to-End Integration Example for Honba Phase 6.2.
//!
//! This example demonstrates the complete trading pipeline:
//! honba-ya MarketDataActor → honba-mon::RiskEngine → honba-ya ExecutionActor
//!
//! Usage: cargo run --package honba-me --example strategy_integration

use std::time::Duration;
use honba_core::{Bar, InstrumentId, Price, Quantity, Tick};
use honba_me::{
    MarketDataFeed, InstrumentId as HonbaInstrumentId,
};
use honba_ya_actors::{
    MarketDataActor, RiskActor, ExecutionActor, Supervisor,
    messages::{MarketDataMsg, RiskMsg, ExecutionMsg},
};
use honba_mon::{
    RiskEngine, RiskLimits, OtrConfig,
};
use honba_te_core::BrokerExecutor;

/// Example broker executor that logs orders instead of sending to a real broker.
struct LoggingBroker;

#[async_trait::async_trait]
impl honba_te_core::BrokerExecutor for LoggingBroker {
    fn name(&self) -> &str {
        "LoggingBroker"
    }

    async fn place_order(&self, req: honba_core::OrderRequest) -> Result<honba_core::OrderAck, honba_te_core::BrokerError> {
        info!("EXECUTION: Placing order - Instrument: {}, Side: {:?}, Qty: {}, Price: {:?}, Algo: {}",
              req.instrument.symbol, req.side, req.quantity, req.order_type.limit_price().map(|p| p.0), req.algo_id);
        Ok(honba_core::OrderAck::new(
            format!("ORD-{}", chrono::Utc::now().timestamp_millis()),
            req.client_order_id,
            honba_core::OrderStatus::Accepted,
            chrono::Utc::now(),
        ))
    }

    async fn cancel_order(&self, _order_id: &str) -> Result<(), honba_te_core::BrokerError> {
        Ok(())
    }

    async fn get_order_status(&self, _order_id: &str) -> Result<honba_core::OrderStatus, honba_te_core::BrokerError> {
        Ok(honba_core::OrderStatus::Accepted)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("=== Honba Phase 6.2: End-to-End Integration Example ===");

    // ========================================================================
    // Step 1: Configure Risk Engine (honba-mon)
    // ========================================================================
    let limits = RiskLimits {
        max_notional_per_order: Some(honba_core::Money::new(rust_decimal::Decimal::from(2500000), honba_core::Currency::INR)),
        max_quantity_per_order: Some(Quantity::new(rust_decimal::Decimal::from(1800))), // NIFTY freeze limit
        max_daily_loss: Some(Decimal::from(100000)),
        order_rate_limit: Some(honba_mon::OrderRateLimit { max_orders: 100, window_ms: 60000 }),
        price_collar_bps: Some(500),
        freeze_limits: std::collections::HashMap::new(),
        max_position_concentration_pct: Some(Decimal::from(10)),
        max_mis_leverage: Some(Decimal::from(5)),
    };

    let otr_config = OtrConfig {
        max_otr: Decimal::from(50),
        min_orders_before_enforcement: 10,
        warning_threshold: Decimal::from(40),
        window_1min_ms: 60000,
        window_5min_ms: 300000,
        halt_cooldown_ms: 60000, // 1 minute cooldown
    };

    let mut risk_engine = RiskEngine::new(limits, otr_config);

    // Register algo IDs
    risk_engine.register_algo("momentum_strat_01");
    risk_engine.register_algo("mean_reversion_01");

    // Set price bands for NIFTY (2% circuit)
    let nifty = InstrumentId::new(honba_core::Exchange::Nse, "NIFTY");
    let nifty_band = honba_core::PriceBand::new(
        Price::from_str_exact("22000.00")?,
        Price::from_str_exact("22880.00")?,
        Price::from_str_exact("22400.00")?,
    );
    risk_engine.set_price_band(nifty.clone(), nifty_band);

    info!("Risk engine configured with NIFTY price band: 22000-22880 (ref: 22400)");

    // ========================================================================
    // Step 2: Create MarketDataActor and connect to RiskActor
    // ========================================================================
    let (md_tx, md_rx) = tokio::sync::mpsc::channel(1000);
    let (strategy_tx, _strategy_rx) = tokio::sync::mpsc::channel(1000);

    let (mut market_data_actor, md_broadcast_rx) = honba_ya_actors::MarketDataActor::new(
        md_rx,
        vec![strategy_tx.clone()],
        100,
    );

    // Register instruments with the actor
    market_data_actor.symbol_mapper_mut().register("NSE:NIFTY", nifty.clone());

    // Spawn market data actor
    let md_handle = tokio::spawn(async move {
        market_data_actor.run().await
    });

    // ========================================================================
    // Step 3: Create execution actor
    // ========================================================================
    let (exec_tx, exec_rx) = tokio::sync::mpsc::channel(1000);
    let exec_actor = ExecutionActor::new(
        std::sync::Arc::new(LoggingBroker),
        exec_rx,
        Some(md_tx.clone()), // risk_tx for cancellations
        None, // strategy_tx
    );
    let exec_handle = tokio::spawn(async move {
        exec_actor.run().await
    });

    // ========================================================================
    // Step 4: Simulate market data flow through the pipeline
    // ========================================================================
    info!("Starting end-to-end simulation...");

    let now = chrono::Utc::now();

    // Simulate a sequence of ticks
    let test_ticks = vec![
        // Normal trade within price band
        Tick::Ltp {
            instrument: nifty.clone(),
            price: Price::from_str_exact("22400.00")?,
            volume: Quantity::new(Decimal::from(100)),
            exchange_ts: now,
            receive_ts: now,
        },
        // Price approaching upper circuit
        Tick::Ltp {
            instrument: nifty.clone(),
            price: Price::from_str_exact("22750.00")?,
            volume: Quantity::new(200),
            exchange_ts: now + chrono::Duration::seconds(1),
            receive_ts: now + chrono::Duration::seconds(1),
        },
        // Price at upper circuit limit
        Tick::Ltp {
            instrument: nifty.clone(),
            price: Price::from_str_exact("22880.00")?,
            volume: Quantity::new(300),
            exchange_ts: now + chrono::Duration::seconds(2),
            receive_ts: now + chrono::Duration::seconds(2),
        },
    ];

    for tick in test_ticks {
        info!("FEED: Tick received - Instrument: {}, Price: {:.2}, Qty: {}",
              tick.instrument().symbol(), tick.price().map(|p| p.0).unwrap_or(0.0),
              tick.volume().map(|v| v.0).unwrap_or(0.0));

        // Send to MarketDataActor
        md_tx.send(honba_ya_actors::messages::MarketDataMsg::IngestTick(tick.clone())).await?;

        // Small delay to simulate real-time
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ========================================================================
    // Step 5: Demonstrate risk check directly
    // ========================================================================
    info!("\n=== Direct Risk Engine Validation ===");

    let order_req = honba_core::OrderRequest::new(
        InstrumentId::new(honba_core::Exchange::Nse, "NIFTY"),
        honba_core::OrderSide::Buy,
        honba_core::OrderType::Limit(Price::from_str_exact("22850.00")?),
        Quantity::new(Decimal::from(100)),
        "momentum_strat_01",
    );

    match risk_engine.check_order(&order_req, now + chrono::Duration::seconds(3)) {
        Ok(()) => info!("RISK: Order APPROVED - instrument: {}, price: {:.2}, qty: {}",
                       order_req.instrument.symbol, order_req.order_type.limit_price().map(|p| p.0).unwrap_or(0.0),
                       order_req.quantity.0),
        Err(refusal) => {
            warn!("RISK: Order REJECTED - reason: {}", refusal);
        }
    }

    // Test an order that exceeds price band
    let over_limit_order = honba_core::OrderRequest::new(
        InstrumentId::new(honba_core::Exchange::Nse, "NIFTY"),
        honba_core::OrderSide::Buy,
        honba_core::OrderType::Limit(Price::from_str_exact("23000.00")?),
        Quantity::new(Decimal::from(100)),
        "momentum_strat_01",
    );

    match risk_engine.check_order(&over_limit_order, now + chrono::Duration::seconds(4)) {
        Ok(()) => info!("RISK: Order APPROVED (unexpected)"),
        Err(refusal) => warn!("RISK: Order REJECTED (expected) - reason: {}", refusal),
    }

    // ========================================================================
    // Step 6: Shutdown
    // ========================================================================
    info!("\n=== Shutting down ===");

    md_tx.send(honba_ya_actors::messages::MarketDataMsg::Shutdown).await?;
    exec_tx.send(honba_ya_actors::messages::ExecutionMsg::Shutdown).await?;

    let _ = md_handle.await;
    let _ = exec_handle.await;

    info!("=== Phase 6.2 Integration Complete ===");
    Ok(())
}