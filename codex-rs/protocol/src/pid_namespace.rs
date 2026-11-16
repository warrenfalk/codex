use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use crate::models::PermissionProfile;
use crate::permissions::FileSystemSandboxPolicy;
use crate::permissions::NetworkSandboxPolicy;

/// PID namespace used by the Linux bubblewrap sandbox. Other platforms ignore it.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum PidNamespace {
    /// Create a fresh namespace for each sandbox invocation.
    #[default]
    Isolated,
    /// Share the executor's existing namespace, including any enclosing container.
    Host,
}

impl PidNamespace {
    pub fn is_isolated(&self) -> bool {
        matches!(self, Self::Isolated)
    }
}

impl PermissionProfile {
    pub fn pid_namespace(&self) -> PidNamespace {
        match self {
            Self::Managed { pid_namespace, .. } => *pid_namespace,
            // Keep the default if a non-managed profile needs a bubblewrap
            // wrapper to enforce managed proxy networking.
            Self::Disabled | Self::External { .. } => PidNamespace::Isolated,
        }
    }

    pub fn with_pid_namespace(mut self, namespace: PidNamespace) -> Self {
        if let Self::Managed { pid_namespace, .. } = &mut self {
            *pid_namespace = namespace;
        }
        self
    }

    /// Replace filesystem/network projections while preserving the other permissions.
    pub fn with_runtime_permissions(
        &self,
        file_system: &FileSystemSandboxPolicy,
        network: NetworkSandboxPolicy,
    ) -> Self {
        Self::from_runtime_permissions_with_enforcement(self.enforcement(), file_system, network)
            .with_pid_namespace(self.pid_namespace())
    }
}

#[cfg(test)]
#[path = "pid_namespace_tests.rs"]
mod tests;
