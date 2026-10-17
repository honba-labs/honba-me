//! Typed quantity violations through `InstrumentRulesProvider` (ADR 0018 decision 4a).

use honba_entities::{Currency, Instrument, InstrumentKind};
use honba_market::null::NullMarketProfile;
use honba_market::profile::MarketProfile;
use honba_market::{MarketError, QuantityViolation};
use honba_messages::{Exchange, InstrumentId};

fn instrument(lot: f64) -> Instrument {
    Instrument::new(
        InstrumentId::new("TEST", Exchange::new("NSE")),
        InstrumentKind::Equity,
        Currency::Inr,
        lot,
        0.05,
    )
}

#[test]
fn provider_rules_report_typed_lot_violation() {
    let profile = NullMarketProfile::default();
    let rules = profile.instrument_rules().rules_for(&instrument(25.0));
    assert_eq!(rules.validate_quantity(50.0), Ok(()));
    let err = rules.validate_quantity(30.0).unwrap_err();
    assert_eq!(
        err,
        QuantityViolation::NotLotMultiple {
            quantity: 30.0,
            lot: 25.0
        }
    );
    assert!(matches!(
        MarketError::from(err),
        MarketError::RuleViolation(_)
    ));
}

#[test]
fn provider_rules_report_typed_below_min() {
    let profile = NullMarketProfile::default();
    let rules = profile.instrument_rules().rules_for(&instrument(25.0));
    assert_eq!(
        rules.validate_quantity(10.0),
        Err(QuantityViolation::BelowMin {
            quantity: 10.0,
            min: 25.0
        })
    );
}

#[cfg(feature = "india")]
#[test]
fn india_profile_reports_typed_violations() {
    use honba_market::india::profile::IndiaMarketProfile;
    let profile = IndiaMarketProfile::default();
    let rules = profile.instrument_rules().rules_for(&instrument(1.0));
    assert_eq!(rules.validate_quantity(7.0), Ok(()));
    assert_eq!(
        rules.validate_quantity(0.5),
        Err(QuantityViolation::BelowMin {
            quantity: 0.5,
            min: 1.0
        })
    );
    assert_eq!(
        rules.validate_quantity(1.5),
        Err(QuantityViolation::NotLotMultiple {
            quantity: 1.5,
            lot: 1.0
        })
    );
    let capped = rules.clone().with_max_quantity(10.0);
    assert_eq!(
        capped.validate_quantity(11.0),
        Err(QuantityViolation::OverFreeze {
            quantity: 11.0,
            max: 10.0
        })
    );
}
