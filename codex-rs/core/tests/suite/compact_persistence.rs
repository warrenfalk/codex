//! Compaction must survive a reload and must report checkpoint persistence failures.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_core::TurnInputRequest;
use codex_login::CodexAuth;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ImageReference;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutRecorder;
use codex_thread_store::AppendThreadItemsParams;
use codex_thread_store::ArchiveThreadParams;
use codex_thread_store::CreateThreadParams;
use codex_thread_store::DeleteThreadParams;
use codex_thread_store::InMemoryThreadStore;
use codex_thread_store::ListThreadsParams;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::PersistContext;
use codex_thread_store::ReadThreadByRolloutPathParams;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ResumeThreadParams;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredThreadHistory;
use codex_thread_store::ThreadPage;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use codex_thread_store::ThreadStoreFuture;
use codex_thread_store::UpdateThreadMetadataParams;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use rand::RngCore;
use rand::SeedableRng;
use serde_json::json;
use test_case::test_case;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_compaction_checkpoint_survives_resume() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mut builder = test_codex().with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing());
    let initial = builder.build_with_auto_env(&server).await?;

    // Two ordinary high-detail screenshots can exceed the writer's former 16 MiB cap
    // without approaching the retained-image token budget. Noise avoids tiny PNG fixtures.
    let mut pixels = vec![0; 1600 * 1600 * 3];
    rand::rngs::StdRng::seed_from_u64(/*state*/ 42).fill_bytes(&mut pixels);
    let image = image::RgbImage::from_raw(/*width*/ 1600, /*height*/ 1600, pixels)
        .context("image dimensions")?;
    let mut encoded = std::io::Cursor::new(Vec::new());
    image.write_to(&mut encoded, image::ImageFormat::Png)?;
    let input_image = UserInput::Image {
        image: ImageReference::Inline {
            image_url: format!(
                "data:image/png;base64,{}",
                STANDARD.encode(encoded.into_inner())
            ),
        },
        detail: Some(ImageDetail::High),
    };
    let before = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_assistant_message("old-answer", "discarded before compaction"),
            responses::ev_completed("before"),
        ]),
    )
    .await;
    initial
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![
            UserInput::Text {
                text: "Keep these screenshots".to_string(),
                text_elements: Vec::new(),
            },
            input_image.clone(),
            input_image,
        ]))
        .await?;
    wait_for_event(&initial.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let retained = before
        .single_request()
        .inputs_of_type("message")
        .into_iter()
        .find(|item| {
            item["role"] == "user"
                && item["content"]
                    .as_array()
                    .is_some_and(|content| content.iter().any(|item| item["type"] == "input_image"))
        })
        .context("user screenshots")?["content"]
        .clone();
    assert!(serde_json::to_vec(&retained)?.len() > 16 * 1024 * 1024);

    responses::mount_sse_once(&server, responses::sse(vec![
        json!({"type":"response.output_item.done", "item": {"type":"compaction", "encrypted_content":"saved-summary"}}),
        responses::ev_completed("compact"),
    ])).await;
    initial.codex.submit(Op::Compact).await?;
    wait_for_event(&initial.codex, |event| {
        assert!(
            !matches!(event, EventMsg::Error(_)),
            "compaction failed: {event:?}"
        );
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let rollout_path = initial.codex.rollout_path().context("rollout path")?;
    // Success is a persistence barrier, so this must work before shutdown flushes anything.
    let (items, _, parse_errors) = RolloutRecorder::load_rollout_items(&rollout_path).await?;
    assert_eq!(parse_errors, 0);
    assert!(
        items
            .iter()
            .any(|item| matches!(item, RolloutItem::Compacted(_)))
    );
    initial.codex.shutdown_and_wait().await?;

    let resumed = builder
        .resume(&server, initial.home.clone(), rollout_path)
        .await?;
    let after = responses::mount_sse_once(
        &server,
        responses::sse(vec![responses::ev_completed("after")]),
    )
    .await;
    resumed.submit_turn("Say hello").await?;
    let request = after.single_request();
    assert_eq!(
        request.inputs_of_type("compaction")[0]["encrypted_content"],
        "saved-summary"
    );
    assert!(!request.body_contains_text("discarded before compaction"));
    let restored = request
        .inputs_of_type("message")
        .into_iter()
        .find(|item| item["content"] == retained)
        .context("retained screenshots after resume")?;
    assert_eq!(restored["content"], retained);
    resumed.codex.shutdown_and_wait().await?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    Append,
    Flush,
}

struct FailingCheckpointStore {
    inner: InMemoryThreadStore,
    failure_point: FailurePoint,
    fail: AtomicBool,
    checkpoint_accepted: AtomicBool,
}

macro_rules! delegate_store_methods {
    ($(fn $name:ident($param:ident: $params:ty) -> $result:ty;)*) => {
        $(fn $name(&self, $param: $params) -> ThreadStoreFuture<'_, $result> {
            ThreadStore::$name(&self.inner, $param)
        })*
    };
}

impl ThreadStore for FailingCheckpointStore {
    fn as_any(&self) -> &dyn Any {
        self
    }

    delegate_store_methods! {
        fn create_thread(params: CreateThreadParams) -> ();
        fn resume_thread(params: ResumeThreadParams) -> ();
        fn discard_thread(thread_id: ThreadId) -> ();
        fn load_history(params: LoadThreadHistoryParams) -> StoredThreadHistory;
        fn read_thread(params: ReadThreadParams) -> StoredThread;
        fn read_thread_by_rollout_path(params: ReadThreadByRolloutPathParams) -> StoredThread;
        fn list_threads(params: ListThreadsParams) -> ThreadPage;
        fn archive_thread(params: ArchiveThreadParams) -> ();
        fn unarchive_thread(params: ArchiveThreadParams) -> StoredThread;
        fn delete_thread(params: DeleteThreadParams) -> ();
        fn update_thread_metadata(params: UpdateThreadMetadataParams) -> Option<StoredThread>;
        fn shutdown_thread(thread_id: ThreadId) -> ();
    }

    fn persist_thread(
        &self,
        thread_id: ThreadId,
        context: PersistContext,
    ) -> ThreadStoreFuture<'_, ()> {
        self.inner.persist_thread(thread_id, context)
    }

    fn append_items(&self, params: AppendThreadItemsParams) -> ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            if params
                .items
                .iter()
                .any(|item| matches!(item, RolloutItem::Compacted(_)))
            {
                if self.failure_point == FailurePoint::Append && self.fail.load(Ordering::SeqCst) {
                    return Err(ThreadStoreError::Internal {
                        message: "checkpoint rejected".to_string(),
                    });
                }
                self.checkpoint_accepted
                    .store(/*val*/ true, Ordering::SeqCst);
            }
            self.inner.append_items(params).await
        })
    }

    fn flush_thread(&self, thread_id: ThreadId) -> ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            if self.failure_point == FailurePoint::Flush
                && self.checkpoint_accepted.load(Ordering::SeqCst)
                && self.fail.load(Ordering::SeqCst)
            {
                return Err(ThreadStoreError::Internal {
                    message: "checkpoint flush failed".to_string(),
                });
            }
            self.inner.flush_thread(thread_id).await
        })
    }
}

