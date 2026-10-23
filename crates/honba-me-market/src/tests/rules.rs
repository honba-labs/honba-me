//! Unit tests for `crate::rules`.

use honba_core::{Price, Quantity};
use crate::{
    InstrumentRules, MarketError, PriceBand, PriceViolation, QuantityViolation,
};
use rust_decimal_macros::dec;

#[test]
fn new_rules_default_min_quantity_to_one_lot() {
    let r = InstrumentRules::new(Quantity::new(dec!(25.0)), Price::new(dec!(0.05)));
    assert_eq!(r.min_order_quantity, Quantity::new(dec!(25.0)));
    assert_eq!(r.max_order_quantity, None);
    assert_eq!(r.with_max_quantity(Quantity::new(dec!(1800.0))).max_order_quantity, Some(Quantity::new(dec!(1800.0))));
}

#[test]
fn quantity_must_be_a_lot_multiple_within_bounds() {
    let q25 = Quantity::from(Decimal::from(25));
    let q100 = Quantity::from(Decimal::from(100));
    let q10 = Quantity::from(Decimal::from(10));
    let q125 = Quantity::from(Decimal::from(125));
    let q30 = Quantity::from(Decimal::from(30));

    let r = InstrumentRules::new(q25, Price::new(Decimal::from(1))).with_max_quantity(q100);
    assert_eq!(r.validate_quantity(q25), Ok(()));
    assert_eq!(r.validate_quantity(q100), Ok(()));
    assert_eq!(
        r.validate_quantity(q10),
        Err(QuantityViolation::BelowMin {
            quantity: q10,
            min: q25
        })
    );
    assert_eq!(
        r.validate_quantity(q125),
        Err(QuantityViolation::OverFreeze {
            quantity: q125,
            max: q100
        })
    );
    assert_eq!(
        r.validate_quantity(q30),
        Err(QuantityViolation::NotLotMultiple {
            quantity: q30,
            lot: q25
        })
    );
}

#[test]
fn quantity_violation_displays_todays_prose_and_converts_to_market_error() {
    let below = QuantityViolation::BelowMin {
        quantity: 10.0,
        min: 25.0,
    };
    assert_eq!(below.to_string(), "quantity 10 below minimum 25");
    let over = QuantityViolation::OverFreeze {
        quantity: 125.0,
        max: 100.0,
    };
    assert_eq!(
        over.to_string(),
        "quantity 125 exceeds maximum freeze limit 100"
    );
    let lot = QuantityViolation::NotLotMultiple {
        quantity: 30.0,
        lot: 25.0,
    };
    assert_eq!(
        lot.to_string(),
        "quantity 30 is not an exact multiple of lot size 25"
    );
    assert_eq!(
        MarketError::from(lot),
        MarketError::RuleViolation(lot.to_string())
    );
}

#[test]
fn price_must_be_positive_and_on_tick() {
    let r = InstrumentRules::new(1.0, 0.05);
    assert_eq!(r.validate_price(100.05), Ok(()));
    assert_eq!(
        r.validate_price(0.0),
        Err(PriceViolation::NonPositive { price: 0.0 })
    );
    assert_eq!(
        r.validate_price(-1.0),
        Err(PriceViolation::NonPositive { price: -1.0 })
    );
    assert_eq!(
        r.validate_price(100.03),
        Err(PriceViolation::OffTick {
            price: 100.03,
            tick: 0.05
        })
    );
}

#[test]
fn nan_and_infinite_prices_are_refused() {
    let r = InstrumentRules::new(1.0, 0.05);
    assert!(matches!(
        r.validate_price(f64::NAN),
        Err(PriceViolation::NonPositive { .. })
    ));
    assert!(matches!(
        r.validate_price(f64::INFINITY),
        Err(PriceViolation::OffTick { .. })
    ));
}

#[test]
fn price_violation_displays_todays_prose_and_converts_to_market_error() {
    let np = PriceViolation::NonPositive { price: 0.0 };
    assert_eq!(np.to_string(), "price 0 must be positive");
    let off = PriceViolation::OffTick {
        price: 100.03,
        tick: 0.05,
    };
    assert_eq!(
        off.to_string(),
        "price 100.03 does not conform to tick size 0.05"
    );
    assert_eq!(
        MarketError::from(off),
        MarketError::RuleViolation(off.to_string())
    );
}

#[test]
fn validate_price_tolerance_matches_is_on_tick() {
    let inst = Instrument::new(
        InstrumentId::new("TEST", Exchange::new("NSE")),
        InstrumentKind::Equity,
        Currency::Inr,
        1.0,
        0.05,
    );
    let rules = InstrumentRules::new(1.0, 0.05);
    // Offsets in ticks around a tick boundary, straddling the 1e-6 tolerance.
    let offsets = [0.0, 5e-7, -5e-7, 2e-6, -2e-6, 1e-5, -1e-5, 5e-5, 1e-4, 0.5];
    for base in [100.0_f64, 100.05, 2500.5, 0.05] {
        for off in offsets {
            let price = base + off * 0.05;
            assert_eq!(
                rules.validate_price(price).is_ok(),
                inst.is_on_tick(price),
                "price {price} (offset {off} ticks)"
            );
        }
    }
}

#[test]
fn price_band_is_inclusive() {
    let b = PriceBand::new(90.0, 110.0);
    assert!(b.contains(90.0) && b.contains(110.0));
    assert!(!b.contains(89.99) && !b.contains(110.01));
}

#[test]
fn default_symbol_grammar_trims_uppercases_and_rejects_blank() {
    let g = NullSymbolGrammar;
    assert_eq!(g.normalize("  reliance "), "RELIANCE");
    assert_eq!(g.validate("TCS"), Ok(()));
    assert!(matches!(
        g.validate("   "),
        Err(MarketError::InvalidSymbol { .. })
    ));
}
