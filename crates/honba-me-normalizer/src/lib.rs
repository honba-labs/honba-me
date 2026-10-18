//! Symbol mapping, normalizers, and canonical exchange routing.

pub use honba_core::{Exchange, InstrumentId};
pub use honba_me_core::SymbolMapper;

/// Parse broker ticker symbol with exchange suffix (e.g., "RELIANCE$NSE", "INFY:NSE").
pub fn parse_broker_symbol(s: &str) -> Option<InstrumentId> {
    let mapper = SymbolMapper::new();
    mapper.to_canonical(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalizer_parsing() {
        let inst = parse_broker_symbol("TCS$NSE").unwrap();
        assert_eq!(inst.symbol, "TCS");
        assert_eq!(inst.exchange, Exchange::Nse);

        let inst_fyers = parse_broker_symbol("NSE:SBIN-EQ").unwrap();
        assert_eq!(inst_fyers.symbol, "SBIN");
        assert_eq!(inst_fyers.exchange, Exchange::Nse);
    }
}
