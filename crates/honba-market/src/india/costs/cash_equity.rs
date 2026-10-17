//! NSE cash-equity (delivery and intraday) fee schedules as a [`CostSchedule`].
//!
//! The legs mirror the Python reference `honba.markets.india.costs` (2025-26 discount-broker
//! schedule): brokerage is a percentage of notional capped per order, STT applies to sells, stamp
//! duty to buys, and GST is charged on brokerage, exchange, SEBI and IPFT charges. Every charge is
//! returned unrounded; a consumer that books money rounds each leg to minor units once.

use honba_messages::OrderSide;

use super::{
    CHARGE_BROKERAGE, CHARGE_EXCHANGE_FEE, CHARGE_GST, CHARGE_IPFT, CHARGE_SEBI_FEE,
    CHARGE_STAMP_DUTY, CHARGE_STT,
};
use crate::costs::{CostSchedule, FeeBreakdown, MarketSegment};

/// The rates of one NSE cash-equity product (all fractions of notional except the cap).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NseCashEquitySchedule {
    /// STT on the sell side.
    pub stt_sell: f64,
    /// Stamp duty on the buy side.
    pub stamp_buy: f64,
    /// Exchange transaction charge.
    pub exchange: f64,
    /// SEBI turnover fee.
    pub sebi: f64,
    /// Investor protection fund contribution.
    pub ipft: f64,
    /// Brokerage as a fraction of notional.
    pub brokerage_pct: f64,
    /// Brokerage cap per order, in major units.
    pub brokerage_cap: f64,
    /// GST rate on brokerage, exchange, SEBI and IPFT.
    pub gst_rate: f64,
}

impl NseCashEquitySchedule {
    /// Equity delivery (CNC).
    pub const fn delivery() -> Self {
        Self {
            stt_sell: 0.0010,
            stamp_buy: 0.00015,
            exchange: 0.0000297,
            sebi: 0.000001,
            ipft: 0.000001,
            brokerage_pct: 0.0003,
            brokerage_cap: 20.0,
            gst_rate: 0.18,
        }
    }

    /// Equity intraday (MIS).
    pub const fn intraday() -> Self {
        Self {
            stt_sell: 0.00025,
            stamp_buy: 0.00003,
            ..Self::delivery()
        }
    }
}

impl CostSchedule for NseCashEquitySchedule {
    /// The segment is ignored: one value is one product. The cost is on the absolute notional; zero costs nothing.
    fn compute_costs(
        &self,
        _segment: &MarketSegment,
        side: OrderSide,
        notional: f64,
    ) -> FeeBreakdown {
        let notional = notional.abs();
        if notional <= 0.0 {
            return FeeBreakdown::empty();
        }
        let is_buy = side == OrderSide::Buy;
        let brokerage = (self.brokerage_pct * notional).min(self.brokerage_cap);
        let exchange = self.exchange * notional;
        let sebi = self.sebi * notional;
        let ipft = self.ipft * notional;
        FeeBreakdown::from_charges(
            [
                (CHARGE_BROKERAGE, brokerage),
                (
                    CHARGE_STT,
                    if is_buy {
                        0.0
                    } else {
                        self.stt_sell * notional
                    },
                ),
                (CHARGE_EXCHANGE_FEE, exchange),
                (CHARGE_SEBI_FEE, sebi),
                (CHARGE_IPFT, ipft),
                (
                    CHARGE_STAMP_DUTY,
                    if is_buy {
                        self.stamp_buy * notional
                    } else {
                        0.0
                    },
                ),
                (
                    CHARGE_GST,
                    self.gst_rate * (brokerage + exchange + sebi + ipft),
                ),
            ]
            .into_iter()
            .map(|(n, a)| crate::costs::Charge::new(n, a))
            .collect(),
        )
    }
}
