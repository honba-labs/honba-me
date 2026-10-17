//! Acceptance test suite for Indian F&O instrument grammar and market registry resolution (E1-S2).

use chrono::NaiveDate;
use honba_entities::{Currency, InstrumentKind, OptionType};
use honba_market::india::fno::{
    default_fno_lot_size, format_fno_symbol, parse_fno_symbol, FnoContract, FnoExpiry,
};
use honba_market::profile::MarketRegistry;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

#[test]
fn symbol_round_trip_property() {
    let test_symbols = [
        // Monthly equity futures
        "RELIANCE24OCTFUT",
        "TCS24NOVFUT",
        "INFY25JANFUT",
        "M&M24OCTFUT",
        // Monthly index futures
        "NIFTY24OCTFUT",
        "BANKNIFTY24OCTFUT",
        "FINNIFTY24NOVFUT",
        "MIDCPNIFTY24DECFUT",
        // Weekly index futures
        "NIFTY24O17FUT",
        "BANKNIFTY24109FUT",
        "FINNIFTY24N06FUT",
        // Monthly index options
        "NIFTY24OCT24000CE",
        "NIFTY24OCT24000PE",
        "BANKNIFTY24OCT51000PE",
        "FINNIFTY24OCT21500CE",
        // Weekly index options across month encodings (1..9, O, N, D)
        "NIFTY2410921000CE",
        "NIFTY2421522000PE",
        "NIFTY2432022500CE",
        "NIFTY2441823000PE",
        "NIFTY2451623500CE",
        "NIFTY2462024000PE",
        "NIFTY2471824500CE",
        "NIFTY2481525000PE",
        "NIFTY2490525000CE",
        "NIFTY24O1724000CE",
        "BANKNIFTY24N0651500PE",
        "NIFTY24D1224500PE",
        // Monthly equity options
        "RELIANCE24OCT3000CE",
        "TCS24OCT4000PE",
        "INFY24NOV1850CE",
        "HDFCBANK24DEC1600PE",
        "CESC24OCT180CE",
        "CENTURYPLY24OCT750PE",
        // Options with decimal strikes
        "RELIANCE24OCT3000.5CE",
        "TCS24OCT4000.25PE",
    ];

    for symbol in test_symbols {
        let contract =
            parse_fno_symbol(symbol).unwrap_or_else(|e| panic!("failed to parse {symbol}: {e}"));
        let formatted = format_fno_symbol(&contract);
        assert_eq!(
            formatted, symbol,
            "formatting round-trip mismatch for {symbol}"
        );

        let reparsed = parse_fno_symbol(&formatted)
            .unwrap_or_else(|e| panic!("failed to reparse {formatted}: {e}"));
        assert_eq!(
            reparsed, contract,
            "contract equality round-trip mismatch for {symbol}"
        );
    }
}

#[test]
fn ce_suffix_ambiguity() {
    // Underlyings that end with 'CE' or 'PE', or contain them as substrings:
    // 1. Bare cash equity symbols must NOT be parsed as F&O contracts.
    assert!(
        parse_fno_symbol("CESC").is_err(),
        "CESC must not parse as an F&O option contract"
    );
    assert!(
        parse_fno_symbol("CENTURYPLY").is_err(),
        "CENTURYPLY must not parse as an F&O option contract"
    );
    assert!(
        parse_fno_symbol("PETRONET").is_err(),
        "PETRONET must not parse as an F&O option contract"
    );
    assert!(
        parse_fno_symbol("FINPIPE").is_err(),
        "FINPIPE must not parse as an F&O option contract"
    );
    assert!(
        parse_fno_symbol("CANFINHOME").is_err(),
        "CANFINHOME must not parse as an F&O option contract"
    );

    // 2. Options on these underlyings must resolve without ambiguity.
    let cesc_ce = parse_fno_symbol("CESC24OCT180CE").expect("valid CESC option");
    match cesc_ce {
        FnoContract::Option {
            underlying,
            expiry,
            strike,
            option_type,
        } => {
            assert_eq!(underlying, "CESC");
            assert_eq!(
                expiry,
                FnoExpiry::Monthly {
                    year: 2024,
                    month: 10
                }
            );
            assert_eq!(strike, 180.0);
            assert_eq!(option_type, OptionType::Call);
        }
        _ => panic!("expected Option contract"),
    }

    let centuryply_pe = parse_fno_symbol("CENTURYPLY24OCT750PE").expect("valid CENTURYPLY option");
    match centuryply_pe {
        FnoContract::Option {
            underlying,
            expiry,
            strike,
            option_type,
        } => {
            assert_eq!(underlying, "CENTURYPLY");
            assert_eq!(
                expiry,
                FnoExpiry::Monthly {
                    year: 2024,
                    month: 10
                }
            );
            assert_eq!(strike, 750.0);
            assert_eq!(option_type, OptionType::Put);
        }
        _ => panic!("expected Option contract"),
    }

    let lice_pe = parse_fno_symbol("LICEHSGFIN24OCT700PE").expect("valid LICEHSGFIN option");
    assert_eq!(lice_pe.underlying(), "LICEHSGFIN");
    assert_eq!(lice_pe.strike(), Some(700.0));
    assert_eq!(lice_pe.option_type(), Some(OptionType::Put));
}

