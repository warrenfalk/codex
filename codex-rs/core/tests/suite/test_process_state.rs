use pretty_assertions::assert_eq;
use std::path::PathBuf;

#[test]
fn test_process_keeps_default_state_out_of_inherited_codex_home() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let inherited_home = temp.path().join("inherited-home");
    std::fs::create_dir(&inherited_home)?;
    std::fs::write(inherited_home.join("sentinel"), "caller state")?;
    let report = temp.path().join("report.json");
    let (_, test_module) = module_path!().split_once("::").expect("test module path");
    let fixture_test = format!("{test_module}::write_default_test_state");
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            &fixture_test,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("CODEX_HOME", &inherited_home)
        .env("CODEX_TEST_PROCESS_HOME_REPORT", &report)
        .env("TMPDIR", temp.path())
        .env("TMP", temp.path())
        .env("TEMP", temp.path())
        .output()?;
    assert!(
        output.status.success(),
        "test subprocess failed: {output:?}"
    );
    let active_home: PathBuf = serde_json::from_slice(&std::fs::read(report)?)?;
    let mut inherited_entries = std::fs::read_dir(&inherited_home)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    inherited_entries.sort();
    assert_eq!(
        (
            active_home == inherited_home,
            inherited_entries,
            std::fs::read_to_string(inherited_home.join("sentinel"))?,
        ),
        (
            false,
            vec![std::ffi::OsString::from("sentinel")],
            "caller state".to_string(),
        ),
    );
    assert_eq!(
        std::fs::read_to_string(active_home.join("test-state"))?,
        "private state"
    );
    Ok(())
}

#[test]
#[ignore = "invoked in a subprocess with an inherited Codex home"]
fn write_default_test_state() -> anyhow::Result<()> {
    let Some(report) = std::env::var_os("CODEX_TEST_PROCESS_HOME_REPORT") else {
        return Ok(());
    };
    let home = codex_utils_home_dir::find_codex_home()?;
    std::fs::write(home.join("test-state"), "private state")?;
    std::fs::write(report, serde_json::to_vec(&home)?)?;
    Ok(())
}
