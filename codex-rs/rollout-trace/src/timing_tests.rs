use super::stream::MAX_OPEN_ITEMS;
use super::*;
use crate::InferenceTraceContext;
use codex_history::timing::ItemBoundary;
use pretty_assertions::assert_eq;

fn context() -> TimingContext {
    TimingContext::new(ThreadId::new(), "turn".to_string(), "model".to_string())
}

#[test]
fn live_snapshots_include_pending_waits_and_retire_finished_spans() {
    let timing = context();
    let id = timing.shared.thread_id;
    let request = timing
        .for_attempt(/*attempt*/ 1)
        .span(TimingPhase::ModelRequest);
    let mut capture = StreamTimingCapture::new(timing.under(&request));
    let initial = timing.drain();
    let waiting = capture.waiting();
    let snapshot = active_timing(id, Some("turn"));
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].record, initial[0]);
    assert!(snapshot[0].stream.as_ref().unwrap().receive_wait_us > 0);
    assert!(timing.drain().is_empty());
    assert!(active_timing(id, Some("other")).is_empty());
    drop(waiting);
    let processing = capture.processing();
    capture.observe(StreamObservation::Delta { id: None, bytes: 7 });
    let snapshot = active_timing(id, None);
    let stream = snapshot[0].stream.as_ref().unwrap();
    assert_eq!((stream.delta_count, stream.delta_bytes), (1, 7));
    assert!(stream.event_processing_us > 0);
    drop(processing);
    capture.finish(&request, TimingStatus::Completed);
    assert!(active_timing(id, None).is_empty());
    assert_eq!(timing.drain().len(), 1);
    let cancelled = timing.span(TimingPhase::StreamSetup);
    assert_eq!(active_timing(id, None).len(), 1);
    drop(cancelled);
    assert!(active_timing(id, None).is_empty());
}

#[test]
fn streamed_deltas_are_aggregated_without_per_delta_records() {
    let timing = context();
    let request = timing
        .for_attempt(/*attempt*/ 1)
        .span(TimingPhase::ModelRequest);
    let mut capture = StreamTimingCapture::new(timing.for_attempt(/*attempt*/ 1).under(&request));
    capture.observe(StreamObservation::Created("response"));
    capture.observe(StreamObservation::Added {
        id: "tool",
        kind: "custom_tool_call",
    });
    let records_before = timing.drain();
    assert_eq!(records_before.len(), 3);
    for _ in 0..1000 {
        assert!(!capture.observe(StreamObservation::Delta {
            id: Some("tool"),
            bytes: 7
        }));
    }
    assert!(timing.drain().is_empty());
    capture.record_wait(Duration::from_micros(/*micros*/ 1234));
    capture.observe(StreamObservation::Done {
        id: "tool",
        kind: "custom_tool_call",
    });
    capture.observe(StreamObservation::Completed("response"));
    capture.finish(&request, TimingStatus::Completed);
    let records = timing.drain();
    assert_eq!(records.len(), 2);
    let TimingEvent::Item {
        stream: Some(item),
        boundary,
        ..
    } = &records[0].event
    else {
        panic!("missing item summary")
    };
    assert_eq!(
        (*boundary, item.delta_count, item.delta_bytes),
        (ItemBoundary::Done, 1000, 7000)
    );
    let TimingEvent::Completed {
        status,
        response_id,
        stream: Some(summary),
        ..
    } = &records[1].event
    else {
        panic!("missing request summary")
    };
    assert_eq!(
        (
            *status,
            response_id.as_deref(),
            summary.event_count,
            summary.delta_count,
            summary.receive_wait_us
        ),
        (TimingStatus::Completed, Some("response"), 1004, 1000, 1234)
    );
    assert_eq!(records_before[2].span_id, records[0].span_id);
    assert_eq!(
        records[0].parent_span_id.as_deref(),
        Some(request.span_id.as_str())
    );
}

