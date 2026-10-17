//! Shared batch-3 vectors (donchian, williams_r, stochastic).
//!
//! Inputs are synthetic OHLC triples; outputs are the streaming results pinned
//! to a Fraction-exact reference (derived independently in Python). Both the
//! streaming indicators and a pure O(n) reference read from this one file.

use honba_indicators::{Donchian, DonchianValue, Indicator, Stochastic, WilliamsR};
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};
use serde_json::Value;

const FIXTURE: &str = include_str!("vectors_batch3.json");

fn any_instrument() -> InstrumentId {
    InstrumentId::new("X", Exchange::new("NSE"))
}

fn bar(h: f64, l: f64, c: f64) -> Bar {
    let bt = BarType::new(
        any_instrument(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
    );
    Bar::new(
        bt,
        c,
        h,
        l,
        c,
        1.0,
        UnixNanos::from_u64(1),
        UnixNanos::from_u64(1),
    )
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn bars_from_inputs(inputs: &[Value]) -> Vec<Bar> {
    inputs
        .iter()
        .map(|row| {
            bar(
                row["high"].as_f64().unwrap(),
                row["low"].as_f64().unwrap(),
                row["close"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn batch3_vectors_donchian() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    let bars = bars_from_inputs(fx["bars"].as_array().unwrap());
    let period = fx["donchian"]["period"].as_u64().unwrap() as usize;
    let expected = fx["donchian"]["expected"].as_array().unwrap();

    let mut d = Donchian::new(period);
    for (i, b) in bars.iter().enumerate() {
        let got = d.update(b);
        match &expected[i] {
            Value::Null => assert_eq!(got, None, "donchian at {i}"),
            Value::Object(obj) => {
                let gv = got.unwrap();
                assert!(
                    close(gv.upper, obj["upper"].as_f64().unwrap()),
                    "upper at {i}"
                );
                assert!(
                    close(gv.lower, obj["lower"].as_f64().unwrap()),
                    "lower at {i}"
                );
                assert!(
                    close(gv.middle, obj["middle"].as_f64().unwrap()),
                    "middle at {i}"
                );
            }
            _ => unreachable!(),
        }
        if let Some(DonchianValue {
            upper,
            lower,
            middle,
        }) = got
        {
            assert_eq!(
                d.value().unwrap(),
                DonchianValue {
                    upper,
                    lower,
                    middle
                }
            );
        }
    }
}

#[test]
fn batch3_vectors_williams_r() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    let bars = bars_from_inputs(fx["bars"].as_array().unwrap());
    let period = fx["williams_r"]["period"].as_u64().unwrap() as usize;
    let expected = fx["williams_r"]["expected"].as_array().unwrap();

    let mut w = WilliamsR::new(period);
    for (i, b) in bars.iter().enumerate() {
        let got = w.update(b);
        match &expected[i] {
            Value::Null => assert_eq!(got, None, "williams at {i}"),
            Value::Number(n) => {
                let e = n.as_f64().unwrap();
                let g = got.unwrap();
                assert!(close(g, e), "williams at {i}: {g} != {e}");
                assert!(close(w.value().unwrap(), e));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn batch3_vectors_stochastic() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    let bars = bars_from_inputs(fx["bars"].as_array().unwrap());
    let period = fx["stochastic"]["period"].as_u64().unwrap() as usize;
    let d_period = fx["stochastic"]["d_period"].as_u64().unwrap() as usize;
    let expected = fx["stochastic"]["expected"].as_array().unwrap();

    let mut s = Stochastic::new(period, d_period);
    for (i, b) in bars.iter().enumerate() {
        let got = s.update(b);
        match &expected[i] {
            Value::Null => assert_eq!(got, None, "stochastic at {i}"),
            Value::Object(obj) => {
                let gv = got.unwrap();
                assert!(close(gv.k, obj["k"].as_f64().unwrap()), "k at {i}");
                match (gv.d, &obj["d"]) {
                    (None, Value::Null) => {}
                    (Some(gd), Value::Number(n)) => {
                        assert!(close(gd, n.as_f64().unwrap()), "d at {i}")
                    }
                    _ => panic!("d mismatch at {i}"),
                }
                assert_eq!(s.value().unwrap(), gv);
            }
            _ => unreachable!(),
        }
    }
}
