//! Flat CSV report writer.

use std::io::Write;

use honba_analytics::PerformanceReport;

use crate::export::error::Result;
use crate::export::writer::ReportWriter;

/// Writes a [`PerformanceReport`] as a two-column `metric,value` CSV.
pub struct CsvReportWriter<W: Write> {
    inner: W,
    header_written: bool,
}

impl<W: Write> CsvReportWriter<W> {
    /// Creates a CSV report writer.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            header_written: false,
        }
    }
}

fn opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x}"),
        None => String::new(),
    }
}

impl<W: Write> ReportWriter for CsvReportWriter<W> {
    fn write(&mut self, r: &PerformanceReport) -> Result<()> {
        if !self.header_written {
            writeln!(self.inner, "metric,value")?;
            self.header_written = true;
        }

        let rows: &[(&str, String)] = &[
            ("trades.n_trades", r.trades.n_trades.to_string()),
            ("trades.n_wins", r.trades.n_wins.to_string()),
            ("trades.n_losses", r.trades.n_losses.to_string()),
            ("trades.n_flat", r.trades.n_flat.to_string()),
            ("trades.win_rate", format!("{}", r.trades.win_rate)),
            ("trades.gross_profit", format!("{}", r.trades.gross_profit)),
            ("trades.gross_loss", format!("{}", r.trades.gross_loss)),
            ("trades.profit_factor", opt(r.trades.profit_factor)),
            ("trades.avg_win", opt(r.trades.avg_win)),
            ("trades.avg_loss", opt(r.trades.avg_loss)),
            ("trades.total_pnl", format!("{}", r.trades.total_pnl)),
            ("trades.expectancy", format!("{}", r.trades.expectancy)),
            ("trades.total_fees", format!("{}", r.trades.total_fees)),
            ("equity.n_periods", r.equity.n_periods.to_string()),
            ("equity.total_return", format!("{}", r.equity.total_return)),
            (
                "equity.annualized_return",
                format!("{}", r.equity.annualized_return),
            ),
            (
                "equity.annualized_volatility",
                format!("{}", r.equity.annualized_volatility),
            ),
            ("equity.sharpe", opt(r.equity.sharpe)),
            ("equity.sortino", opt(r.equity.sortino)),
            ("equity.max_drawdown", format!("{}", r.equity.max_drawdown)),
            (
                "equity.max_drawdown_pct",
                format!("{}", r.equity.max_drawdown_pct),
            ),
            ("equity.calmar", opt(r.equity.calmar)),
        ];

        for (k, v) in rows {
            writeln!(self.inner, "{k},{v}")?;
        }
        Ok(())
    }
}
