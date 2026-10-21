use honba_engine::DataFeed;
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Event, Exchange, InstrumentId, PriceType,
    UnixNanos,
};

use crate::dataset::{
    ColumnarSlice, ColumnarSliceBuilder, Dataset, DatasetBuildError, DatasetFeed,
};

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol, Exchange::new("NSE"))
}

fn spec() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
}

fn slice_with_closes(symbol: &str, starts: &[u64], closes: &[f64]) -> ColumnarSlice {
    let mut builder = ColumnarSliceBuilder::new(instrument(symbol), spec());
    for (index, close) in closes.iter().enumerate() {
        builder
            .push(
                UnixNanos::from_u64(starts[index]),
                *close,
                *close + 1.0,
                *close - 1.0,
                *close,
                1.0,
            )
            .expect("the row is valid");
    }
    builder.finish().expect("the builder is valid")
}

#[test]
fn an_empty_builder_produces_an_empty_slice() {
    let slice = ColumnarSliceBuilder::new(instrument("TCS"), spec())
        .finish()
        .expect("an empty builder is valid");

    assert_eq!(slice.len(), 0);
    assert!(slice.is_empty());
    assert!(slice.timestamps().is_empty());
    assert!(slice.closes().is_empty());
    assert_eq!(slice.bar(0), None);
    assert_eq!(slice.instrument(), &instrument("TCS"));
    assert_eq!(*slice.bar_spec(), spec());
}

#[test]
fn the_builder_is_reachable_from_the_slice() {
    let mut builder = ColumnarSlice::builder(instrument("TCS"), spec());
    builder
        .push(UnixNanos::from_u64(10), 1.0, 1.0, 1.0, 1.0, 1.0)
        .expect("the row is valid");
    assert_eq!(builder.len(), 1);
    assert!(!builder.is_empty());

    let slice = builder.finish().expect("the builder is valid");
    assert_eq!(slice.len(), 1);
}

#[test]
fn one_bar_fills_every_column_and_round_trips() {
    let mut builder = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    builder
        .push(UnixNanos::from_u64(100), 10.0, 12.0, 9.0, 11.0, 500.0)
        .expect("the row is valid");
    let slice = builder.finish().expect("the builder is valid");

    assert_eq!(slice.len(), 1);
    assert!(!slice.is_empty());
    assert_eq!(slice.timestamps(), &[UnixNanos::from_u64(100)]);
    assert_eq!(slice.opens(), &[10.0]);
    assert_eq!(slice.highs(), &[12.0]);
    assert_eq!(slice.lows(), &[9.0]);
    assert_eq!(slice.closes(), &[11.0]);
    assert_eq!(slice.volumes(), &[500.0]);

    let expected = Bar::new(
        BarType::new(instrument("TCS"), spec()),
        10.0,
        12.0,
        9.0,
        11.0,
        500.0,
        UnixNanos::from_u64(100),
        UnixNanos::from_u64(100),
    );
    assert_eq!(slice.bar(0), Some(expected));
    assert_eq!(slice.bar(1), None);
}

#[test]
fn a_pushed_bar_keeps_its_event_timestamp() {
    let bar = Bar::new(
        BarType::new(instrument("TCS"), spec()),
        10.0,
        12.0,
        9.0,
        11.0,
        500.0,
        UnixNanos::from_u64(100),
        UnixNanos::from_u64(7),
    );
    let mut builder = ColumnarSliceBuilder::new(
        bar.bar_type().instrument_id().clone(),
        bar.bar_type().spec(),
    );
    builder.push_bar(&bar).expect("the bar is finite");

    let slice = builder.finish().expect("the builder is valid");
    let rebuilt = Bar::new(
        BarType::new(instrument("TCS"), spec()),
        10.0,
        12.0,
        9.0,
        11.0,
        500.0,
        UnixNanos::from_u64(100),
        UnixNanos::from_u64(100),
    );
    assert_eq!(slice.bar(0), Some(rebuilt));
}

#[test]
fn a_timestamp_regression_is_rejected() {
    let mut builder = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    builder
        .push(UnixNanos::from_u64(10), 1.0, 1.0, 1.0, 1.0, 1.0)
        .expect("the first row is valid");

    let err = builder
        .push(UnixNanos::from_u64(9), 1.0, 1.0, 1.0, 1.0, 1.0)
        .expect_err("a regression is rejected");

    assert_eq!(
        err,
        DatasetBuildError::NonMonotonicTimestamp {
            index: 1,
            previous: 10,
            got: 9,
        }
    );
}

