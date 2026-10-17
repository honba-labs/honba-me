//! JSON report writer.

use std::io::Write;

use honba_analytics::PerformanceReport;

use crate::export::error::Result;
use crate::export::writer::ReportWriter;

/// Writes a [`PerformanceReport`] as JSON.
pub struct JsonReportWriter<W: Write> {
    inner: W,
    pretty: bool,
}

impl<W: Write> JsonReportWriter<W> {
    /// Creates a compact JSON writer.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            pretty: false,
        }
    }

    /// Creates a pretty-printed JSON writer.
    pub fn pretty(inner: W) -> Self {
        Self {
            inner,
            pretty: true,
        }
    }
}

impl<W: Write> ReportWriter for JsonReportWriter<W> {
    fn write(&mut self, report: &PerformanceReport) -> Result<()> {
        if self.pretty {
            serde_json::to_writer_pretty(&mut self.inner, report)?;
        } else {
            serde_json::to_writer(&mut self.inner, report)?;
        }
        Ok(())
    }
}
