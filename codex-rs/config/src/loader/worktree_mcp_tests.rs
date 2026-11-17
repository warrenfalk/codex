use crate::ConfigLayerSource;
use crate::ConfigLayerStack;
use crate::ConfigLoadOptions;
use crate::LoaderOverrides;
use crate::McpServerConfig;
use crate::NoopThreadConfigLoader;
use crate::loader::LocalConfigLayers;
use crate::loader::load_config_layers_state;
use crate::loader::local::load_local_config_layers_with_overrides;
use crate::loader::project_trust_key;
use crate::loader::tests::TestFileSystem;
use crate::merge_toml_values;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::TempDir;
use toml::Value as TomlValue;

struct Fixture {
    _temp: TempDir,
    home: AbsolutePathBuf,
    repo: AbsolutePathBuf,
    cwd: AbsolutePathBuf,
    overrides: LoaderOverrides,
}

impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let temp = tempfile::tempdir()?;
        let root = AbsolutePathBuf::from_absolute_path(temp.path().canonicalize()?)?;
        let home = root.join("home");
        let repo = root.join("repo");
        let cwd = root.join("worktree");
        let gitdir = repo.join(".git/worktrees/topic");
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&gitdir)?;
        std::fs::create_dir_all(&cwd)?;
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n")?;
        std::fs::write(cwd.join(".git"), format!("gitdir: {}\n", gitdir.display()))?;
        std::fs::write(
            gitdir.join("gitdir"),
            format!("{}\n", cwd.join(".git").display()),
        )?;
        std::fs::write(gitdir.join("commondir"), "../..\n")?;
        let mut overrides = LoaderOverrides::without_managed_config_for_tests();
        overrides.system_config_path = Some(root.join("system.toml").to_path_buf());
        let fixture = Self {
            _temp: temp,
            home,
            repo,
            cwd,
            overrides,
        };
        fixture.write_user("")?;
        Ok(fixture)
    }

    fn write_user(&self, extra: &str) -> anyhow::Result<()> {
        let key = TomlValue::String(project_trust_key(self.repo.as_path()));
        std::fs::write(
            self.home.join("config.toml"),
            format!("model = 'user-model'\n[projects.{key}]\ntrust_level = 'trusted'\n{extra}"),
        )?;
        Ok(())
    }

    fn write_project(dir: &AbsolutePathBuf, config: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir.join(".codex"))?;
        std::fs::write(dir.join(".codex/config.toml"), config)?;
        Ok(())
    }

    async fn load(&self) -> anyhow::Result<(ConfigLayerStack, LocalConfigLayers)> {
        let canonical = load_config_layers_state(
            &TestFileSystem,
            self.home.as_path(),
            Some(self.cwd.clone()),
            &[],
            self.overrides.clone(),
            &NoopThreadConfigLoader,
        )
        .await?;
        let local = load_local_config_layers_with_overrides(
            &TestFileSystem,
            self.home.as_path(),
            &self.cwd,
            &self.overrides,
        )
        .await?;
        Ok((canonical, local))
    }
}

fn assert_servers(
    canonical: &ConfigLayerStack,
    local: &LocalConfigLayers,
    expected: &str,
) -> anyhow::Result<()> {
    let expected: TomlValue = toml::from_str(expected)?;
    let expected_servers: BTreeMap<String, McpServerConfig> = expected
        .get("mcp_servers")
        .cloned()
        .unwrap_or_else(|| TomlValue::Table(toml::map::Map::new()))
        .try_into()?;
    let mut local_effective = TomlValue::Table(toml::map::Map::new());
    for layer in &local.config.layers {
        merge_toml_values(&mut local_effective, &layer.toml);
    }
    for effective in [canonical.effective_config(), local_effective] {
        // Check the effective transport too; recursive merging can otherwise
        // produce a plausible TOML table that is not a usable MCP definition.
        let servers: BTreeMap<String, McpServerConfig> = effective
            .get("mcp_servers")
            .cloned()
            .unwrap_or_else(|| TomlValue::Table(toml::map::Map::new()))
            .try_into()?;
        assert_eq!(servers, expected_servers);
    }
    Ok(())
}

