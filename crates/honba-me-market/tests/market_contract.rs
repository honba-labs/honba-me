//! Contract test suite running against any MarketProfile implementation.

use chrono::NaiveDate;
use honba_entities::{Currency, Instrument, InstrumentKind, PositionSide};
use honba_messages::{Exchange, InstrumentId, OrderSide};

use honba_market::costs::MarketSegment;
use honba_market::null::NullMarketProfile;
use honba_market::profile::{MarketProfile, MarketRegistry};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// Generic contract verification suite for any MarketProfile
fn verify_market_profile_contract(profile: &dyn MarketProfile) {
    // 1. Profile identification
    assert!(!profile.market_code().is_empty());
    assert_ne!(profile.currency(), Currency::Gbp); // Sanity check

    // 2. Calendar contracts
    let cal = profile.calendar();
    let s = cal.session();
    assert!(s.close() > s.open());

    let test_d = date(2025, 6, 2); // Monday
    let next_d = cal.next_trading_day(test_d);
    assert!(next_d > test_d);
    assert!(cal.is_trading_day(next_d));

    let prev_d = cal.prev_trading_day(next_d);
    assert!(prev_d < next_d);
    assert!(cal.is_trading_day(prev_d));

    // 3. Cost Schedule contracts
    let costs = profile.cost_schedule();
    let seg = MarketSegment::new("equity_delivery");
    let buy_breakdown = costs.compute_costs(&seg, OrderSide::Buy, 100_000.0);
    assert!(buy_breakdown.total() >= 0.0);

    // 4. Instrument Rules contracts
    let inst = Instrument::new(
        InstrumentId::new("TEST", Exchange::new("TEST_EX")),
        InstrumentKind::Equity,
        profile.currency(),
        1.0,
        0.05,
    );
    let rules = profile.instrument_rules().rules_for(&inst);
    assert!(rules.lot_size > 0.0);
    assert!(rules.tick_size > 0.0);
    assert!(rules.validate_quantity(1.0).is_ok());
    assert!(rules.validate_price(100.05).is_ok());

    // 5. Symbol Grammar contracts
    let grammar = profile.symbol_grammar();
    assert_eq!(grammar.normalize("  test  "), "TEST");
    assert!(grammar.validate("TEST").is_ok());
    assert!(grammar.validate("   ").is_err());

    // 6. Margin Model contracts
    let margin = profile.margin_model().calculate_margin(
        InstrumentKind::Equity,
        PositionSide::Long,
        100_000.0,
    );
    assert!(margin.initial >= 0.0);
    assert!(margin.maintenance >= 0.0);
    assert!(margin.initial >= margin.maintenance);

    // 7. Settlement Rules contracts
    let settlement = profile.settlement_rules();
    let settle_d = settlement.settlement_date(test_d, InstrumentKind::Equity, cal);
    assert!(settle_d >= test_d);
}

#[test]
fn null_market_pack_satisfies_contract() {
    let profile = NullMarketProfile::default();
    verify_market_profile_contract(&profile);
}

#[cfg(feature = "india")]
#[test]
fn india_market_pack_satisfies_contract() {
    let profile = honba_market::india::profile::IndiaMarketProfile::default();
    verify_market_profile_contract(&profile);
}

#[test]
fn market_registry_resolves_null_and_features() {
    let reg = MarketRegistry::default_registry();
    let null_prof = reg.get("null").expect("null profile should be registered");
    assert_eq!(null_prof.market_code(), "null");

    #[cfg(feature = "india")]
    {
        let india_prof = reg
            .get("nse_bse")
            .expect("nse_bse profile should be registered");
        assert_eq!(india_prof.market_code(), "nse_bse");
        assert_eq!(india_prof.currency(), Currency::Inr);
    }
}