#[test_case(FailurePoint::Append; "append")]
#[test_case(FailurePoint::Flush; "flush")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_persistence_failure_is_reported(failure_point: FailurePoint) -> Result<()> {
    let server = responses::start_mock_server().await;
    let store = Arc::new(FailingCheckpointStore {
        inner: InMemoryThreadStore::default(),
        failure_point,
        fail: AtomicBool::new(/*v*/ true),
        checkpoint_accepted: AtomicBool::new(/*v*/ false),
    });
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .with_thread_store(store.clone())
        .build_with_auto_env(&server)
        .await?;
    responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_assistant_message("original", "original history"),
            responses::ev_completed("initial"),
        ]),
    )
    .await;
    test.submit_turn("remember this").await?;
    responses::mount_sse_once(&server, responses::sse(vec![
        json!({"type":"response.output_item.done", "item": {"type":"compaction", "encrypted_content":"new-summary"}}),
        responses::ev_completed("compact"),
    ])).await;
    test.codex.submit(Op::Compact).await?;
    let mut errors = Vec::new();
    loop {
        match test.codex.next_event().await?.msg {
            EventMsg::Error(error) => errors.push(error.message),
            EventMsg::ItemCompleted(event) => assert!(
                !matches!(event.item, TurnItem::ContextCompaction(_)),
                "unsaved compaction reported success"
            ),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }
    insta::assert_snapshot!(
        match failure_point {
            FailurePoint::Append => "compaction_append_error",
            FailurePoint::Flush => "compaction_flush_error",
        },
        errors.join("\n")
    );
    assert_eq!(errors.len(), 1);
    store.fail.store(/*val*/ false, Ordering::SeqCst);
    let after = responses::mount_sse_once(
        &server,
        responses::sse(vec![responses::ev_completed("recovered")]),
    )
    .await;
    test.submit_turn("continue").await?;
    let request = after.single_request();
    // Rejected checkpoints must not replace live history. Accepted checkpoints remain queued
    // on a flush failure, and the live history must agree with what retry will persist.
    assert_eq!(
        request.body_contains_text("original history"),
        failure_point == FailurePoint::Append
    );
    assert_eq!(
        request.inputs_of_type("compaction").len(),
        usize::from(failure_point == FailurePoint::Flush)
    );
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[test_case(FailurePoint::Append; "append")]
#[test_case(FailurePoint::Flush; "flush")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_turn_compaction_save_failure_warns_without_losing_answer(
    failure_point: FailurePoint,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let store = Arc::new(FailingCheckpointStore {
        inner: InMemoryThreadStore::default(),
        failure_point,
        fail: AtomicBool::new(/*v*/ true),
        checkpoint_accepted: AtomicBool::new(/*v*/ false),
    });
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .with_thread_store(store.clone())
        .with_config(|config| config.model_post_turn_compact_threshold_percent = 1)
        .build_with_auto_env(&server)
        .await?;
    let mock = responses::mount_sse_sequence(&server, vec![
        responses::sse(vec![
            responses::ev_assistant_message("answer", "Completed answer"),
            responses::ev_completed_with_tokens("answer", /*total_tokens*/ 20_000),
        ]),
        responses::sse(vec![
            json!({"type":"response.output_item.done", "item": {"type":"compaction", "encrypted_content":"new-summary"}}),
            responses::ev_completed("compact"),
        ]),
    ]).await;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Finish this work".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut warnings = Vec::new();
    let completed = loop {
        match test.codex.next_event().await?.msg {
            EventMsg::Warning(warning) => warnings.push(warning.message),
            EventMsg::Error(error) => panic!("completed answer failed: {error:?}"),
            EventMsg::ItemCompleted(event) => assert!(
                !matches!(event.item, TurnItem::ContextCompaction(_)),
                "unsaved compaction reported success"
            ),
            EventMsg::TurnComplete(completed) => break completed,
            _ => {}
        }
    };
    assert_eq!(
        (completed.last_agent_message.as_deref(), completed.error),
        (Some("Completed answer"), None)
    );
    assert_eq!(mock.requests().len(), 2);
    insta::assert_snapshot!(
        match failure_point {
            FailurePoint::Append => "post_turn_compaction_append_warning",
            FailurePoint::Flush => "post_turn_compaction_flush_warning",
        },
        warnings.join("\n")
    );
    store.fail.store(/*val*/ false, Ordering::SeqCst);
    test.codex.shutdown_and_wait().await?;
    Ok(())
}
