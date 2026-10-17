//! Securities Transaction Tax rates.

use honba_messages::OrderSide;

/// Securities Transaction Tax rates for a given effective period.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SttRates {
    /// Delivery equity, both sides.
    pub equity_delivery: f64,
    /// Intraday equity, sell side only.
    pub equity_intraday_sell: f64,
    /// Equity futures, sell side only.
    pub equity_futures_sell: f64,
    /// Equity options, sell side only (premium).
    pub equity_options_sell: f64,
    /// Equity options, exercised (intrinsic).
    pub equity_options_exercise: f64,
}

impl SttRates {
    /// Creates a rate table.
    pub const fn new(
        equity_delivery: f64,
        equity_intraday_sell: f64,
        equity_futures_sell: f64,
        equity_options_sell: f64,
        equity_options_exercise: f64,
    ) -> Self {
        Self {
            equity_delivery,
            equity_intraday_sell,
            equity_futures_sell,
            equity_options_sell,
            equity_options_exercise,
        }
    }

    /// Returns the rate applied to a delivery equity fill.
    pub fn for_equity_delivery(&self, _side: OrderSide) -> f64 {
        self.equity_delivery
    }

    /// Returns the rate applied to an intraday equity fill.
    pub fn for_equity_intraday(&self, side: OrderSide) -> f64 {
        match side {
            OrderSide::Sell => self.equity_intraday_sell,
            _ => 0.0,
        }
    }

    /// Returns the rate applied to a futures fill.
    pub fn for_equity_futures(&self, side: OrderSide) -> f64 {
        match side {
            OrderSide::Sell => self.equity_futures_sell,
            _ => 0.0,
        }
    }

    /// Returns the rate applied to an options fill.
    pub fn for_equity_options(&self, side: OrderSide) -> f64 {
        match side {
            OrderSide::Sell => self.equity_options_sell,
            _ => 0.0,
        }
    }
}

impl Default for SttRates {
    fn default() -> Self {
        Self {
            equity_delivery: 0.001,
            equity_intraday_sell: 0.00025,
            equity_futures_sell: 0.0002,
            equity_options_sell: 0.001,
            equity_options_exercise: 0.00125,
        }
    }
}
