#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]

//! Market data catalog, Parquet readers/writers, and report exporters for Honba.

pub mod bhavcopy;
pub mod dataset;
pub mod export;
/// Data import sources and loaders
pub mod import;
/// Dated instrument snapshots (E1-S3).
pub mod instruments;
pub mod live_quotes;
/// A persistent Parquet tick store (ADR 0030).
pub mod parquet_ticks;
pub mod reader;
pub mod ticks;

pub use dataset::{
    ColumnarSlice, ColumnarSliceBuilder, Dataset, DatasetBuildError, DatasetChunk, DatasetFeed,
    StreamedDatasetFeed,
};
pub use export::{
    CsvReportWriter, ExportError, JsonReportWriter, MarkdownReportWriter, ReportWriter,
};
pub use import::parquet_source::{ParquetBarSource, ParquetError};
pub use instruments::{snapshot_schema, CatalogError, CatalogInstrumentMaster};
pub use live_quotes::LiveQuoteStore;
pub use parquet_ticks::{tick_schema, write_tick_file, ParquetTickStore, TickStoreError, TICK_DIR};
pub use reader::{DatasetReader, ReaderError};
pub use ticks::InMemoryTickReader;

#[cfg(test)]
mod tests;
