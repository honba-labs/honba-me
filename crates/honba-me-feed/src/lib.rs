#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]

//! Market data catalog, Parquet readers/writers, and report exporters for Honba.

pub mod bhavcopy;
/// Bridge from legacy `f64` wire types to canonical `honba_core` types.
pub mod canonical;
pub mod dataset;
pub mod export;
/// Data import sources and loaders
pub mod import;
/// Dated instrument snapshots (E1-S3).
pub mod instruments;
/// Parquet Lakehouse v2: canonical, exact-decimal, Hive-partitioned storage.
pub mod lakehouse;
pub mod live_quotes;
/// A persistent Parquet tick store (ADR 0030).
pub mod parquet_ticks;
pub mod reader;
/// Live tick recorder daemon flushing to the lakehouse.
pub mod recorder;
pub mod ticks;

pub use dataset::{
    ColumnarSlice, ColumnarSliceBuilder, Dataset, DatasetBuildError, DatasetChunk, DatasetFeed,
    StreamedDatasetFeed,
};
pub use export::{
    CsvReportWriter, ExportError, JsonReportWriter, MarkdownReportWriter, ReportWriter,
};
pub use import::parquet_source::{ParquetBarSource, ParquetError};
pub use instruments::{CatalogError, CatalogInstrumentMaster, snapshot_schema};
pub use lakehouse::{Lakehouse, LakehouseCompression, LakehouseError};
pub use live_quotes::LiveQuoteStore;
pub use recorder::{RecorderConfig, RecorderStats, TickRecorder};
pub use parquet_ticks::{ParquetTickStore, TICK_DIR, TickStoreError, tick_schema, write_tick_file};
pub use reader::{DatasetReader, ReaderError};
pub use ticks::InMemoryTickReader;

/// Local port traits and data types (replaces the deleted `honba-kokoro-ports`).
pub mod ports;
/// Local market-data wire types (replaces the deleted `honba-kokoro-{entities,messages}`).
pub mod wire;

#[cfg(test)]
mod tests;
