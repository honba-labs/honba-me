//! Unit tests for `crate::rules`.

use crate::{
    InstrumentRules, MarketError, NullSymbolGrammar, PriceBand, PriceViolation, QuantityViolation,
    SymbolGrammar,
};
use honba_core::{Price, Quantity};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn new_rules_default_min_quantity_to_one_lot() {
    let r = InstrumentRules::new(Quantity::new(dec!(25.0)), Price::new(dec!(0.05)));
    assert_eq!(r.min_order_quantity, Quantity::new(dec!(25.0)));
    assert_eq!(r.max_order_quantity, None);
    assert_eq!(
        r.with_max_quantity(Quantity::new(dec!(1800.0)))
            .max_order_quantity,
        Some(Quantity::new(dec!(1800.0)))
    );
}

#[test]
fn quantity_must_be_a_lot_multiple_within_bounds() {
    let q25 = Quantity::from_u64(25);
    let q100 = Quantity::from_u64(100);
    let q10 = Quantity::from_u64(10);
    let q125 = Quantity::from_u64(125);
    let q30 = Quantity::from_u64(30);

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
        quantity: Quantity::from_u64(10),
        min: Quantity::from_u64(25),
    };
    assert_eq!(below.to_string(), "quantity 10 below minimum 25");
    let over = QuantityViolation::OverFreeze {
        quantity: Quantity::from_u64(125),
        max: Quantity::from_u64(100),
    };
    assert_eq!(
        over.to_string(),
        "quantity 125 exceeds maximum freeze limit 100"
    );
    let lot = QuantityViolation::NotLotMultiple {
        quantity: Quantity::from_u64(30),
        lot: Quantity::from_u64(25),
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
    let r = InstrumentRules::new(Quantity::from_u64(1), Price::new(dec!(0.05)));
    assert_eq!(r.validate_price(Price::new(dec!(100.05))), Ok(()));
    assert_eq!(
        r.validate_price(Price::new(dec!(0))),
        Err(PriceViolation::NonPositive {
            price: Price::new(dec!(0))
        })
    );
    assert_eq!(
        r.validate_price(Price::new(dec!(-1.0))),
        Err(PriceViolation::NonPositive {
            price: Price::new(dec!(-1.0))
        })
    );
    assert_eq!(
        r.validate_price(Price::new(dec!(100.03))),
        Err(PriceViolation::OffTick {
            price: Price::new(dec!(100.03)),
            tick: Price::new(dec!(0.05)),
        })
    );
}

#[test]
fn nan_and_infinite_prices_are_refused() {
    assert!(Price::from_rupees("NaN").is_err());
    assert!(Price::from_rupees("inf").is_err());
    assert!(Price::from_rupees("-1.0").is_err());
}

#[test]
fn price_violation_displays_todays_prose_and_converts_to_market_error() {
    let np = PriceViolation::NonPositive {
        price: Price::new(dec!(0)),
    };
    assert_eq!(np.to_string(), "price 0 must be positive");
    let off = PriceViolation::OffTick {
        price: Price::new(dec!(100.03)),
        tick: Price::new(dec!(0.05)),
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
    let rules = InstrumentRules::new(Quantity::from_u64(1), Price::new(dec!(0.05)));
    for base in [dec!(100.0), dec!(100.05), dec!(2500.5), dec!(0.05)] {
        assert!(rules.validate_price(Price::new(base)).is_ok());
        let off = base + dec!(0.01);
        assert!(rules.validate_price(Price::new(off)).is_err());
    }
}

#[test]
fn price_band_is_inclusive() {
    let b = PriceBand::new(
        Price::new(dec!(90.0)),
        Price::new(dec!(110.0)),
        Price::new(dec!(100.0)),
    );
    assert!(b.contains(Price::new(dec!(90.0))) && b.contains(Price::new(dec!(110.0))));
    assert!(!b.contains(Price::new(dec!(89.99))) && !b.contains(Price::new(dec!(110.01))));
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
