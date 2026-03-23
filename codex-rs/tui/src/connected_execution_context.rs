use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codex_app_server_protocol::ExecutionContext;
use serde::Deserialize;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::warn;
use url::Host;
use url::Url;

#[derive(Clone, Debug)]
pub(crate) struct ExecutionContextCache {
    enabled: bool,
    entries: Arc<Mutex<HashMap<PathBuf, Option<ExecutionContext>>>>,
}

impl ExecutionContextCache {
    #[cfg(test)]
    pub(crate) fn for_local_server() -> Self {
        Self {
            enabled: true,
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub(crate) async fn insert_for_test(
        &self,
        cwd: PathBuf,
        execution_context: Option<ExecutionContext>,
    ) {
        self.entries.lock().await.insert(cwd, execution_context);
    }

    pub(crate) fn for_url(url: &str) -> Self {
        Self {
            enabled: app_server_url_is_local(url),
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn execution_context_for_cwd_with_feedback<F>(
        &self,
        cwd: &Path,
        on_nix_command_start: F,
    ) -> Option<ExecutionContext>
    where
        F: FnMut(),
    {
        if !self.enabled {
            return None;
        }

        let cache_key = cwd.to_path_buf();
        {
            let guard = self.entries.lock().await;
            if let Some(cached) = guard.get(&cache_key) {
                return cached.clone();
            }
        }

        if !tokio::fs::try_exists(cwd.join("flake.nix")).await.ok()? {
            let mut guard = self.entries.lock().await;
            guard.insert(cache_key, None);
            return None;
        }

        let mut on_nix_command_start = on_nix_command_start;
        on_nix_command_start();

        let execution_context = derive_execution_context(cwd).await;
        let mut guard = self.entries.lock().await;
        guard.insert(cache_key, execution_context.clone());
        execution_context
    }
}

async fn derive_execution_context(cwd: &Path) -> Option<ExecutionContext> {
    let output = match Command::new("nix")
        .args(["print-dev-env", "--json"])
        .current_dir(cwd)
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            warn!(
                "failed to derive execution context for {}: {err}",
                cwd.display()
            );
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        warn!(
            "nix print-dev-env failed for {} with status {}: {}",
            cwd.display(),
            output.status,
            stderr
        );
        return None;
    }

    match serde_json::from_slice::<NixPrintDevEnv>(&output.stdout) {
        Ok(dev_env) => Some(dev_env.into_execution_context()),
        Err(err) => {
            warn!(
                "failed to parse nix print-dev-env output for {}: {err}",
                cwd.display()
            );
            None
        }
    }
}

fn app_server_url_is_local(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };

    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(addr)) => addr.is_loopback(),
        Some(Host::Ipv6(addr)) => addr.is_loopback(),
        None => false,
    }
}

#[derive(Debug, Deserialize)]
struct NixPrintDevEnv {
    variables: BTreeMap<String, NixVariable>,
}

impl NixPrintDevEnv {
    fn into_execution_context(self) -> ExecutionContext {
        let env = self
            .variables
            .into_iter()
            .filter_map(|(key, variable)| {
                matches!(variable.kind, NixVariableKind::Exported).then_some((key, variable.value))
            })
            .collect();
        ExecutionContext { env }
    }
}

#[derive(Debug, Deserialize)]
struct NixVariable {
    #[serde(rename = "type")]
    kind: NixVariableKind,
    value: String,
}

#[derive(Debug, Deserialize)]
enum NixVariableKind {
    #[serde(rename = "exported")]
    Exported,
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn execution_context_keeps_only_exported_nix_variables() {
        let dev_env: NixPrintDevEnv = serde_json::from_value(json!({
            "variables": {
                "PATH": { "type": "exported", "value": "/nix/bin" },
                "NIX_LDFLAGS": { "type": "exported", "value": "-L/nix/store/lib" },
                "BASH": { "type": "var", "value": "/nix/store/bash/bin/bash" }
            }
        }))
        .expect("deserialize nix print-dev-env output");

        assert_eq!(
            dev_env.into_execution_context(),
            ExecutionContext {
                env: BTreeMap::from([
                    ("NIX_LDFLAGS".to_string(), "-L/nix/store/lib".to_string()),
                    ("PATH".to_string(), "/nix/bin".to_string()),
                ]),
            }
        );
    }

    #[test]
    fn local_server_detection_accepts_loopback_hosts() {
        assert!(app_server_url_is_local("ws://127.0.0.1:4222"));
        assert!(app_server_url_is_local("ws://localhost:4222"));
        assert!(app_server_url_is_local("ws://[::1]:4222"));
        assert!(!app_server_url_is_local("ws://example.com:4222"));
    }
}
