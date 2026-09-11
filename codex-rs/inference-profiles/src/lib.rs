use codex_model_provider_info::ModelProviderInfo;
use codex_responses_chat_completions_proxy::BackendCapabilities;
use codex_responses_chat_completions_proxy::EmbeddedProxy;
use codex_responses_chat_completions_proxy::PromptCacheKeyPolicy;
use codex_responses_chat_completions_proxy::ProxyConfig;
use codex_responses_chat_completions_proxy::ReasoningContentPolicy;
use codex_responses_chat_completions_proxy::start_embedded;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

pub const KIMI_K3_MODEL_ID: &str = "kimi-k3";
pub const KIMI_PROVIDER_ID: &str = "kimi";

const KIMI_CHAT_COMPLETIONS_URL: &str = "https://api.moonshot.ai/v1/chat/completions";
const KIMI_CREDENTIAL_FILE: &str = ".kimi-codex";
const KIMI_CREDENTIAL_VARIABLE: &str = "API_KEY";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InferenceProfile {
    KimiK3,
}

#[derive(Debug)]
pub struct ResolvedProvider {
    pub id: String,
    pub info: ModelProviderInfo,
    pub profile: Option<InferenceProfile>,
}

#[derive(Debug, thiserror::Error)]
pub enum InferenceProfileError {
    #[error(
        "Kimi K3 requires a Kimi Open Platform API key. Create ~/.kimi-codex with a non-empty API_KEY=<key> entry."
    )]
    MissingKimiCredential,
    #[error("failed to read ~/.kimi-codex for Kimi K3: {0}")]
    InvalidKimiCredentialFile(String),
    #[error("failed to start the embedded Kimi Responses adapter: {0}")]
    AdapterStartup(String),
    #[error("the inference profile runtime lock is poisoned")]
    RuntimeLockPoisoned,
}

#[derive(Debug)]
pub struct InferenceProfileRuntime {
    kimi_credential_path: Option<PathBuf>,
    kimi_proxy: Mutex<Option<EmbeddedProxy>>,
}

impl Default for InferenceProfileRuntime {
    fn default() -> Self {
        Self {
            kimi_credential_path: dirs::home_dir().map(|home| home.join(KIMI_CREDENTIAL_FILE)),
            kimi_proxy: Mutex::new(None),
        }
    }
}

impl InferenceProfileRuntime {
    /// Creates a runtime that loads the Kimi credential from an explicit file.
    pub fn with_kimi_credential_file(path: impl Into<PathBuf>) -> Self {
        Self {
            kimi_credential_path: Some(path.into()),
            kimi_proxy: Mutex::new(None),
        }
    }

    pub fn resolve_provider(
        &self,
        model: &str,
        baseline_provider_id: &str,
        baseline_provider: &ModelProviderInfo,
    ) -> Result<ResolvedProvider, InferenceProfileError> {
        match inference_profile_for_model(model) {
            Some(InferenceProfile::KimiK3) => self.resolve_kimi_k3(),
            None => Ok(ResolvedProvider {
                id: baseline_provider_id.to_string(),
                info: baseline_provider.clone(),
                profile: None,
            }),
        }
    }

    fn resolve_kimi_k3(&self) -> Result<ResolvedProvider, InferenceProfileError> {
        let mut proxy = self
            .kimi_proxy
            .lock()
            .map_err(|_| InferenceProfileError::RuntimeLockPoisoned)?;
        if proxy.is_none() {
            let credential_path = self
                .kimi_credential_path
                .as_deref()
                .ok_or(InferenceProfileError::MissingKimiCredential)?;
            let api_key = read_kimi_api_key(credential_path)?;
            *proxy = Some(
                start_embedded(kimi_proxy_config(api_key))
                    .map_err(|error| InferenceProfileError::AdapterStartup(error.to_string()))?,
            );
        }
        let base_url = proxy.as_ref().map(EmbeddedProxy::base_url).ok_or_else(|| {
            InferenceProfileError::AdapterStartup(
                "adapter did not remain available after startup".to_string(),
            )
        })?;
        Ok(ResolvedProvider {
            id: KIMI_PROVIDER_ID.to_string(),
            info: ModelProviderInfo {
                name: "Kimi K3 via embedded Chat Completions adapter".to_string(),
                base_url: Some(base_url),
                stream_idle_timeout_ms: Some(300_000),
                ..ModelProviderInfo::default()
            },
            profile: Some(InferenceProfile::KimiK3),
        })
    }
}

pub fn inference_profile_for_model(model: &str) -> Option<InferenceProfile> {
    match model {
        KIMI_K3_MODEL_ID => Some(InferenceProfile::KimiK3),
        _ => None,
    }
}

pub fn effective_provider_id<'a>(model: &str, baseline_provider_id: &'a str) -> &'a str {
    match inference_profile_for_model(model) {
        Some(InferenceProfile::KimiK3) => KIMI_PROVIDER_ID,
        None => baseline_provider_id,
    }
}

fn read_kimi_api_key(path: &Path) -> Result<String, InferenceProfileError> {
    let entries = dotenvy::from_path_iter(path).map_err(|error| {
        if error.not_found() {
            InferenceProfileError::MissingKimiCredential
        } else {
            InferenceProfileError::InvalidKimiCredentialFile(error.to_string())
        }
    })?;
    for entry in entries {
        let (name, value) = entry
            .map_err(|error| InferenceProfileError::InvalidKimiCredentialFile(error.to_string()))?;
        if name == KIMI_CREDENTIAL_VARIABLE && !value.trim().is_empty() {
            return Ok(value);
        }
    }
    Err(InferenceProfileError::MissingKimiCredential)
}

fn kimi_proxy_config(api_key: String) -> ProxyConfig {
    ProxyConfig {
        listen_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        upstream_url: KIMI_CHAT_COMPLETIONS_URL.to_string(),
        upstream_model: Some(KIMI_K3_MODEL_ID.to_string()),
        upstream_bearer: Some(api_key),
        forward_inbound_authorization: false,
        server_info: None,
        request_timeout: Duration::from_secs(2 * 60 * 60),
        stream_idle_timeout: Duration::from_secs(5 * 60),
        capabilities: BackendCapabilities {
            developer_role: false,
            image_input: true,
            parallel_tool_calls: true,
            prompt_cache_key: PromptCacheKeyPolicy::Forward,
            reasoning_content: ReasoningContentPolicy::Plaintext,
            reasoning_effort: true,
            streaming: true,
            structured_output: true,
        },
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
