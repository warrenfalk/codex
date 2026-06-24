use super::*;
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use tokio::time::timeout;

#[test]
fn discover_envrc_uses_parent_lookup() {
    let temp = tempfile::tempdir().expect("temp dir");
    let nested = temp.path().join("a/b/c");
    std::fs::create_dir_all(&nested).expect("create nested dir");
    let envrc = temp.path().join("a/.envrc");
    std::fs::write(&envrc, "export FOO=bar").expect("write envrc");

    assert_eq!(discover_envrc(&nested), Some(envrc));
}

#[test]
fn apply_overlay_keeps_shell_environment_set_above_direnv() {
    let mut env = HashMap::from([
        ("FOO".to_string(), "base".to_string()),
        ("CODEX_THREAD_ID".to_string(), "thread".to_string()),
    ]);
    let overlay = ProjectEnvOverlay {
        env: HashMap::from([
            ("FOO".to_string(), "direnv".to_string()),
            ("BAR".to_string(), "direnv".to_string()),
            ("CODEX_THREAD_ID".to_string(), "direnv".to_string()),
        ]),
        envrc_path: AbsolutePathBuf::from_absolute_path("/tmp/.envrc").expect("absolute path"),
        watched_input_count: 1,
    };
    apply_overlay(
        &mut env,
        &overlay,
        &HashMap::from([("FOO".to_string(), "set".to_string())]),
        Some("thread".to_string()),
    );

    assert_eq!(
        env,
        HashMap::from([
            ("FOO".to_string(), "set".to_string()),
            ("BAR".to_string(), "direnv".to_string()),
            ("CODEX_THREAD_ID".to_string(), "thread".to_string()),
        ])
    );
}

#[test]
fn parse_watched_paths_includes_envrc_and_deduplicates() {
    let cwd = Path::new("/tmp/project");
    let envrc = Path::new("/tmp/project/.envrc");
    assert_eq!(
        parse_watched_paths("flake.nix\n/tmp/project/.envrc\n", cwd, envrc),
        vec![
            PathBuf::from("/tmp/project/.envrc"),
            PathBuf::from("/tmp/project/flake.nix"),
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn successful_capture_injects_exported_env_and_uses_private_xdg_state() {
    let fixture = FakeDirenvFixture::new(
        r#"case "$1" in
  version)
    echo fake-direnv
    ;;
  allow)
    test "$2" = "$FAKE_PROJECT_DIR/.envrc" || exit 10
    test "$XDG_CONFIG_HOME" = "$FAKE_PROJECT_DIR/.direnv/codex/project-env/xdg/config" || exit 11
    test "$XDG_DATA_HOME" = "$FAKE_PROJECT_DIR/.direnv/codex/project-env/xdg/data" || exit 12
    test "$XDG_CACHE_HOME" = "$FAKE_PROJECT_DIR/.direnv/codex/project-env/xdg/cache" || exit 13
    test -z "${DIRENV_CONFIG+x}" || exit 14
    test -z "${DIRENV_LAYOUT_DIR+x}" || exit 15
    : > "$FAKE_PROJECT_DIR/allow-ran"
    ;;
  export)
    test "$2" = json || exit 16
    printf '{"FOO":"from_direnv","DIRENV_DIFF":"diff"}'
    ;;
  watch-print)
    printf '.envrc\nflake.nix\n'
    ;;
  *)
    exit 99
    ;;
esac
"#,
    );
    fixture.write_project_file(".envrc", "export FOO=from_direnv\n");
    fixture.write_project_file("flake.nix", "{}\n");
    let manager = fixture.manager();
    let mut status_rx = manager.subscribe_status();

    let overlay = manager
        .environment_for_command(
            &fixture.cwd(),
            ProjectEnvMode::Auto,
            CancellationToken::new(),
        )
        .await
        .expect("project env loads")
        .expect("envrc applies");

    assert_eq!(
        overlay.env,
        HashMap::from([
            ("DIRENV_DIFF".to_string(), "diff".to_string()),
            ("FOO".to_string(), "from_direnv".to_string()),
        ])
    );
    assert_eq!(overlay.watched_input_count, 2);
    let project_path = fixture.project_path();
    assert!(project_path.join("allow-ran").is_file());
    assert!(
        project_path
            .join(".direnv/codex/project-env/xdg/config")
            .is_dir()
    );

    let building = timeout(Duration::from_secs(1), status_rx.recv())
        .await
        .expect("building status")
        .expect("status channel open");
    assert_eq!(building.state, ProjectEnvState::Building);

    let ready = timeout(Duration::from_secs(1), status_rx.recv())
        .await
        .expect("ready status")
        .expect("status channel open");
    assert_eq!(ready.state, ProjectEnvState::Ready);
    assert_eq!(ready.watched_input_count, 2);
}

