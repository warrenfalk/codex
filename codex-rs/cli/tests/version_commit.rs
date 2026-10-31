use anyhow::Result;
use tempfile::TempDir;

#[test]
fn version_commit_prints_build_stamp_without_loading_config() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(home.path().join("config.toml"), "invalid = [")?;
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    let result = cmd
        .current_dir(home.path())
        .env("CODEX_HOME", home.path())
        .env("STABLE_GIT_COMMIT", "runtime-value-must-not-be-used")
        .arg("--version-commit")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&result.get_output().stdout)?;
    insta::assert_snapshot!(
        stdout.replace(option_env!("STABLE_GIT_COMMIT").unwrap_or("unknown"), "<source-commit>"),
        @"<source-commit>"
    );
    Ok(())
}
