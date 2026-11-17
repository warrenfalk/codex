use super::*;
use crate::session::tests::make_session_and_context_with_rx;
use codex_http_client::RetryAfter;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn capacity_backoff_caps_and_warns_once_without_ending_the_turn() {
    let (session, turn, events) = make_session_and_context_with_rx().await;
    tokio::time::pause();
    let mut retry = CapacityRetryState::default();
    let mut delays = Vec::new();
    let mut warning_times = Vec::new();
    let started_at = Instant::now();
    for _ in 0..10 {
        let before = Instant::now();
        let wait = retry.wait(&session, &turn, &CodexErr::ServerOverloaded);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                biased;
                event = events.recv() => {
                    match event.unwrap().msg {
                        EventMsg::StreamError(event) => assert_eq!(event.codex_error_info, Some(CodexErrorInfo::ServerOverloaded)),
                        EventMsg::ModelCapacityWarning(_) => warning_times.push(Instant::now().duration_since(started_at).as_secs()),
                        event => panic!("unexpected event: {event:?}"),
                    }
                }
                () = &mut wait => break,
            }
        }
        delays.push(Instant::now().duration_since(before).as_secs());
    }
    assert_eq!(delays, vec![5, 10, 20, 40, 60, 60, 60, 60, 60, 60]);
    assert_eq!(warning_times, vec![300]);
}

#[tokio::test]
async fn capacity_warning_fires_during_long_server_advised_wait() {
    let (session, turn, events) = make_session_and_context_with_rx().await;
    tokio::time::pause();
    let started_at = Instant::now();
    let error = CodexErr::ServerOverloaded
        .with_retry_after(RetryAfter::from_delay(Duration::from_secs(600)).unwrap());
    let mut retry = CapacityRetryState::default();
    let wait = retry.wait(&session, &turn, &error);
    tokio::pin!(wait);
    let mut warning_times = Vec::new();
    loop {
        tokio::select! {
            biased;
            event = events.recv() => match event.unwrap().msg {
                EventMsg::StreamError(_) => {},
                EventMsg::ModelCapacityWarning(_) => warning_times.push(Instant::now().duration_since(started_at).as_secs()),
                event => panic!("unexpected event: {event:?}"),
            },
            () = &mut wait => break,
        }
    }
    assert_eq!(warning_times, vec![300]);
    assert_eq!(Instant::now().duration_since(started_at).as_secs(), 600);
}