#[test]
fn rows_of_different_lengths_are_rejected() {
    let mut builder = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    let err = builder
        .push_rows(
            &[UnixNanos::from_u64(10), UnixNanos::from_u64(20)],
            &[1.0],
            &[1.0, 1.0],
            &[1.0, 1.0],
            &[1.0, 1.0],
            &[1.0, 1.0],
        )
        .expect_err("a short column is rejected");

    assert_eq!(
        err,
        DatasetBuildError::LengthMismatch {
            expected: 2,
            got: 1
        }
    );
    assert_eq!(builder.len(), 0);
}

#[test]
fn a_whole_block_of_rows_is_appended() {
    let mut builder = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    builder
        .push_rows(
            &[UnixNanos::from_u64(10), UnixNanos::from_u64(20)],
            &[1.0, 2.0],
            &[1.5, 2.5],
            &[0.5, 1.5],
            &[1.25, 2.25],
            &[100.0, 200.0],
        )
        .expect("the block is valid");
    assert_eq!(builder.len(), 2);

    let slice = builder.finish().expect("the builder is valid");
    assert_eq!(slice.opens(), &[1.0, 2.0]);
    assert_eq!(slice.volumes(), &[100.0, 200.0]);
}

#[test]
fn a_non_finite_value_is_rejected() {
    let mut nan_close = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    let err = nan_close
        .push(UnixNanos::from_u64(10), 1.0, 1.0, 1.0, f64::NAN, 1.0)
        .expect_err("NaN is rejected");
    assert_eq!(
        err,
        DatasetBuildError::NonFiniteValue {
            index: 0,
            column: "close"
        }
    );

    let mut infinite_volume = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    let err = infinite_volume
        .push(UnixNanos::from_u64(10), 1.0, 1.0, 1.0, 1.0, f64::INFINITY)
        .expect_err("infinity is rejected");
    assert_eq!(
        err,
        DatasetBuildError::NonFiniteValue {
            index: 0,
            column: "volume"
        }
    );
}

#[test]
fn a_rejected_row_leaves_the_builder_unchanged() {
    let mut builder = ColumnarSliceBuilder::new(instrument("TCS"), spec());
    builder
        .push(UnixNanos::from_u64(10), 1.0, 1.0, 1.0, 1.0, 1.0)
        .expect("the first row is valid");

    assert!(
        builder
            .push(UnixNanos::from_u64(9), 1.0, 1.0, 1.0, 1.0, 1.0)
            .is_err()
    );
    assert!(
        builder
            .push(UnixNanos::from_u64(11), 1.0, 1.0, 1.0, f64::NAN, 1.0)
            .is_err()
    );
    assert_eq!(builder.len(), 1);

    let slice = builder.finish().expect("the builder is valid");
    assert_eq!(slice.timestamps(), &[UnixNanos::from_u64(10)]);
}

#[test]
fn cloning_a_slice_copies_the_pointer() {
    let slice = slice_with_closes("TCS", &[10], &[5.0]);
    let clone = slice.clone();

    assert!(ColumnarSlice::ptr_eq(&slice, &clone));
    assert!(ColumnarSlice::ptr_eq(&slice, &slice.clone()));

    let rebuilt = slice_with_closes("TCS", &[10], &[5.0]);
    assert_eq!(slice, rebuilt);
    assert!(!ColumnarSlice::ptr_eq(&slice, &rebuilt));
}

#[test]
fn a_columnar_slice_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ColumnarSlice>();
    assert_send_sync::<ColumnarSliceBuilder>();
}

#[test]
fn a_dataset_is_send_sync_static() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<Dataset>();
    assert_send_sync_static::<DatasetFeed>();
}

#[test]
fn from_slices_sorts_by_instrument_id() {
    let tcs = slice_with_closes("TCS", &[10], &[5.0]);
    let infy = slice_with_closes("INFY", &[20], &[6.0]);
    let reliance = slice_with_closes("RELIANCE", &[30], &[7.0]);

    let dataset = Dataset::from_slices(vec![tcs, reliance, infy]).expect("the instruments differ");

    let order: Vec<&str> = dataset.instruments().map(|id| id.symbol()).collect();
    assert_eq!(order, vec!["INFY", "RELIANCE", "TCS"]);
    assert_eq!(dataset.len(), 3);
    assert_eq!(dataset.bar_count(), 3);
}

