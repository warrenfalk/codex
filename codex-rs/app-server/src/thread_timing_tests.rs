use super::*;
use app_test_support::create_fake_rollout;
use codex_thread_store::LocalThreadStore;
use codex_thread_store::LocalThreadStoreConfig;
use codex_utils_absolute_path::test_support::PathExt;
use tempfile::TempDir;
use tokio::sync::mpsc;

#[tokio::test]
async fn subscriptions_are_owned_bounded_and_cleanup_survives_backpressure() {
    let home = TempDir::new().unwrap();
    let thread = create_fake_rollout(
        home.path(),
        "2025-01-03T17-30-00",
        "2025-01-03T17:30:00Z",
        "preview",
        Some("test"),
        None,
    )
    .unwrap();
    let store = Arc::new(LocalThreadStore::new(
        LocalThreadStoreConfig {
            codex_home: home.path().to_path_buf(),
            sqlite: codex_state::SqliteConfig::new_for_testing(home.path().abs()),
            default_model_provider_id: "test".to_string(),
        },
        None,
    ));
    // The first update fills this queue; another producer can be cancelled while blocked.
    let (tx, _rx) = mpsc::channel(/*buffer*/ 1);
    let service = ThreadTimingService::new(
        store,
        Arc::new(OutgoingMessageSender::new(
            tx,
            codex_analytics::AnalyticsEventsClient::disabled(),
        )),
    );
    let params = |id: &str| ThreadTimingSubscribeParams {
        subscription_id: id.to_string(),
        thread_id: thread.clone(),
        turn_id: None,
        cursor: None,
    };
    service
        .subscribe(ConnectionId(1), params("same"))
        .await
        .unwrap();
    service
        .subscribe(ConnectionId(2), params("same"))
        .await
        .unwrap();
    assert!(
        service
            .subscribe(ConnectionId(1), params("same"))
            .await
            .is_err()
    );
    assert_eq!(service.subscriptions.lock().await.len(), 2);
    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 1),
        service.unsubscribe(ConnectionId(1), "same".to_string()),
    )
    .await
    .unwrap();
    assert!(
        service
            .subscriptions
            .lock()
            .await
            .contains_key(&(ConnectionId(2), "same".to_string()))
    );
    for index in 1..MAX_SUBSCRIPTIONS {
        service
            .subscribe(ConnectionId(2), params(&format!("watch-{index}")))
            .await
            .unwrap();
    }
    assert!(
        service
            .subscribe(ConnectionId(2), params("overflow"))
            .await
            .is_err()
    );
    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 1),
        service.connection_closed(ConnectionId(2)),
    )
    .await
    .unwrap();
    assert!(service.subscriptions.lock().await.is_empty());
    service
        .unsubscribe(ConnectionId(2), "same".to_string())
        .await;
}
