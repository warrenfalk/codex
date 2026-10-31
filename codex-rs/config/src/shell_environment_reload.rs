use crate::ConfigLayerStack;
use crate::ConfigLoadOptions;
use crate::NoopThreadConfigLoader;
use crate::loader::load_config_layers_state;
use crate::types::ShellEnvironmentPolicyToml;
use codex_file_system::ExecutorFileSystem;
use codex_protocol::config_types::ShellEnvironmentPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
#[path = "shell_environment_reload_tests.rs"]
mod tests;

/// Retains the original loading rules and last observed policy for turn-boundary reloads.
/// Other session settings and host-provided thread layers remain unchanged.
#[derive(Clone, Default)]
pub struct ShellEnvironmentPolicyReload {
    options: Option<Arc<ConfigLoadOptions>>,
    observed: ShellEnvironmentPolicy,
}

impl std::fmt::Debug for ShellEnvironmentPolicyReload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellEnvironmentPolicyReload")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ShellEnvironmentPolicyReload {
    fn eq(&self, other: &Self) -> bool {
        self.observed == other.observed
            && match (&self.options, &other.options) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                (Some(_), None) | (None, Some(_)) => false,
            }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ShellConfig {
    shell_environment_policy: ShellEnvironmentPolicyToml,
}

impl ShellEnvironmentPolicyReload {
    pub fn new(options: ConfigLoadOptions, observed: ShellEnvironmentPolicy) -> Self {
        Self {
            options: Some(Arc::new(options)),
            observed,
        }
    }

    /// Reloads through normal discovery, filtering, and precedence rules. Returns a policy
    /// only when file-backed settings changed, preserving programmatic runtime overrides
    /// while their underlying configuration is unchanged.
    pub async fn reload(
        &mut self,
        fs: &dyn ExecutorFileSystem,
        codex_home: &Path,
        cwd: AbsolutePathBuf,
        session_layers: &ConfigLayerStack,
    ) -> std::io::Result<Option<ShellEnvironmentPolicy>> {
        let Some(options) = &self.options else {
            return Ok(None);
        };
        let mut session = toml::Value::Table(Default::default());
        for layer in session_layers.layers_low_to_high() {
            if matches!(layer.name, crate::ConfigLayerSource::SessionFlags) {
                crate::merge_toml_values(&mut session, &layer.config);
            }
        }
        let overrides = session
            .as_table()
            .into_iter()
            .flatten()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        let layers = load_config_layers_state(
            fs,
            codex_home,
            Some(cwd),
            &overrides,
            options.as_ref().clone(),
            &NoopThreadConfigLoader,
        )
        .await?;
        let config: ShellConfig = layers
            .effective_config()
            .try_into()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let policy = ShellEnvironmentPolicy::from(config.shell_environment_policy);
        if policy == self.observed {
            return Ok(None);
        }
        self.observed = policy.clone();
        Ok(Some(policy))
    }
}