#[test]
fn a_duplicate_instrument_is_rejected() {
    let first = slice_with_closes("TCS", &[10], &[5.0]);
    let second = slice_with_closes("TCS", &[20], &[6.0]);

    let err =
        Dataset::from_slices(vec![first, second]).expect_err("a repeated instrument is rejected");

    assert_eq!(
        err,
        DatasetBuildError::DuplicateInstrument(instrument("TCS"))
    );
}

#[test]
fn slice_lookup_hits_and_misses() {
    let infy = slice_with_closes("INFY", &[20], &[6.0]);
    let tcs = slice_with_closes("TCS", &[10], &[5.0]);
    let dataset =
        Dataset::from_slices(vec![tcs.clone(), infy.clone()]).expect("distinct instruments");

    assert_eq!(dataset.slice(&instrument("TCS")), Some(&tcs));
    assert_eq!(dataset.slice(&instrument("INFY")), Some(&infy));
    assert!(dataset.slice(&instrument("SBIN")).is_none());
    assert_eq!(dataset.slices(), &[infy, tcs]);
}

#[test]
fn bar_count_sums_the_slices() {
    let infy = slice_with_closes("INFY", &[10, 20, 30], &[6.0, 7.0, 8.0]);
    let tcs = slice_with_closes("TCS", &[10, 20], &[5.0, 6.0]);

    let dataset = Dataset::from_slices(vec![tcs, infy]).expect("distinct instruments");

    assert_eq!(dataset.len(), 2);
    assert_eq!(dataset.bar_count(), 5);
}

#[test]
fn an_empty_dataset_has_nothing_in_it() {
    let dataset = Dataset::new();

    assert!(dataset.is_empty());
    assert_eq!(dataset.len(), 0);
    assert_eq!(dataset.bar_count(), 0);
    assert_eq!(dataset.slices(), &[] as &[ColumnarSlice]);
    assert!(dataset.instruments().next().is_none());
    assert!(dataset.slice(&instrument("TCS")).is_none());
}

#[test]
fn cloning_a_dataset_copies_the_pointer() {
    let slice = slice_with_closes("TCS", &[10], &[5.0]);
    let dataset = Dataset::from_slices(vec![slice]).expect("one instrument");
    let clone = dataset.clone();

    assert!(Dataset::ptr_eq(&dataset, &clone));

    let other_slice = slice_with_closes("TCS", &[10], &[5.0]);
    let rebuilt = Dataset::from_slices(vec![other_slice]).expect("one instrument");
    assert!(!Dataset::ptr_eq(&dataset, &rebuilt));
}

#[test]
fn slice_messages_carry_every_bar() {
    let slice = slice_with_closes("TCS", &[10, 20], &[5.0, 6.0]);

    let messages = slice.messages(UnixNanos::from_u64(7));

    assert_eq!(messages.len(), 2);
    let closes: Vec<f64> = messages
        .iter()
        .map(|message| match message.event() {
            Event::Bar(bar) => bar.close(),
            other => panic!("expected a bar, got {other:?}"),
        })
        .collect();
    assert_eq!(closes, vec![5.0, 6.0]);
    assert!(
        messages
            .iter()
            .all(|message| message.ts_init() == UnixNanos::from_u64(7))
    );

    let empty = slice_with_closes("TCS", &[], &[]);
    assert!(empty.messages(UnixNanos::from_u64(7)).is_empty());
}

#[test]
fn a_feed_replays_every_bar_once_in_instrument_order() {
    let tcs = slice_with_closes("TCS", &[10, 20], &[5.0, 6.0]);
    let infy = slice_with_closes("INFY", &[30, 40, 50], &[7.0, 8.0, 9.0]);
    let dataset = Dataset::from_slices(vec![tcs, infy]).expect("distinct instruments");

    let mut feed = DatasetFeed::new(&dataset, UnixNanos::from_u64(1));
    let mut seen: Vec<(String, u64, f64)> = Vec::new();
    while let Some(message) = feed.next().expect("the feed does not fail") {
        let Event::Bar(bar) = message.into_event() else {
            panic!("the feed replays bars");
        };
        seen.push((
            bar.bar_type().instrument_id().symbol().to_string(),
            bar.ts_event().as_u64(),
            bar.close(),
        ));
    }

    assert_eq!(
        seen,
        vec![
            ("INFY".to_string(), 30, 7.0),
            ("INFY".to_string(), 40, 8.0),
            ("INFY".to_string(), 50, 9.0),
            ("TCS".to_string(), 10, 5.0),
            ("TCS".to_string(), 20, 6.0),
        ]
    );
    assert_eq!(dataset.bar_count(), seen.len());
}

