use super::*;
use crate::LoaderOverrides;
use crate::loader::tests::TestFileSystem;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn reload_tracks_policy_addition_replacement_removal_and_filters() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let cwd = AbsolutePathBuf::from_absolute_path(home.path())?;
    let file = home.path().join("config.toml");
    let layers = ConfigLayerStack::default();
    let mut reload = ShellEnvironmentPolicyReload::new(
        LoaderOverrides::without_managed_config_for_tests().into(),
        ShellEnvironmentPolicy::default(),
    );
    for contents in [
        "[shell_environment_policy.set]\nGH_TOKEN = 'synthetic-first'",
        "[shell_environment_policy]\ninherit = 'none'\n[shell_environment_policy.set]\nGH_TOKEN = 'synthetic-second'\n[shell_environment_policy.filters]\nPATH = 'include'",
        "",
    ] {
        std::fs::write(&file, contents)?;
        let config: ShellConfig = toml::from_str(contents)?;
        assert_eq!(
            reload
                .reload(&TestFileSystem, home.path(), cwd.clone(), &layers)
                .await?,
            Some(ShellEnvironmentPolicy::from(
                config.shell_environment_policy
            )),
        );
        assert_eq!(
            reload
                .reload(&TestFileSystem, home.path(), cwd.clone(), &layers)
                .await?,
            None
        );
    }
    Ok(())
}

#[tokio::test]
async fn reload_retains_session_overrides_and_recovers_after_malformed_config() -> anyhow::Result<()>
{
    let home = tempfile::tempdir()?;
    let cwd = AbsolutePathBuf::from_absolute_path(home.path())?;
    let file = home.path().join("config.toml");
    let options: ConfigLoadOptions = LoaderOverrides::without_managed_config_for_tests().into();
    let overrides = vec![(
        "shell_environment_policy.set.GH_TOKEN".to_string(),
        toml::Value::String("synthetic-session".to_string()),
    )];
    let layers = load_config_layers_state(
        &TestFileSystem,
        home.path(),
        Some(cwd.clone()),
        &overrides,
        options.clone(),
        &NoopThreadConfigLoader,
    )
    .await?;
    let config: ShellConfig = layers.effective_config().try_into()?;
    let observed = ShellEnvironmentPolicy::from(config.shell_environment_policy);
    let mut reload = ShellEnvironmentPolicyReload::new(options, observed);
    std::fs::write(
        &file,
        "[shell_environment_policy.set]\nGH_TOKEN = 'synthetic-file'",
    )?;
    assert_eq!(
        reload
            .reload(&TestFileSystem, home.path(), cwd.clone(), &layers)
            .await?,
        None
    );
    std::fs::write(&file, "[shell_environment_policy.set\n")?;
    assert!(
        reload
            .reload(&TestFileSystem, home.path(), cwd.clone(), &layers)
            .await
            .is_err()
    );
    std::fs::write(&file, "[shell_environment_policy.set]\nOTHER = 'recovered'")?;
    let config: ShellConfig = toml::from_str(
        "[shell_environment_policy.set]\nGH_TOKEN = 'synthetic-session'\nOTHER = 'recovered'",
    )?;
    assert_eq!(
        reload
            .reload(&TestFileSystem, home.path(), cwd, &layers)
            .await?,
        Some(ShellEnvironmentPolicy::from(
            config.shell_environment_policy
        )),
    );
    Ok(())
}

#[tokio::test]
async fn reload_discovers_new_project_config_and_honors_trust() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(project.join(".codex"))?;
    std::fs::create_dir(project.join(".git"))?;
    let cwd = AbsolutePathBuf::from_absolute_path(&project)?;
    let options: ConfigLoadOptions = LoaderOverrides::without_managed_config_for_tests().into();
    let mut reload = ShellEnvironmentPolicyReload::new(options, ShellEnvironmentPolicy::default());
    let layers = ConfigLayerStack::default();
    let project_config = "[shell_environment_policy.set]\nGH_TOKEN = 'synthetic-project'";
    std::fs::write(project.join(".codex/config.toml"), project_config)?;
    assert_eq!(
        reload
            .reload(&TestFileSystem, &home, cwd.clone(), &layers)
            .await?,
        None
    );
    let projects = toml::Table::from_iter([(
        project.to_string_lossy().into_owned(),
        toml::Value::Table(toml::toml! { trust_level = "trusted" }),
    )]);
    let config = toml::Table::from_iter([("projects".to_string(), toml::Value::Table(projects))]);
    std::fs::write(home.join("config.toml"), toml::to_string(&config)?)?;
    let expected: ShellConfig = toml::from_str(project_config)?;
    assert_eq!(
        reload
            .reload(&TestFileSystem, &home, cwd.clone(), &layers)
            .await?,
        Some(ShellEnvironmentPolicy::from(
            expected.shell_environment_policy
        )),
    );
    std::fs::remove_file(home.join("config.toml"))?;
    assert_eq!(
        reload.reload(&TestFileSystem, &home, cwd, &layers).await?,
        Some(ShellEnvironmentPolicy::default()),
    );
    Ok(())
}

#[tokio::test]
async fn reload_uses_selected_profile_file() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let cwd = AbsolutePathBuf::from_absolute_path(home.path())?;
    let file = cwd.join("profiles/work.toml");
    std::fs::create_dir_all(file.parent().unwrap())?;
    std::fs::write(
        home.path().join("config.toml"),
        "[shell_environment_policy.set]\nGH_TOKEN = 'base'",
    )?;
    let options = ConfigLoadOptions {
        loader_overrides: LoaderOverrides {
            user_config_path: Some(file.clone()),
            user_config_profile: Some("work".parse()?),
            ..LoaderOverrides::without_managed_config_for_tests()
        },
        ..Default::default()
    };
    let mut reload = ShellEnvironmentPolicyReload::new(options, ShellEnvironmentPolicy::default());
    for value in ["profile-first", "profile-second"] {
        let contents = format!("[shell_environment_policy.set]\nGH_TOKEN = '{value}'");
        std::fs::write(&file, &contents)?;
        let expected: ShellConfig = toml::from_str(&contents)?;
        assert_eq!(
            reload
                .reload(
                    &TestFileSystem,
                    home.path(),
                    cwd.clone(),
                    &ConfigLayerStack::default()
                )
                .await?,
            Some(ShellEnvironmentPolicy::from(
                expected.shell_environment_policy
            )),
        );
    }
    Ok(())
}
