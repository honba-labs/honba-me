//! Typed quantity violations through `InstrumentRulesProvider` (ADR 0018 decision 4a).

use honba_core::{Exchange, InstrumentId, Price, Quantity};
use honba_market::profile::MarketProfile;
use honba_market::{InstrumentRules, MarketError, QuantityViolation};
use rust_decimal_macros::dec;

fn instrument_id() -> InstrumentId {
    InstrumentId::new(Exchange::Nse, "TEST")
}

#[test]
fn provider_rules_report_typed_lot_violation() {
    let rules = InstrumentRules::new(Quantity::from_u64(25), Price::new(dec!(0.05)));
    assert_eq!(rules.validate_quantity(Quantity::from_u64(50)), Ok(()));
    let err = rules.validate_quantity(Quantity::from_u64(30)).unwrap_err();
    assert_eq!(
        err,
        QuantityViolation::NotLotMultiple {
            quantity: Quantity::from_u64(30),
            lot: Quantity::from_u64(25),
        }
    );
    assert!(matches!(
        MarketError::from(err),
        MarketError::RuleViolation(_)
    ));
}

#[test]
fn provider_rules_report_typed_below_min() {
    let rules = InstrumentRules::new(Quantity::from_u64(25), Price::new(dec!(0.05)));
    assert_eq!(
        rules.validate_quantity(Quantity::from_u64(10)),
        Err(QuantityViolation::BelowMin {
            quantity: Quantity::from_u64(10),
            min: Quantity::from_u64(25),
        })
    );
}

#[cfg(feature = "india")]
#[test]
fn india_profile_reports_typed_violations() {
    use honba_market::india::profile::IndiaMarketProfile;
    let profile = IndiaMarketProfile::default();
    let rules = profile.instrument_rules().rules_for(&instrument_id());
    assert_eq!(rules.validate_quantity(Quantity::from_u64(7)), Ok(()));
    assert_eq!(
        rules.validate_quantity(Quantity::new(dec!(0.5))),
        Err(QuantityViolation::BelowMin {
            quantity: Quantity::new(dec!(0.5)),
            min: Quantity::from_u64(1),
        })
    );
    assert_eq!(
        rules.validate_quantity(Quantity::new(dec!(1.5))),
        Err(QuantityViolation::NotLotMultiple {
            quantity: Quantity::new(dec!(1.5)),
            lot: Quantity::from_u64(1),
        })
    );
    let capped = rules.clone().with_max_quantity(Quantity::from_u64(10));
    assert_eq!(
        capped.validate_quantity(Quantity::from_u64(11)),
        Err(QuantityViolation::OverFreeze {
            quantity: Quantity::from_u64(11),
            max: Quantity::from_u64(10),
        })
    );
}