#[test]
fn every_message_carries_the_runs_init_timestamp() {
    let dataset = Dataset::from_slices(vec![slice_with_closes("TCS", &[10, 20], &[5.0, 6.0])])
        .expect("valid");
    let mut feed = DatasetFeed::new(&dataset, UnixNanos::from_u64(999));

    let mut count = 0;
    while let Some(message) = feed.next().expect("the feed does not fail") {
        assert_eq!(message.ts_init(), UnixNanos::from_u64(999));
        assert!(matches!(message.event(), Event::Bar(_)));
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn an_exhausted_feed_keeps_returning_none() {
    let dataset =
        Dataset::from_slices(vec![slice_with_closes("TCS", &[10], &[5.0])]).expect("valid");
    let mut feed = DatasetFeed::new(&dataset, UnixNanos::from_u64(1));

    assert!(feed.next().expect("the first bar").is_some());
    for _ in 0..3 {
        assert!(
            feed.next()
                .expect("an exhausted feed does not fail")
                .is_none()
        );
    }
}

#[test]
fn a_feed_over_an_empty_dataset_yields_nothing() {
    let mut feed = DatasetFeed::new(&Dataset::new(), UnixNanos::from_u64(1));

    assert!(feed.next().expect("an empty feed does not fail").is_none());
}

#[test]
fn chunks_cover_slice() {
    let timestamps: Vec<u64> = (1..=25).map(|i| i * 10).collect();
    let closes: Vec<f64> = (1..=25).map(|i| i as f64 * 1.5).collect();
    let original = slice_with_closes("TCS", &timestamps, &closes);
    assert_eq!(original.len(), 25);

    let chunks = original.chunks(7);
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].len(), 7);
    assert_eq!(chunks[1].len(), 7);
    assert_eq!(chunks[2].len(), 7);
    assert_eq!(chunks[3].len(), 4);

    let total_reconstructed: usize = chunks.iter().map(|c| c.len()).sum();
    assert_eq!(total_reconstructed, original.len());

    let mut cursor = 0;
    for chunk in &chunks {
        for i in 0..chunk.len() {
            let chunk_bar = chunk.bar(i).expect("bar exists in chunk");
            let orig_bar = original.bar(cursor).expect("bar exists in orig");
            assert_eq!(chunk_bar, orig_bar);
            cursor += 1;
        }
    }
    assert_eq!(cursor, original.len());
}

#[test]
fn streamed_dataset_feed_replays_all_chunks_deterministically() {
    let timestamps: Vec<u64> = (1..=20).map(|i| i * 10).collect();
    let closes_tcs: Vec<f64> = (1..=20).map(|i| 100.0 + i as f64).collect();
    let closes_infy: Vec<f64> = (1..=20).map(|i| 200.0 + i as f64).collect();

    let ds = Dataset::from_slices(vec![
        slice_with_closes("TCS", &timestamps, &closes_tcs),
        slice_with_closes("INFY", &timestamps, &closes_infy),
    ])
    .expect("valid")
    .with_id(honba_messages::DatasetId::new("NSE:NIFTY50:1m:test_hash"));

    assert_eq!(
        ds.id().map(|id| id.as_str()),
        Some("NSE:NIFTY50:1m:test_hash")
    );

    let mut standard_feed = DatasetFeed::new(&ds, UnixNanos::from_u64(42));
    let mut streamed_feed =
        crate::dataset::StreamedDatasetFeed::new(&ds, 6, UnixNanos::from_u64(42));

    let mut standard_msgs = Vec::new();
    while let Some(msg) = standard_feed.next().expect("valid") {
        standard_msgs.push(msg);
    }

    let mut streamed_msgs = Vec::new();
    while let Some(msg) = streamed_feed.next().expect("valid") {
        streamed_msgs.push(msg);
    }

    assert_eq!(streamed_msgs.len(), 40);
    assert_eq!(streamed_msgs.len(), standard_msgs.len());
}