#[tokio::test]
async fn fresh_worktree_inherits_only_mcp_and_preserves_source() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "model = 'main-model'\napproval_policy = 'never'\n\
         [mcp_servers.linear]\nurl = 'https://linear.example/mcp'\n\
         [hooks]\nSessionStart = []",
    )?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(
        &canonical,
        &local,
        "[mcp_servers.linear]\nurl = 'https://linear.example/mcp'",
    )?;
    let source = ConfigLayerSource::Project {
        dot_codex_folder: fixture.repo.join(".codex"),
    };
    let inherited = canonical
        .layers_low_to_high()
        .find(|layer| layer.name == source)
        .expect("main-checkout MCP layer");
    assert_eq!(
        (
            canonical.effective_config().get("model").cloned(),
            inherited
                .config
                .as_table()
                .map(|table| table.keys().cloned().collect::<Vec<_>>()),
            inherited.config_folder(),
            inherited.hooks_config_folder(),
            canonical.origins()["mcp_servers.linear.url"].name.clone(),
            fixture.cwd.join(".codex").exists(),
        ),
        (
            Some(TomlValue::String("user-model".into())),
            Some(vec!["mcp_servers".into()]),
            None,
            None,
            source,
            false,
        ),
    );
    Ok(())
}

#[tokio::test]
async fn worktree_overrides_disable_and_add_servers_without_suppressing_defaults()
-> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.linear]\nurl = 'https://linear.example/mcp'\n\
         [mcp_servers.other]\nurl = 'https://other.example/mcp'\nstartup_timeout_sec = 15",
    )?;
    Fixture::write_project(
        &fixture.cwd,
        "model = 'worktree-model'\n\
         [mcp_servers.linear]\nenabled = false\n\
         [mcp_servers.other]\nstartup_timeout_sec = 30\nenabled_tools = ['read']\n\
         [mcp_servers.local]\ncommand = 'echo'",
    )?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(
        &canonical,
        &local,
        "[mcp_servers.linear]\nurl = 'https://linear.example/mcp'\nenabled = false\n\
         [mcp_servers.other]\nurl = 'https://other.example/mcp'\nstartup_timeout_sec = 30\nenabled_tools = ['read']\n\
         [mcp_servers.local]\ncommand = 'echo'",
    )?;
    assert_eq!(
        canonical.origins()["mcp_servers.linear.enabled"].name,
        ConfigLayerSource::Project {
            dot_codex_folder: fixture.cwd.join(".codex"),
        },
    );
    Ok(())
}

#[tokio::test]
async fn main_checkout_edits_are_read_again_without_copying() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(&fixture.cwd, "model = 'worktree-model'")?;
    for url in ["https://first.example/mcp", "https://second.example/mcp"] {
        let config = format!("[mcp_servers.linear]\nurl = '{url}'");
        Fixture::write_project(&fixture.repo, &config)?;
        let (canonical, local) = fixture.load().await?;
        assert_servers(&canonical, &local, &config)?;
    }
    assert_eq!(
        std::fs::read_to_string(fixture.cwd.join(".codex/config.toml"))?,
        "model = 'worktree-model'",
    );
    Ok(())
}

#[tokio::test]
async fn nested_main_defaults_stay_below_all_worktree_overrides() -> anyhow::Result<()> {
    let mut fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.linear]\nurl = 'https://root.example/mcp'\nstartup_timeout_sec = 10",
    )?;
    Fixture::write_project(
        &fixture.repo.join("child"),
        "[mcp_servers.linear]\nurl = 'https://child.example/mcp'\n\
         [mcp_servers.child]\ncommand = 'echo'\ncwd = 'relative-server-dir'",
    )?;
    Fixture::write_project(
        &fixture.cwd,
        "[mcp_servers.linear]\nurl = 'https://worktree.example/mcp'",
    )?;
    fixture.cwd = fixture.cwd.join("child");
    std::fs::create_dir_all(&fixture.cwd)?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(
        &canonical,
        &local,
        "[mcp_servers.linear]\nurl = 'https://worktree.example/mcp'\nstartup_timeout_sec = 10\n\
         [mcp_servers.child]\ncommand = 'echo'\ncwd = 'relative-server-dir'",
    )?;
    Ok(())
}

