//! DuckDB views over the Parquet lakehouse.
//!
//! The lakehouse is plain Hive-partitioned Parquet, so DuckDB can query it **in place** —
//! zero-copy, with partition pruning on `exchange` / `date` / `timeframe` / `year` / `month` —
//! from the `duckdb` CLI, Python (`honba-data`, notebooks) or any DuckDB client. This module
//! generates the `CREATE VIEW` statements; it deliberately does not link the DuckDB C++
//! engine into the Rust build (a multi-minute compile for no runtime benefit here).
//!
//! ```text
//! duckdb -init <(honba-cli lake views) -c "SELECT symbol, count(*) FROM ticks GROUP BY 1"
//! ```
//! or from Rust: `std::fs::write("views.sql", DuckDbViews::new(root).script())`.

use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

/// Generator for the `ticks` and `bars` views.
#[derive(Debug, Clone)]
pub struct DuckDbViews {
    root: PathBuf,
}

/// SQL string literal.
fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

impl DuckDbViews {
    /// Views over the lakehouse rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn glob(&self, pattern: &str) -> String {
        lit(&format!("{}/{pattern}", self.root.display()))
    }

    /// `ticks`: every tick file. Partition columns `exchange` (from the path) and `date` are
    /// exposed; `exchange_ts` / `receive_ts` are `TIMESTAMP WITH TIME ZONE`, prices are
    /// `DECIMAL(18,4)`.
    pub fn ticks_view(&self) -> String {
        format!(
            "CREATE OR REPLACE VIEW ticks AS SELECT * FROM read_parquet({}, \
             hive_partitioning = true, union_by_name = true);",
            self.glob("ticks/*/*/*.parquet")
        )
    }

    /// `bars`: every bar file across timeframes. Partition columns `timeframe`, `year`, `month`
    /// (and `exchange`) are exposed.
    pub fn bars_view(&self) -> String {
        format!(
            "CREATE OR REPLACE VIEW bars AS SELECT * FROM read_parquet({}, \
             hive_partitioning = true, union_by_name = true);",
            self.glob("bars/*/*/*/*/*.parquet")
        )
    }

    /// All statements, in order.
    pub fn statements(&self) -> Vec<String> {
        vec![self.ticks_view(), self.bars_view()]
    }

    /// The statements as one newline-separated script.
    pub fn script(&self) -> String {
        self.statements().join("\n") + "\n"
    }

    /// Writes [`script`](Self::script) to `path`.
    pub fn write_script(&self, path: &Path) -> io::Result<()> {
        std::fs::write(path, self.script())
    }

    /// A pruned query for one symbol-day of ticks.
    pub fn tick_query(&self, symbol: &str, date: NaiveDate) -> String {
        format!(
            "SELECT * FROM ticks WHERE date = {} AND symbol = {} ORDER BY exchange_ts;",
            lit(&date.to_string()),
            lit(symbol)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_glob_the_hive_layout_and_escape_quotes() {
        let v = DuckDbViews::new("/data/it's");
        let s = v.script();
        assert!(s.contains("'/data/it''s/ticks/*/*/*.parquet'"), "{s}");
        assert!(s.contains("'/data/it''s/bars/*/*/*/*/*.parquet'"), "{s}");
        assert!(s.contains("hive_partitioning = true"));
        assert_eq!(v.statements().len(), 2);
    }

    #[test]
    fn tick_query_is_partition_pruned_and_injection_safe() {
        let q = DuckDbViews::new("/d")
            .tick_query("O'RLY", NaiveDate::from_ymd_opt(2026, 10, 20).unwrap());
        assert!(q.contains("date = '2026-10-20'"));
        assert!(q.contains("symbol = 'O''RLY'"));
    }
}
