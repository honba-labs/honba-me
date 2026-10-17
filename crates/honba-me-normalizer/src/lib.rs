//! Symbol mapping and corporate actions.
use honba_kokoro_types::{Exchange, InstrumentId};

pub fn parse_broker_symbol(s: &str) -> Option<InstrumentId> {
    let mut parts = s.split('$');
    let sym = parts.next()?;
    let exch = parts.next()?;
    let exchange = match exch {
        "NSE" => Exchange::Nse,
        "BSE" => Exchange::Bse,
        _ => return None,
    };
    Some(InstrumentId {
        exchange,
        symbol: sym.to_string(),
        isin: None,
    })
}
