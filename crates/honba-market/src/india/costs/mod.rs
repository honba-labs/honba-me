//! Transaction cost models for Indian markets.

pub mod cash_equity;
pub mod model;
pub mod source;
pub mod stt;

use std::sync::Arc;

use chrono::NaiveDate;

use crate::costs::DatedCostScheduleSource;

pub use cash_equity::NseCashEquitySchedule;
pub use model::{CostBreakdown, CostModel, Segment};
pub use source::CostModelSource;
pub use stt::SttRates;

/// The date from which the current (FY 2025-26) NSE cash-equity delivery
/// schedule is documented to apply.
///
/// Historical statutory rates are not yet sourced, so the dated convenience in
/// this module holds only this single entry; an earlier date has no schedule.
pub const NSE_CASH_EQUITY_DELIVERY_EFFECTIVE_FROM: &str = "2025-04-01";

/// The documented effective date of the current NSE cash-equity delivery
/// schedule, parsed from [`NSE_CASH_EQUITY_DELIVERY_EFFECTIVE_FROM`].
pub fn nse_cash_equity_delivery_effective_from() -> NaiveDate {
    NaiveDate::parse_from_str(NSE_CASH_EQUITY_DELIVERY_EFFECTIVE_FROM, "%Y-%m-%d")
        .expect("valid effective date")
}

/// A [`DatedCostScheduleSource`] holding the single current NSE cash-equity
/// delivery schedule, effective from [`nse_cash_equity_delivery_effective_from`].
///
/// Historical schedules are still to be sourced; until then the source errors
/// for any earlier date.
pub fn nse_cash_equity_delivery_source() -> DatedCostScheduleSource {
    DatedCostScheduleSource::new(vec![(
        nse_cash_equity_delivery_effective_from(),
        Arc::new(NseCashEquitySchedule::delivery()),
    )])
}

/// Named charge constant for Securities Transaction Tax.
pub const CHARGE_STT: &str = "stt";
/// Named charge constant for exchange transaction fees.
pub const CHARGE_EXCHANGE_FEE: &str = "exchange_fee";
/// Named charge constant for Goods and Services Tax.
pub const CHARGE_GST: &str = "gst";
/// Named charge constant for stamp duty.
pub const CHARGE_STAMP_DUTY: &str = "stamp_duty";
/// Named charge constant for SEBI regulatory turnover fees.
pub const CHARGE_SEBI_FEE: &str = "sebi_fee";
/// Named charge constant for broker commissions.
pub const CHARGE_BROKERAGE: &str = "brokerage";
/// Named charge constant for the investor protection fund contribution.
pub const CHARGE_IPFT: &str = "ipft";