#[tokio::test]
async fn worktree_can_switch_inherited_transports_in_both_directions() -> anyhow::Result<()> {
    let mut fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.to_stdio]\nurl = 'https://main.example/mcp'\n\
         http_headers = { Example = 'header' }\nauth = 'oauth'\nstartup_timeout_sec = 10\n\
         [mcp_servers.to_http]\ncommand = 'echo'\nargs = ['main']\nenv = { EXAMPLE = 'value' }\ncwd = 'main-dir'",
    )?;
    Fixture::write_project(
        &fixture.repo.join("child"),
        "[mcp_servers.to_stdio]\nenv_http_headers = { Example = 'EXAMPLE' }\n\
         [mcp_servers.to_http]\nenv_vars = ['EXAMPLE']",
    )?;
    Fixture::write_project(
        &fixture.cwd,
        "[mcp_servers.to_stdio]\ncommand = 'local-server'\nargs = ['worktree']\n\
         [mcp_servers.to_http]\nurl = 'https://worktree.example/mcp'",
    )?;
    fixture.cwd = fixture.cwd.join("child");
    std::fs::create_dir_all(&fixture.cwd)?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(
        &canonical,
        &local,
        "[mcp_servers.to_stdio]\ncommand = 'local-server'\nargs = ['worktree']\nstartup_timeout_sec = 10\n\
         [mcp_servers.to_http]\nurl = 'https://worktree.example/mcp'",
    )?;
    Ok(())
}

#[tokio::test]
async fn project_boundary_and_project_config_opt_out_are_respected() -> anyhow::Result<()> {
    let mut fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.root]\nurl = 'https://root.example/mcp'",
    )?;
    let child_config = "[mcp_servers.child]\nurl = 'https://child.example/mcp'";
    Fixture::write_project(&fixture.repo.join("child"), child_config)?;
    let key = TomlValue::String(project_trust_key(fixture.repo.as_path()));
    std::fs::write(
        fixture.home.join("config.toml"),
        format!(
            "project_root_markers = ['.company-root']\n[projects.{key}]\ntrust_level = 'trusted'"
        ),
    )?;
    fixture.cwd = fixture.cwd.join("child");
    std::fs::create_dir_all(&fixture.cwd)?;
    std::fs::write(fixture.cwd.join(".company-root"), "")?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(&canonical, &local, child_config)?;
    fixture.overrides.ignore_project_config = true;
    let canonical = load_config_layers_state(
        &TestFileSystem,
        fixture.home.as_path(),
        Some(fixture.cwd.clone()),
        &[],
        fixture.overrides.clone(),
        &NoopThreadConfigLoader,
    )
    .await?;
    assert_eq!(canonical.effective_config().get("mcp_servers"), None);
    Ok(())
}

#[tokio::test]
async fn session_overrides_win_over_main_defaults() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.write_user(
        "[mcp_servers.linear]\nurl = 'https://user.example/mcp'\nstartup_timeout_sec = 10",
    )?;
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.linear]\nurl = 'https://main.example/mcp'",
    )?;
    let canonical = load_config_layers_state(
        &TestFileSystem,
        fixture.home.as_path(),
        Some(fixture.cwd.clone()),
        &[(
            "mcp_servers.linear.url".into(),
            TomlValue::String("https://session.example/mcp".into()),
        )],
        fixture.overrides.clone(),
        &NoopThreadConfigLoader,
    )
    .await?;
    let servers: BTreeMap<String, McpServerConfig> = canonical.effective_config()["mcp_servers"]
        .clone()
        .try_into()?;
    let expected: TomlValue = toml::from_str(
        "[mcp_servers.linear]\nurl = 'https://session.example/mcp'\nstartup_timeout_sec = 10",
    )?;
    assert_eq!(
        servers,
        expected["mcp_servers"]
            .clone()
            .try_into::<BTreeMap<String, McpServerConfig>>()?
    );
    assert_eq!(
        canonical.origins()["mcp_servers.linear.url"].name,
        ConfigLayerSource::SessionFlags
    );
    Ok(())
}

#[tokio::test]
async fn untrusted_worktree_does_not_read_or_inherit_main_config() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(&fixture.repo, "this is not valid TOML")?;
    let key = TomlValue::String(project_trust_key(fixture.cwd.as_path()));
    fixture.write_user(&format!("[projects.{key}]\ntrust_level = 'untrusted'"))?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(&canonical, &local, "")?;
    Ok(())
}