#[test]
fn weekly_vs_monthly() {
    // 1. Monthly options
    let monthly_opt = parse_fno_symbol("NIFTY24OCT24000CE").expect("valid monthly option");
    assert!(monthly_opt.expiry().is_monthly());
    assert!(!monthly_opt.expiry().is_weekly());
    assert_eq!(monthly_opt.expiry().day(), None);
    assert_eq!(monthly_opt.expiry().month(), 10);
    assert_eq!(monthly_opt.expiry().year(), 2024);
    assert_eq!(monthly_opt.expiry().expiry_date(), Some(date(2024, 10, 31)));

    // 2. Weekly options
    let weekly_opt_oct = parse_fno_symbol("NIFTY24O1724000CE").expect("valid weekly Oct option");
    assert!(weekly_opt_oct.expiry().is_weekly());
    assert!(!weekly_opt_oct.expiry().is_monthly());
    assert_eq!(weekly_opt_oct.expiry().day(), Some(17));
    assert_eq!(weekly_opt_oct.expiry().month(), 10);
    assert_eq!(weekly_opt_oct.expiry().year(), 2024);
    assert_eq!(
        weekly_opt_oct.expiry().expiry_date(),
        Some(date(2024, 10, 17))
    );

    let weekly_opt_nov =
        parse_fno_symbol("BANKNIFTY24N0651500PE").expect("valid weekly Nov option");
    assert!(weekly_opt_nov.expiry().is_weekly());
    assert_eq!(weekly_opt_nov.expiry().day(), Some(6));
    assert_eq!(weekly_opt_nov.expiry().month(), 11);
    assert_eq!(weekly_opt_nov.expiry().year(), 2024);
    assert_eq!(
        weekly_opt_nov.expiry().expiry_date(),
        Some(date(2024, 11, 6))
    );

    let weekly_opt_jan = parse_fno_symbol("FINNIFTY2410921000CE").expect("valid weekly Jan option");
    assert!(weekly_opt_jan.expiry().is_weekly());
    assert_eq!(weekly_opt_jan.expiry().day(), Some(9));
    assert_eq!(weekly_opt_jan.expiry().month(), 1);
    assert_eq!(weekly_opt_jan.expiry().year(), 2024);

    let weekly_opt_dec = parse_fno_symbol("NIFTY24D1224500PE").expect("valid weekly Dec option");
    assert!(weekly_opt_dec.expiry().is_weekly());
    assert_eq!(weekly_opt_dec.expiry().day(), Some(12));
    assert_eq!(weekly_opt_dec.expiry().month(), 12);

    // 3. Futures comparison
    let monthly_fut = parse_fno_symbol("NIFTY24OCTFUT").expect("valid monthly future");
    assert!(monthly_fut.expiry().is_monthly());
    assert_eq!(monthly_fut.expiry().day(), None);

    let weekly_fut = parse_fno_symbol("NIFTY24O17FUT").expect("valid weekly future");
    assert!(weekly_fut.expiry().is_weekly());
    assert_eq!(weekly_fut.expiry().day(), Some(17));
}

#[test]
fn catalog_resolves_fno_symbol() {
    let registry = MarketRegistry::default_registry();
    let profile = registry
        .get("nse_bse")
        .expect("nse_bse profile must be registered in default registry");

    // 1. Resolve Monthly Option
    let inst_opt = profile
        .resolve_instrument("NIFTY24OCT24000CE")
        .expect("resolves monthly option");
    assert_eq!(inst_opt.id().symbol(), "NIFTY24OCT24000CE");
    assert_eq!(inst_opt.id().exchange().as_str(), "NSE");
    assert_eq!(inst_opt.kind(), InstrumentKind::Option);
    assert_eq!(inst_opt.currency(), Currency::Inr);
    assert_eq!(inst_opt.lot_size(), default_fno_lot_size("NIFTY"));
    assert_eq!(inst_opt.tick_size(), 0.05);
    assert_eq!(inst_opt.underlying(), Some("NIFTY"));
    assert_eq!(inst_opt.strike(), Some(24000.0));
    assert_eq!(inst_opt.option_type(), Some(OptionType::Call));
    assert_eq!(inst_opt.expiry(), Some(date(2024, 10, 31)));
    assert!(inst_opt.is_derivative());

    // 2. Resolve Weekly Option
    let inst_w = profile
        .resolve_instrument("NIFTY24O1724000CE")
        .expect("resolves weekly option");
    assert_eq!(inst_w.kind(), InstrumentKind::Option);
    assert_eq!(inst_w.expiry(), Some(date(2024, 10, 17)));
    assert_eq!(inst_w.strike(), Some(24000.0));
    assert_eq!(inst_w.option_type(), Some(OptionType::Call));
    assert!(inst_w.is_derivative());

    // 3. Resolve Monthly Future
    let inst_fut = profile
        .resolve_instrument("RELIANCE24OCTFUT")
        .expect("resolves future");
    assert_eq!(inst_fut.kind(), InstrumentKind::Future);
    assert_eq!(inst_fut.underlying(), Some("RELIANCE"));
    assert_eq!(inst_fut.strike(), None);
    assert_eq!(inst_fut.option_type(), None);
    assert_eq!(inst_fut.expiry(), Some(date(2024, 10, 31)));
    assert!(inst_fut.is_derivative());

    // 4. Resolve Cash Equity
    let inst_eq = profile
        .resolve_instrument("RELIANCE")
        .expect("resolves cash equity");
    assert_eq!(inst_eq.kind(), InstrumentKind::Equity);
    assert_eq!(inst_eq.underlying(), None);
    assert_eq!(inst_eq.strike(), None);
    assert_eq!(inst_eq.option_type(), None);
    assert_eq!(inst_eq.expiry(), None);
    assert_eq!(inst_eq.lot_size(), 1.0);
    assert_eq!(inst_eq.tick_size(), 0.05);
    assert!(!inst_eq.is_derivative());

    // 5. Explicit exchange qualifier
    let inst_bse = profile
        .resolve_instrument("BSE:TCS")
        .expect("resolves BSE qualified symbol");
    assert_eq!(inst_bse.id().exchange().as_str(), "BSE");
    assert_eq!(inst_bse.id().symbol(), "TCS");
}
