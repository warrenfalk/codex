use super::resolve_relative_paths_in_config_toml;
use super::tests::TestFileSystem;
use crate::config_toml::ConfigToml;
use crate::load_permission_profile_instructions;
use crate::merge::merge_toml_values;
use std::collections::BTreeMap;
use std::io;
use tempfile::tempdir;

use pretty_assertions::assert_eq;

#[tokio::test]
async fn instruction_files_inherit_and_override_across_config_directories() -> anyhow::Result<()> {
    let parent_dir = tempdir()?;
    let child_dir = tempdir()?;
    std::fs::write(parent_dir.path().join("policy.md"), "Parent instructions")?;
    std::fs::write(child_dir.path().join("policy.md"), "Child instructions")?;
    std::fs::write(child_dir.path().join("empty.md"), "")?;
    let mut parent = resolve_relative_paths_in_config_toml(
        toml::from_str(
            r#"[permissions.parent]
instructions_file = "policy.md"
[permissions.inherited]
extends = "parent"
"#,
        )?,
        parent_dir.path(),
    )?;
    let child = resolve_relative_paths_in_config_toml(
        toml::from_str(
            r#"[permissions.child]
extends = "inherited"
instructions_file = "policy.md"
[permissions.empty]
extends = "child"
instructions_file = "empty.md"
"#,
        )?,
        child_dir.path(),
    )?;
    merge_toml_values(&mut parent, &child);
    let config: ConfigToml = parent.try_into()?;
    let instructions =
        load_permission_profile_instructions(&TestFileSystem, config.permissions.as_ref(), |_| {
            None
        })
        .await?;
    assert_eq!(
        instructions,
        BTreeMap::from([
            ("parent".to_string(), "Parent instructions".to_string()),
            ("inherited".to_string(), "Parent instructions".to_string()),
            ("child".to_string(), "Child instructions".to_string()),
            ("empty".to_string(), String::new()),
        ])
    );
    Ok(())
}

#[tokio::test]
async fn instruction_files_report_read_errors_and_enforce_the_size_limit() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let config: ConfigToml = resolve_relative_paths_in_config_toml(
        toml::from_str("[permissions.work]\ninstructions_file = 'policy.md'")?,
        dir.path(),
    )?
    .try_into()?;
    let path = dir.path().join("policy.md");
    for (contents, expected_kind) in [
        (None, io::ErrorKind::NotFound),
        (Some(vec![b'x'; 4_001]), io::ErrorKind::InvalidInput),
        (Some(vec![0xff]), io::ErrorKind::InvalidData),
    ] {
        if let Some(contents) = contents {
            std::fs::write(&path, contents)?;
        }
        let error = load_permission_profile_instructions(
            &TestFileSystem,
            config.permissions.as_ref(),
            |_| None,
        )
        .await
        .expect_err("invalid instruction file must fail config loading");
        assert_eq!(error.kind(), expected_kind);
        assert!(error.to_string().contains("permission profile `work`"));
        assert!(error.to_string().contains(&path.display().to_string()));
    }
    let contents = "x".repeat(4_000);
    std::fs::write(path, &contents)?;
    assert_eq!(
        load_permission_profile_instructions(&TestFileSystem, config.permissions.as_ref(), |_| {
            None
        })
        .await?,
        BTreeMap::from([("work".to_string(), contents)]),
    );
    Ok(())
}
