//! Main-checkout MCP defaults for validated linked worktrees.
//!
//! Keep the original source paths without turning those paths into roots for
//! discovery of other project artifacts. Worktree layers override these defaults.

use super::DiscoveredProjectLayer;
use super::ProjectTrustContext;
use super::validate_config_toml_strictly;
use crate::CONFIG_TOML_FILE;
use crate::merge_toml_values;
use codex_file_system::ExecutorFileSystem;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use std::io;
use toml::Value as TomlValue;

pub(super) async fn inherit_main_checkout_mcp(
    fs: &dyn ExecutorFileSystem,
    dirs: &[AbsolutePathBuf],
    worktree_layers: Vec<DiscoveredProjectLayer>,
    trust_context: &ProjectTrustContext,
    codex_home: &AbsolutePathBuf,
    strict_config: bool,
) -> io::Result<Vec<DiscoveredProjectLayer>> {
    if trust_context.repo_root.is_none() || trust_context.checkout_root == trust_context.repo_root {
        return Ok(worktree_layers);
    }
    let mut layers = Vec::new();
    let codex_home_uri = PathUri::from_abs_path(codex_home);
    let canonical_home = fs
        .canonicalize(&codex_home_uri, /*sandbox*/ None)
        .await
        .ok();
    for dir in dirs {
        if !trust_context.decision_for_dir(dir).is_trusted() {
            continue;
        }
        let Some(folder) = trust_context.root_checkout_hooks_folder_for_dir(dir) else {
            continue;
        };
        if folder == *codex_home
            || fs
                .canonicalize(&PathUri::from_abs_path(&folder), /*sandbox*/ None)
                .await
                .ok()
                .zip(canonical_home.as_ref())
                .is_some_and(|(folder, home)| &folder == home)
        {
            continue;
        }

        let file = folder.join(CONFIG_TOML_FILE);
        let contents = match fs
            .read_file_text(
                &PathUri::from_abs_path(&file),
                Default::default(),
                /*sandbox*/ None,
            )
            .await
        {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "Failed to read inherited MCP config {}: {error}",
                        file.display()
                    ),
                ));
            }
        };
        let mut config: TomlValue = toml::from_str(&contents).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Error parsing inherited MCP config {}: {error}",
                    file.display()
                ),
            )
        })?;
        let Some(servers) = config
            .as_table_mut()
            .and_then(|table| table.remove("mcp_servers"))
        else {
            continue;
        };
        let config = TomlValue::Table(toml::map::Map::from_iter([("mcp_servers".into(), servers)]));
        layers.push(DiscoveredProjectLayer {
            dot_codex_folder: folder,
            config,
            disabled_reason: None,
            hooks_config_folder_override: None,
            load_root_checkout_hooks: false,
            mcp_only: true,
        });
    }
    layers.extend(worktree_layers);

    // The general config merge is recursive. When a higher layer switches an
    // inherited server's transport, discard the displaced transport's fields
    // from the defaults so the effective server remains a valid definition.
    for index in 0..layers.len() {
        let (defaults, overlays) = layers.split_at_mut(index);
        let overlay = &overlays[0];
        if overlay.disabled_reason.is_some() {
            continue;
        }
        let Some(servers) = overlay
            .config
            .get("mcp_servers")
            .and_then(TomlValue::as_table)
        else {
            continue;
        };
        for (name, server) in servers {
            let Some(server) = server.as_table() else {
                continue;
            };
            let (active_transport, displaced_fields): (&str, &[&str]) =
                match (server.contains_key("command"), server.contains_key("url")) {
                    (true, false) => (
                        "command",
                        &[
                            "url",
                            "bearer_token",
                            "bearer_token_env_var",
                            "http_headers",
                            "env_http_headers",
                            "http_headers_helper",
                            "oauth",
                            "oauth_resource",
                            "auth",
                        ],
                    ),
                    (false, true) => ("url", &["command", "args", "env", "env_vars", "cwd"]),
                    (true, true) | (false, false) => continue,
                };
            for layer in defaults.iter_mut().filter(|layer| layer.mcp_only) {
                if let Some(default) = layer
                    .config
                    .get_mut("mcp_servers")
                    .and_then(|servers| servers.get_mut(name))
                    .and_then(TomlValue::as_table_mut)
                    && !default.contains_key(active_transport)
                {
                    for field in displaced_fields {
                        default.remove(*field);
                    }
                }
            }
        }
        if strict_config {
            let mut effective_mcp = TomlValue::Table(toml::map::Map::new());
            for layer in defaults
                .iter()
                .chain(std::iter::once(overlay))
                .filter(|layer| layer.disabled_reason.is_none())
            {
                merge_toml_values(&mut effective_mcp, &layer.config);
            }
            let mut validation_config = overlay.config.clone();
            if let Some(servers) = effective_mcp.get("mcp_servers")
                && let Some(table) = validation_config.as_table_mut()
            {
                table.insert("mcp_servers".into(), servers.clone());
            }
            let file = overlay.dot_codex_folder.join(CONFIG_TOML_FILE);
            let contents = fs
                .read_file_text(
                    &PathUri::from_abs_path(&file),
                    Default::default(),
                    /*sandbox*/ None,
                )
                .await?;
            validate_config_toml_strictly(
                file.as_path(),
                &contents,
                &validation_config,
                overlay.dot_codex_folder.as_path(),
            )?;
        }
    }
    Ok(layers)
}

#[cfg(test)]
#[path = "worktree_mcp_tests.rs"]
mod tests;