#[test]
fn dropped_spans_close_as_cancelled_and_terminal_events_are_idempotent() {
    let timing = context();
    let span = timing.span(TimingPhase::StreamSetup);
    drop(span);
    let finished = timing.span(TimingPhase::ModelRequest);
    finished.finish(
        TimingStatus::Failed,
        /*response_id*/ None,
        /*stream*/ None,
    );
    finished.finish(
        TimingStatus::Completed,
        /*response_id*/ None,
        /*stream*/ None,
    );
    drop(finished);
    let statuses: Vec<_> = timing
        .drain()
        .into_iter()
        .filter_map(|record| match record.event {
            TimingEvent::Completed { status, .. } => Some(status),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        vec![TimingStatus::Cancelled, TimingStatus::Failed]
    );
}

#[test]
fn transport_attempts_are_observed_without_enabling_payload_tracing() {
    let timing = context();
    let trace = InferenceTraceContext::disabled().with_timing(timing.for_attempt(/*attempt*/ 2));
    let failed = trace.start_attempt();
    assert!(!failed.needs_output_items());
    failed.record_failed(
        "private failure text",
        /*upstream_request_id*/ None,
        &[],
    );
    let completed = trace.start_attempt();
    completed.record_completed(
        "response",
        /*upstream_request_id*/ None,
        /*token_usage*/ &None,
        &[],
    );
    drop(failed);
    drop(completed);
    let records = timing.drain();
    assert_eq!(records.len(), 4);
    assert!(
        records
            .iter()
            .all(|record| record.attempt == 2 && record.phase == TimingPhase::TransportAttempt)
    );
    assert_eq!(records[0].span_id, records[1].span_id);
    assert_eq!(records[2].span_id, records[3].span_id);
    assert_ne!(records[0].span_id, records[2].span_id);
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("private failure text")
    );
}

#[test]
fn completion_without_an_added_event_does_not_invent_a_start() {
    let timing = context();
    let mut capture = StreamTimingCapture::new(timing.clone());
    capture.observe(StreamObservation::Done {
        id: "reasoning",
        kind: "reasoning",
    });
    let records = timing.drain();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        &records[0].event,
        TimingEvent::Item {
            boundary: ItemBoundary::Done,
            stream: None,
            ..
        }
    ));
}

#[test]
fn consumer_cancellation_closes_the_transport_before_late_producer_cleanup() {
    let timing = context();
    let trace = InferenceTraceContext::disabled().with_timing(timing.clone());
    let producer = trace.start_attempt();
    timing.cancel_open_transports();
    producer.record_cancelled("consumer stopped", /*upstream_request_id*/ None, &[]);
    drop(producer);
    let records = timing.drain();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].span_id, records[1].span_id);
    assert!(matches!(
        records[1].event,
        TimingEvent::Completed {
            status: TimingStatus::Cancelled,
            ..
        }
    ));
}

#[test]
fn concurrent_transport_completion_cannot_escape_the_consumers_final_drain() {
    for _ in 0..128 {
        let timing = context();
        let span = timing.span(TimingPhase::TransportAttempt);
        timing.drain();
        let barrier = std::sync::Barrier::new(/*n*/ 2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                span.finish(
                    TimingStatus::Completed,
                    /*response_id*/ None,
                    /*stream*/ None,
                );
            });
            barrier.wait();
            timing.cancel_open_transports();
            let records = timing.drain();
            assert_eq!(records.len(), 1);
            assert!(matches!(records[0].event, TimingEvent::Completed { .. }));
        });
        drop(span);
        assert!(timing.drain().is_empty());
    }
}

#[test]
fn unidentified_deltas_are_not_assigned_to_overlapping_items() {
    let timing = context();
    let request = timing.span(TimingPhase::ModelRequest);
    let mut capture = StreamTimingCapture::new(timing.under(&request));
    capture.observe(StreamObservation::Added {
        id: "first",
        kind: "message",
    });
    capture.observe(StreamObservation::Added {
        id: "second",
        kind: "message",
    });
    capture.observe(StreamObservation::Delta { id: None, bytes: 7 });
    capture.observe(StreamObservation::Delta {
        id: Some("second"),
        bytes: 4,
    });
    capture.observe(StreamObservation::Done {
        id: "first",
        kind: "message",
    });
    capture.observe(StreamObservation::Done {
        id: "second",
        kind: "message",
    });
    capture.finish(&request, TimingStatus::Completed);
    let records = timing.drain();
    let deltas: Vec<_> = records
        .iter()
        .filter_map(|record| match &record.event {
            TimingEvent::Item {
                stream: Some(metrics),
                ..
            } => Some(metrics.delta_count),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec![0, 1]);
    assert!(matches!(
        &records.last().unwrap().event,
        TimingEvent::Completed {
            stream: Some(StreamTiming { delta_count: 2, .. }),
            ..
        }
    ));
}

#[test]
fn excessive_unfinished_items_are_bounded_and_reported() {
    let timing = context();
    let request = timing.span(TimingPhase::ModelRequest);
    let mut capture = StreamTimingCapture::new(timing.under(&request));
    for index in 0..MAX_OPEN_ITEMS + 1 {
        capture.observe(StreamObservation::Added {
            id: &format!("item-{index}"),
            kind: "message",
        });
    }
    capture.finish(&request, TimingStatus::Completed);
    let records = timing.drain();
    assert_eq!(records.len(), MAX_OPEN_ITEMS + 2);
    assert!(matches!(
        &records.last().unwrap().event,
        TimingEvent::Completed {
            stream: Some(StreamTiming {
                omitted_item_count: 1,
                ..
            }),
            ..
        }
    ));
}
