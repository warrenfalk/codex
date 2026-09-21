use super::*;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[test]
fn narration_preserves_prose_and_link_labels_and_skips_code() {
    assert_eq!(
        spoken_text(
            "# Choose a **voice**\n\nUse `say` with [Charles](https://example.com).\n\n- First choice\n- Second choice\n\n```sh\nrm -rf example\n```\n\nAfter the example."
        ),
        "Choose a voice\nUse say with Charles.\nFirst choice\nSecond choice\nAfter the example."
    );
    assert_eq!(
        spoken_text("```rust\nfn main() {}\n```\n\n<!-- hidden -->"),
        ""
    );
    assert_eq!(
        spoken_text("¡Sí!\nUna **opción** &amp; otra."),
        "¡Sí! Una opción & otra."
    );
}

#[tokio::test]
async fn failures_disable_only_the_current_worker() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let events = AppEventSender::new(tx);
    let mut speech = Speech::default();
    speech.set_mode(TtsMode::Final);
    let config = TtsConfig {
        command: vec!["codex-test-missing-speech-executable".into()],
        ..Default::default()
    };
    speech
        .enqueue("turn", "item", "Hello".into(), &config, &events)
        .unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let AppEvent::TtsFailed {
        generation,
        message,
    } = failure
    else {
        panic!("expected speech failure");
    };
    assert!(message.contains("could not start"));
    speech.stop();
    assert!(!speech.accept_failure(generation));
    assert_eq!(speech.mode(), TtsMode::Final);

    speech
        .enqueue("turn", "next", "Hello again".into(), &config, &events)
        .unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let AppEvent::TtsFailed { generation, .. } = failure else {
        panic!("expected speech failure");
    };
    assert!(speech.accept_failure(generation));
    assert_eq!(speech.mode(), TtsMode::Off);
}

#[tokio::test]
async fn speech_queue_and_individual_messages_are_bounded() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let events = AppEventSender::new(tx);
    let config = TtsConfig::default();
    let mut speech = Speech::default();
    speech.set_mode(TtsMode::ProgressAndFinal);
    let error = speech
        .enqueue(
            "turn",
            "large",
            "x".repeat(MAX_MESSAGE_BYTES + 1),
            &config,
            &events,
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "message exceeds the 64 KiB speech limit");
    // No await: the worker cannot drain the bounded queue during this loop.
    for index in 0..MAX_QUEUED_MESSAGES {
        speech
            .enqueue("turn", &index.to_string(), "text".into(), &config, &events)
            .unwrap();
    }
    let error = speech
        .enqueue("turn", "overflow", "text".into(), &config, &events)
        .unwrap_err();
    assert_eq!(error.to_string(), "speech queue is full");
    speech.stop();
}

#[cfg(unix)]
#[tokio::test]
async fn commands_receive_literal_arguments_and_stdin_in_order_without_duplicates()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("speech.txt");
    let config = TtsConfig {
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf '%s\\n' \"$2\" >> \"$1\"; cat >> \"$1\"; printf '\\nEND\\n' >> \"$1\"".into(),
            "fake-say".into(),
            output.to_string_lossy().into_owned(),
            "literal argument; $(not-a-command)".into(),
        ],
        ..Default::default()
    };
    let (tx, _rx) = mpsc::unbounded_channel();
    let events = AppEventSender::new(tx);
    let mut speech = Speech::default();
    speech.enqueue("turn", "off", "silent".into(), &config, &events)?;
    speech.set_mode(TtsMode::ProgressAndFinal);
    speech.enqueue(
        "turn",
        "first",
        "--hello; $(literal)\n¡Sí!".into(),
        &config,
        &events,
    )?;
    speech.enqueue("turn", "first", "duplicate".into(), &config, &events)?;
    speech.enqueue("turn", "second", "Second message".into(), &config, &events)?;
    let expected = "literal argument; $(not-a-command)\n--hello; $(literal)\n¡Sí!\nEND\nliteral argument; $(not-a-command)\nSecond message\nEND\n";
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(&output).is_ok_and(|contents| contents == expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(std::fs::read_to_string(output)?, expected);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn stopping_or_dropping_speech_kills_the_active_command_and_discards_queued_text()
-> anyhow::Result<()> {
    for drop_speech in [false, true] {
        let directory = tempfile::tempdir()?;
        let pid_file = directory.path().join("pid");
        let config = TtsConfig {
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf '%s' \"$$\" > \"$1\"; exec sleep 30".into(),
                "fake-say".into(),
                pid_file.to_string_lossy().into_owned(),
            ],
            ..Default::default()
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let events = AppEventSender::new(tx);
        let mut speech = Speech::default();
        speech.set_mode(TtsMode::Final);
        speech.enqueue("turn", "active", "First".into(), &config, &events)?;
        speech.enqueue("turn", "queued", "Second".into(), &config, &events)?;
        let pid: i32 = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(contents) = std::fs::read_to_string(&pid_file)
                    && let Ok(pid) = contents.parse()
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        if drop_speech {
            drop(speech);
        } else {
            speech.stop();
            assert_eq!(speech.mode(), TtsMode::Final);
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            // Signal zero probes existence without sending a signal to the fake command.
            while unsafe {
                libc::kill(pid, /*sig*/ 0)
            } == 0
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(std::fs::read_to_string(pid_file)?, pid.to_string());
    }
    Ok(())
}
