use super::*;
use pretty_assertions::assert_eq;

#[test]
fn kitty_target_comes_from_tui_environment_and_rejects_unsupported_contexts() {
    for id in ["1", "42"] {
        let cmd = command(|name| (name == "KITTY_WINDOW_ID").then(|| id.into())).unwrap();
        assert_eq!(
            (
                cmd.as_std().get_program().to_string_lossy().into_owned(),
                cmd.as_std()
                    .get_args()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            ),
            (
                "kitten".into(),
                vec![
                    "@".into(),
                    "--use-password".into(),
                    "never".into(),
                    "focus-window".into(),
                    "--match".into(),
                    format!("id:{id}")
                ]
            ),
        );
    }
    for id in [None, Some("0"), Some("invalid")] {
        assert!(
            command(|name| if name == "KITTY_WINDOW_ID" {
                id.map(Into::into)
            } else {
                None
            })
            .is_err()
        );
    }
    for multiplexer in ["TMUX", "STY"] {
        assert!(
            command(|name| ["KITTY_WINDOW_ID", multiplexer]
                .contains(&name)
                .then(|| "42".into()))
            .unwrap_err()
            .to_string()
            .contains("inner pane")
        );
    }
}

#[tokio::test]
async fn propagates_remote_control_rejection_and_missing_executable() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo 'remote control disabled' >&2; exit 1"]);
    let error = focus(command).await.unwrap_err();
    assert!(error.to_string().contains("remote control disabled"));
    let dir = tempfile::tempdir().unwrap();
    let error = focus(Command::new(dir.path().join("missing-kitten")))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Could not run kitten"));
}

#[tokio::test]
async fn acknowledged_focus_succeeds_and_hung_command_times_out() {
    let mut command = Command::new("sh");
    command.args(["-c", "exit 0"]);
    focus(command).await.unwrap();
    let mut command = Command::new("sh");
    command.args(["-c", "exec sleep 30"]);
    assert!(
        focus(command)
            .await
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
}
