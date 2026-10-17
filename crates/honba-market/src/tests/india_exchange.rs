//! Unit tests for `crate::india::exchange`.

use crate::india::exchange::{resolve_instrument, IndiaExchange, InstrumentParseError};

#[test]
fn exchange_parses_case_insensitively_and_displays_upper_case() {
    assert_eq!("nse".parse::<IndiaExchange>().unwrap(), IndiaExchange::Nse);
    assert_eq!(
        " BSE ".parse::<IndiaExchange>().unwrap(),
        IndiaExchange::Bse
    );
    assert_eq!(IndiaExchange::Bse.to_string(), "BSE");
}

#[test]
fn unknown_exchange_error_lists_supported_ones() {
    let err = "NYSE".parse::<IndiaExchange>().unwrap_err();
    let text = err.to_string();
    assert!(text.contains("NYSE") && text.contains("NSE, BSE"), "{text}");
}

#[test]
fn bare_symbol_defaults_to_nse() {
    let id = resolve_instrument("INFY", None).unwrap();
    assert_eq!((id.symbol(), id.exchange().as_str()), ("INFY", "NSE"));
}

#[test]
fn bare_symbol_uses_the_explicit_exchange() {
    let id = resolve_instrument("INFY", Some("BSE")).unwrap();
    assert_eq!(id.exchange().as_str(), "BSE");
}

#[test]
fn qualified_symbol_carries_its_exchange() {
    let id = resolve_instrument("bse:INFY", None).unwrap();
    assert_eq!((id.symbol(), id.exchange().as_str()), ("INFY", "BSE"));
}

#[test]
fn qualified_symbol_agrees_with_a_matching_flag() {
    let id = resolve_instrument("NSE:INFY", Some("nse")).unwrap();
    assert_eq!(id.exchange().as_str(), "NSE");
}

#[test]
fn conflicting_qualifier_and_flag_is_an_error() {
    let err = resolve_instrument("NSE:INFY", Some("BSE")).unwrap_err();
    assert!(matches!(err, InstrumentParseError::Conflict { .. }));
    let text = err.to_string();
    assert!(text.contains("NSE") && text.contains("BSE"), "{text}");
}

#[test]
fn unknown_exchange_is_rejected_in_either_position() {
    assert!(resolve_instrument("NYSE:IBM", None).is_err());
    assert!(resolve_instrument("IBM", Some("NYSE")).is_err());
}

#[test]
fn malformed_symbols_are_rejected() {
    for bad in ["", ":INFY", "NSE:", "NSE:A:B", "NSE: "] {
        assert!(
            matches!(
                resolve_instrument(bad, None),
                Err(InstrumentParseError::InvalidSymbol(_))
            ),
            "{bad:?}"
        );
    }
}