#[cfg(unix)]
#[tokio::test]
async fn bypass_skips_capture_even_when_envrc_exists() {
    let fixture = FakeDirenvFixture::new("exit 42\n");
    fixture.write_project_file(".envrc", "exit 1\n");

    let overlay = fixture
        .manager()
        .environment_for_command(
            &fixture.cwd(),
            ProjectEnvMode::Bypass,
            CancellationToken::new(),
        )
        .await
        .expect("bypass succeeds");

    assert_eq!(overlay, None);
}

#[cfg(unix)]
#[tokio::test]
async fn failed_capture_blocks_auto_with_bypass_hint() {
    let fixture = FakeDirenvFixture::new(
        r#"case "$1" in
  version)
    echo fake-direnv
    ;;
  allow)
    exit 0
    ;;
  export)
    echo broken envrc >&2
    exit 17
    ;;
  watch-print)
    printf '.envrc\n'
    ;;
esac
"#,
    );
    fixture.write_project_file(".envrc", "exit 1\n");

    let err = fixture
        .manager()
        .environment_for_command(
            &fixture.cwd(),
            ProjectEnvMode::Auto,
            CancellationToken::new(),
        )
        .await
        .expect_err("broken direnv blocks auto");

    let message = err.model_message();
    assert!(message.contains("broken envrc"));
    assert!(message.contains(r#"project_env: "bypass""#));
}

#[cfg(unix)]
#[tokio::test]
async fn reverted_watched_input_fingerprint_reuses_cached_env() {
    let fixture = FakeDirenvFixture::new(
        r#"case "$1" in
  version)
    echo fake-direnv
    ;;
  allow)
    exit 0
    ;;
  export)
    printf x >> "$FAKE_PROJECT_DIR/captures"
    if test -f "$FAKE_PROJECT_DIR/toggle"; then
      printf '{"FOO":"present"}'
    else
      printf '{"FOO":"missing"}'
    fi
    ;;
  watch-print)
    printf '.envrc\ntoggle\n'
    ;;
esac
"#,
    );
    fixture.write_project_file(".envrc", "watch_file toggle\n");
    let manager = fixture.manager();
    let cwd = fixture.cwd();

    let first = manager
        .environment_for_command(&cwd, ProjectEnvMode::Auto, CancellationToken::new())
        .await
        .expect("project env loads")
        .expect("envrc applies");
    assert_eq!(first.env.get("FOO"), Some(&"missing".to_string()));
    assert_eq!(
        std::fs::read_to_string(fixture.project_path().join("captures")).expect("captures file"),
        "x"
    );

    fixture.write_project_file("toggle", "on\n");
    let second = manager
        .environment_for_command(&cwd, ProjectEnvMode::Auto, CancellationToken::new())
        .await
        .expect("project env reloads")
        .expect("envrc applies");
    assert_eq!(second.env.get("FOO"), Some(&"present".to_string()));
    assert_eq!(
        std::fs::read_to_string(fixture.project_path().join("captures")).expect("captures file"),
        "xx"
    );

    std::fs::remove_file(fixture.project_path().join("toggle")).expect("remove watched file");
    let third = manager
        .environment_for_command(&cwd, ProjectEnvMode::Auto, CancellationToken::new())
        .await
        .expect("project env cache is reused")
        .expect("envrc applies");
    assert_eq!(third.env.get("FOO"), Some(&"missing".to_string()));
    assert_eq!(
        std::fs::read_to_string(fixture.project_path().join("captures")).expect("captures file"),
        "xx"
    );
}

#[cfg(unix)]
struct FakeDirenvFixture {
    temp: tempfile::TempDir,
}

#[cfg(unix)]
impl FakeDirenvFixture {
    fn new(script_body: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir(&bin_dir).expect("create bin dir");
        let direnv = bin_dir.join("direnv");
        std::fs::write(&direnv, format!("#!/bin/sh\n{script_body}")).expect("write fake direnv");
        let mut permissions = std::fs::metadata(&direnv)
            .expect("fake direnv metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&direnv, permissions).expect("chmod fake direnv");
        std::fs::create_dir(temp.path().join("project")).expect("create project dir");
        Self { temp }
    }

    fn project_path(&self) -> PathBuf {
        self.temp.path().join("project")
    }

    fn cwd(&self) -> AbsolutePathBuf {
        AbsolutePathBuf::try_from(self.project_path()).expect("absolute cwd")
    }

    fn write_project_file(&self, relative: &str, contents: &str) {
        std::fs::write(self.project_path().join(relative), contents).expect("write project file");
    }

    fn manager(&self) -> ProjectEnvManager {
        ProjectEnvManager::new(ProjectEnvConfig {
            disabled: false,
            process_env: HashMap::from([
                (
                    "PATH".to_string(),
                    self.temp.path().join("bin").to_string_lossy().to_string(),
                ),
                (
                    "FAKE_PROJECT_DIR".to_string(),
                    self.project_path().to_string_lossy().to_string(),
                ),
                ("DIRENV_CONFIG".to_string(), "must-be-removed".to_string()),
            ]),
        })
    }
}
