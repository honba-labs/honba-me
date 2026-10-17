use honba_analytics::{EquityStats, PerformanceReport, TradeStats};

use crate::export::{CsvReportWriter, JsonReportWriter, MarkdownReportWriter, ReportWriter};

fn sample_report() -> PerformanceReport {
    PerformanceReport {
        trades: TradeStats {
            n_trades: 10,
            n_wins: 6,
            n_losses: 4,
            n_flat: 0,
            win_rate: 0.60,
            gross_profit: 1200.0,
            gross_loss: 400.0,
            profit_factor: Some(3.0),
            avg_win: Some(200.0),
            avg_loss: Some(-100.0),
            total_pnl: 800.0,
            expectancy: 80.0,
            total_fees: 15.0,
        },
        equity: EquityStats {
            n_periods: 252,
            total_return: 0.25,
            annualized_return: 0.25,
            annualized_volatility: 0.15,
            sharpe: Some(1.6667),
            sortino: Some(2.1000),
            max_drawdown: 50.0,
            max_drawdown_pct: 0.05,
            calmar: Some(5.0),
        },
    }
}

#[test]
fn test_json_report_writer_compact_and_pretty() {
    let report = sample_report();

    // Compact
    let mut compact_buf = Vec::new();
    let mut compact_writer = JsonReportWriter::new(&mut compact_buf);
    compact_writer.write(&report).expect("compact write");
    let compact_str = String::from_utf8(compact_buf).expect("utf8");
    assert!(!compact_str.contains('\n'));

    let deserialized: PerformanceReport =
        serde_json::from_str(&compact_str).expect("deserialize compact");
    assert_eq!(deserialized, report);

    // Pretty
    let mut pretty_buf = Vec::new();
    let mut pretty_writer = JsonReportWriter::pretty(&mut pretty_buf);
    pretty_writer.write(&report).expect("pretty write");
    let pretty_str = String::from_utf8(pretty_buf).expect("utf8");
    assert!(pretty_str.contains('\n'));

    let deserialized_pretty: PerformanceReport =
        serde_json::from_str(&pretty_str).expect("deserialize pretty");
    assert_eq!(deserialized_pretty, report);
}

#[test]
fn test_csv_report_writer() {
    let report = sample_report();
    let mut buf = Vec::new();
    let mut writer = CsvReportWriter::new(&mut buf);
    writer.write(&report).expect("csv write");

    let csv_str = String::from_utf8(buf).expect("utf8");
    let lines: Vec<&str> = csv_str.lines().collect();

    assert_eq!(lines[0], "metric,value");
    assert!(lines.contains(&"trades.n_trades,10"));
    assert!(lines.contains(&"trades.win_rate,0.6"));
    assert!(lines.contains(&"trades.profit_factor,3"));
    assert!(lines.contains(&"equity.n_periods,252"));
    assert!(lines.contains(&"equity.total_return,0.25"));
    assert!(lines.contains(&"equity.sharpe,1.6667"));
}

#[test]
fn test_markdown_report_writer() {
    let report = sample_report();
    let mut buf = Vec::new();
    let mut writer = MarkdownReportWriter::new(&mut buf);
    writer.write(&report).expect("markdown write");

    let md_str = String::from_utf8(buf).expect("utf8");
    assert!(md_str.contains("# Performance Report"));
    assert!(md_str.contains("## Trades"));
    assert!(md_str.contains("| Win rate | 60.00% |"));
    assert!(md_str.contains("## Equity"));
    assert!(md_str.contains("| Sharpe | 1.6667 |"));
}
