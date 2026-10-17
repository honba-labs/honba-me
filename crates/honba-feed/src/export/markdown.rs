//! Markdown report writer.

use std::io::Write;

use honba_analytics::PerformanceReport;

use crate::export::error::Result;
use crate::export::writer::ReportWriter;

/// Renders a [`PerformanceReport`] as a human-readable Markdown document.
pub struct MarkdownReportWriter<W: Write> {
    inner: W,
}

impl<W: Write> MarkdownReportWriter<W> {
    /// Creates a Markdown writer.
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

fn opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.4}"),
        None => "-".to_string(),
    }
}

impl<W: Write> ReportWriter for MarkdownReportWriter<W> {
    fn write(&mut self, r: &PerformanceReport) -> Result<()> {
        writeln!(self.inner, "# Performance Report\n")?;
        writeln!(self.inner, "## Trades\n")?;
        writeln!(self.inner, "| Metric | Value |")?;
        writeln!(self.inner, "|---|---|")?;
        writeln!(self.inner, "| Trades | {} |", r.trades.n_trades)?;
        writeln!(self.inner, "| Wins | {} |", r.trades.n_wins)?;
        writeln!(self.inner, "| Losses | {} |", r.trades.n_losses)?;
        writeln!(
            self.inner,
            "| Win rate | {:.2}% |",
            r.trades.win_rate * 100.0
        )?;
        writeln!(
            self.inner,
            "| Gross profit | {:.2} |",
            r.trades.gross_profit
        )?;
        writeln!(self.inner, "| Gross loss | {:.2} |", r.trades.gross_loss)?;
        writeln!(
            self.inner,
            "| Profit factor | {} |",
            opt(r.trades.profit_factor)
        )?;
        writeln!(self.inner, "| Total PnL | {:.2} |", r.trades.total_pnl)?;
        writeln!(self.inner, "| Expectancy | {:.4} |", r.trades.expectancy)?;
        writeln!(self.inner, "| Total fees | {:.2} |", r.trades.total_fees)?;

        writeln!(self.inner, "\n## Equity\n")?;
        writeln!(self.inner, "| Metric | Value |")?;
        writeln!(self.inner, "|---|---|")?;
        writeln!(self.inner, "| Periods | {} |", r.equity.n_periods)?;
        writeln!(
            self.inner,
            "| Total return | {:.2}% |",
            r.equity.total_return * 100.0
        )?;
        writeln!(
            self.inner,
            "| Annualized return | {:.2}% |",
            r.equity.annualized_return * 100.0
        )?;
        writeln!(
            self.inner,
            "| Annualized volatility | {:.2}% |",
            r.equity.annualized_volatility * 100.0
        )?;
        writeln!(self.inner, "| Sharpe | {} |", opt(r.equity.sharpe))?;
        writeln!(self.inner, "| Sortino | {} |", opt(r.equity.sortino))?;
        writeln!(
            self.inner,
            "| Max drawdown | {:.4} |",
            r.equity.max_drawdown
        )?;
        writeln!(
            self.inner,
            "| Max drawdown % | {:.2}% |",
            r.equity.max_drawdown_pct * 100.0
        )?;
        writeln!(self.inner, "| Calmar | {} |", opt(r.equity.calmar))?;

        Ok(())
    }
}
