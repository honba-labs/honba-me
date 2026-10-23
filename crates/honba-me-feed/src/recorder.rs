//! Live tick recorder daemon.
//!
//! [`TickRecorder`] is a background tokio task that drains a stream of canonical
//! [`Tick`]s into rolling in-memory buffers and flushes them to the Hive-partitioned
//! [`Lakehouse`] as compressed Parquet. A flush happens when the buffer reaches
//! [`RecorderConfig::max_ticks`] (default 100 000) **or** every
//! [`RecorderConfig::flush_interval`] (default 60 s), whichever comes first, and once more
//! on shutdown.
//!
//! Parquet encoding runs on the blocking pool, so a slow disk never stalls the ingest loop.
//! If a flush fails the batch is kept and retried on the next flush; the buffer is capped at
//! [`RecorderConfig::max_buffered`], beyond which the *oldest* ticks are dropped and counted
//! in [`RecorderStats::dropped`] rather than growing memory without bound.

use std::sync::Arc;
use std::time::Duration;

use honba_core::Tick;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::lakehouse::{Lakehouse, LakehouseError};

/// Tuning for a [`TickRecorder`].
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Flush as soon as this many ticks are buffered.
    pub max_ticks: usize,
    /// Flush at least this often while ticks are buffered.
    pub flush_interval: Duration,
    /// Hard cap on buffered ticks while flushes are failing.
    pub max_buffered: usize,
    /// Capacity of the ingest channel.
    pub channel_capacity: usize,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            max_ticks: 100_000,
            flush_interval: Duration::from_secs(60),
            max_buffered: 1_000_000,
            channel_capacity: 65_536,
        }
    }
}

/// Counters reported when a recorder stops.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecorderStats {
    /// Ticks received.
    pub received: u64,
    /// Ticks durably written.
    pub written: u64,
    /// Parquet files written.
    pub files: u64,
    /// Successful flushes.
    pub flushes: u64,
    /// Flushes that failed (their ticks were retained and retried).
    pub failed_flushes: u64,
    /// Ticks dropped because the buffer cap was exceeded while flushes failed, or because a
    /// broadcast source lagged.
    pub dropped: u64,
}

/// Handle to a running recorder task.
pub struct TickRecorder {
    tx: mpsc::Sender<Tick>,
    join: JoinHandle<Result<RecorderStats, LakehouseError>>,
}

impl TickRecorder {
    /// Spawns the recorder on the current tokio runtime.
    pub fn spawn(lakehouse: Lakehouse, config: RecorderConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.channel_capacity.max(1));
        let join = tokio::spawn(run(lakehouse, config, rx));
        Self { tx, join }
    }

    /// A sender for feeding ticks. Clone freely; the recorder stops once the
    /// [`TickRecorder`] is shut down **and** all clones are dropped.
    pub fn sender(&self) -> mpsc::Sender<Tick> {
        self.tx.clone()
    }

    /// Queues one tick, waiting if the channel is full. Returns `false` if the recorder died.
    pub async fn record(&self, tick: Tick) -> bool {
        self.tx.send(tick).await.is_ok()
    }

    /// Subscribes the recorder to a `honba-me-core` style broadcast stream.
    ///
    /// Spawns a forwarder that ends when the broadcast closes; ticks missed by lagging are
    /// reported in [`RecorderStats::dropped`] only via the forwarder's return value.
    pub fn attach_broadcast(&self, mut rx: broadcast::Receiver<Tick>) -> JoinHandle<u64> {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut lagged = 0u64;
            loop {
                match rx.recv().await {
                    Ok(t) => {
                        if tx.send(t).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => lagged += n,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            lagged
        })
    }

    /// Stops accepting ticks, flushes everything buffered and returns the final stats.
    ///
    /// Completes once every [`sender`](Self::sender) clone is dropped too.
    pub async fn shutdown(self) -> Result<RecorderStats, LakehouseError> {
        let Self { tx, join } = self;
        drop(tx);
        match join.await {
            Ok(r) => r,
            Err(e) => Err(LakehouseError::Io(std::io::Error::other(e.to_string()))),
        }
    }
}

async fn flush(
    lakehouse: &Lakehouse,
    buf: &mut Vec<Tick>,
    stats: &mut RecorderStats,
) -> Result<(), LakehouseError> {
    if buf.is_empty() {
        return Ok(());
    }
    let batch = Arc::new(std::mem::take(buf));
    let (lh, job) = (lakehouse.clone(), Arc::clone(&batch));
    let result = tokio::task::spawn_blocking(move || lh.write_ticks(&job))
        .await
        .map_err(|e| LakehouseError::Io(std::io::Error::other(e.to_string())))
        .and_then(|r| r);
    match result {
        Ok(paths) => {
            stats.written += batch.len() as u64;
            stats.files += paths.len() as u64;
            stats.flushes += 1;
            Ok(())
        }
        Err(e) => {
            stats.failed_flushes += 1;
            *buf = Arc::try_unwrap(batch).unwrap_or_else(|a| (*a).clone());
            Err(e)
        }
    }
}

fn cap(buf: &mut Vec<Tick>, max: usize, stats: &mut RecorderStats) {
    if buf.len() > max {
        let excess = buf.len() - max;
        buf.drain(..excess);
        stats.dropped += excess as u64;
    }
}

async fn run(
    lakehouse: Lakehouse,
    config: RecorderConfig,
    mut rx: mpsc::Receiver<Tick>,
) -> Result<RecorderStats, LakehouseError> {
    let mut stats = RecorderStats::default();
    let mut buf: Vec<Tick> = Vec::with_capacity(config.max_ticks.min(1 << 16));
    let mut timer = tokio::time::interval(config.flush_interval);
    timer.tick().await; // the first tick completes immediately
    let mut next_flush_at = config.max_ticks;

    loop {
        tokio::select! {
            maybe = rx.recv() => match maybe {
                Some(tick) => {
                    stats.received += 1;
                    buf.push(tick);
                    if buf.len() >= next_flush_at {
                        if flush(&lakehouse, &mut buf, &mut stats).await.is_ok() {
                            next_flush_at = config.max_ticks;
                        } else {
                            cap(&mut buf, config.max_buffered, &mut stats);
                            // back off: retry on the timer or after another full batch
                            next_flush_at = buf.len() + config.max_ticks;
                        }
                    }
                }
                None => break,
            },
            _ = timer.tick() => {
                if flush(&lakehouse, &mut buf, &mut stats).await.is_ok() {
                    next_flush_at = config.max_ticks;
                } else {
                    cap(&mut buf, config.max_buffered, &mut stats);
                    next_flush_at = buf.len() + config.max_ticks;
                }
            }
        }
    }
    flush(&lakehouse, &mut buf, &mut stats).await?;
    Ok(stats)
}