#[tokio::test]
async fn unregistered_worktree_pointer_does_not_inherit_main_mcp() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.linear]\nurl = 'https://main.example/mcp'",
    )?;
    std::fs::write(
        fixture.repo.join(".git/worktrees/topic/gitdir"),
        format!("{}\n", fixture.repo.join("wrong/.git").display()),
    )?;
    let key = TomlValue::String(project_trust_key(fixture.cwd.as_path()));
    fixture.write_user(&format!("[projects.{key}]\ntrust_level = 'trusted'"))?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(&canonical, &local, "")?;
    Ok(())
}

#[tokio::test]
async fn ordinary_checkout_and_missing_main_config_use_existing_layers() -> anyhow::Result<()> {
    let mut fixture = Fixture::new()?;
    let (canonical, local) = fixture.load().await?;
    assert_servers(&canonical, &local, "")?;
    let config = "[mcp_servers.linear]\nurl = 'https://main.example/mcp'";
    Fixture::write_project(&fixture.repo, config)?;
    fixture.cwd = fixture.repo.clone();
    let (canonical, local) = fixture.load().await?;
    assert_servers(&canonical, &local, config)?;
    let projects = canonical
        .layers_low_to_high()
        .filter(|layer| matches!(layer.name, ConfigLayerSource::Project { .. }))
        .map(|layer| (layer.name.clone(), layer.config_folder()))
        .collect::<Vec<_>>();
    assert_eq!(
        projects,
        vec![(
            ConfigLayerSource::Project {
                dot_codex_folder: fixture.repo.join(".codex")
            },
            Some(fixture.repo.join(".codex")),
        )]
    );
    Ok(())
}

#[tokio::test]
async fn invalid_inherited_config_reports_main_source() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(&fixture.repo, "[mcp_servers.linear")?;
    for error in [
        load_config_layers_state(
            &TestFileSystem,
            fixture.home.as_path(),
            Some(fixture.cwd.clone()),
            &[],
            fixture.overrides.clone(),
            &NoopThreadConfigLoader,
        )
        .await
        .expect_err("invalid inherited config"),
        load_local_config_layers_with_overrides(
            &TestFileSystem,
            fixture.home.as_path(),
            &fixture.cwd,
            &fixture.overrides,
        )
        .await
        .expect_err("invalid inherited config"),
    ] {
        assert!(
            error.to_string().contains(
                &fixture
                    .repo
                    .join(".codex/config.toml")
                    .display()
                    .to_string()
            )
        );
    }
    Ok(())
}

#[tokio::test]
async fn strict_validation_checks_inherited_mcp_only() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    Fixture::write_project(
        &fixture.repo,
        "unrelated_unknown_key = 1\n[mcp_servers.linear]\nurl = 'https://main.example/mcp'",
    )?;
    let options = ConfigLoadOptions {
        loader_overrides: fixture.overrides.clone(),
        strict_config: true,
        ..Default::default()
    };
    let canonical = load_config_layers_state(
        &TestFileSystem,
        fixture.home.as_path(),
        Some(fixture.cwd.clone()),
        &[],
        options.clone(),
        &NoopThreadConfigLoader,
    )
    .await?;
    assert_eq!(
        canonical.effective_config().get("unrelated_unknown_key"),
        None
    );
    Fixture::write_project(&fixture.cwd, "[mcp_servers.linear]\nenabled = false")?;
    let canonical = load_config_layers_state(
        &TestFileSystem,
        fixture.home.as_path(),
        Some(fixture.cwd.clone()),
        &[],
        options.clone(),
        &NoopThreadConfigLoader,
    )
    .await?;
    assert_eq!(
        canonical.effective_config().get("mcp_servers").cloned(),
        Some(
            toml::from_str::<TomlValue>(
                "[mcp_servers.linear]\nurl = 'https://main.example/mcp'\nenabled = false"
            )?["mcp_servers"]
                .clone()
        )
    );
    Fixture::write_project(
        &fixture.repo,
        "[mcp_servers.linear]\nurl = 'https://main.example/mcp'\nunknown_mcp_field = 1",
    )?;
    let error = load_config_layers_state(
        &TestFileSystem,
        fixture.home.as_path(),
        Some(fixture.cwd.clone()),
        &[],
        options,
        &NoopThreadConfigLoader,
    )
    .await
    .expect_err("unknown inherited MCP field");
    assert!(error.to_string().contains("unknown_mcp_field"), "{error}");
    Ok(())
}
