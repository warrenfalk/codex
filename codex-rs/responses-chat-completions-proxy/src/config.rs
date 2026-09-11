use clap::Parser;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

pub struct ProxyConfig {
    pub listen_address: IpAddr,
    pub port: u16,
    pub upstream_url: String,
    pub upstream_model: Option<String>,
    pub upstream_bearer: Option<String>,
    pub forward_inbound_authorization: bool,
    pub server_info: Option<PathBuf>,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub capabilities: BackendCapabilities,
}

#[derive(Clone, Copy, Debug)]
pub struct BackendCapabilities {
    pub developer_role: bool,
    pub image_input: bool,
    pub parallel_tool_calls: bool,
    pub prompt_cache_key: PromptCacheKeyPolicy,
    pub reasoning_content: ReasoningContentPolicy,
    pub reasoning_effort: bool,
    pub streaming: bool,
    pub structured_output: bool,
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self {
            developer_role: false,
            image_input: false,
            parallel_tool_calls: false,
            prompt_cache_key: PromptCacheKeyPolicy::Omit,
            reasoning_content: ReasoningContentPolicy::Unsupported,
            reasoning_effort: false,
            streaming: true,
            structured_output: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptCacheKeyPolicy {
    Omit,
    Forward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasoningContentPolicy {
    Unsupported,
    Plaintext,
}

#[derive(Debug, Parser)]
#[command(about = "Expose a Codex-compatible Responses API over a Chat Completions backend")]
pub struct Args {
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    pub listen_address: IpAddr,

    #[arg(long, default_value_t = 0)]
    pub port: u16,

    #[arg(long)]
    pub upstream_url: String,

    #[arg(long)]
    pub upstream_model: Option<String>,

    #[arg(long)]
    pub upstream_api_key_env: Option<String>,

    #[arg(long)]
    pub forward_inbound_authorization: bool,

    #[arg(long)]
    pub disable_upstream_streaming: bool,

    #[arg(long)]
    pub server_info: Option<PathBuf>,

    #[arg(long, default_value_t = 120)]
    pub request_timeout_seconds: u64,

    #[arg(long, default_value_t = 60)]
    pub stream_idle_timeout_seconds: u64,

    #[arg(long)]
    pub supports_developer_role: bool,

    #[arg(long)]
    pub supports_image_input: bool,

    #[arg(long)]
    pub supports_parallel_tool_calls: bool,

    #[arg(long)]
    pub forwards_prompt_cache_key: bool,

    #[arg(long)]
    pub supports_reasoning_content: bool,

    #[arg(long)]
    pub supports_reasoning_effort: bool,

    #[arg(long)]
    pub supports_structured_output: bool,
}

impl Args {
    pub fn into_config(self) -> anyhow::Result<ProxyConfig> {
        if self.upstream_api_key_env.is_some() && self.forward_inbound_authorization {
            anyhow::bail!(
                "--upstream-api-key-env and --forward-inbound-authorization are mutually exclusive"
            );
        }
        let upstream_bearer = self
            .upstream_api_key_env
            .as_deref()
            .map(std::env::var)
            .transpose()?
            .filter(|value| !value.is_empty());
        Ok(ProxyConfig {
            listen_address: self.listen_address,
            port: self.port,
            upstream_url: self.upstream_url,
            upstream_model: self.upstream_model,
            upstream_bearer,
            forward_inbound_authorization: self.forward_inbound_authorization,
            server_info: self.server_info,
            request_timeout: Duration::from_secs(self.request_timeout_seconds),
            stream_idle_timeout: Duration::from_secs(self.stream_idle_timeout_seconds),
            capabilities: BackendCapabilities {
                developer_role: self.supports_developer_role,
                image_input: self.supports_image_input,
                parallel_tool_calls: self.supports_parallel_tool_calls,
                prompt_cache_key: if self.forwards_prompt_cache_key {
                    PromptCacheKeyPolicy::Forward
                } else {
                    PromptCacheKeyPolicy::Omit
                },
                reasoning_content: if self.supports_reasoning_content {
                    ReasoningContentPolicy::Plaintext
                } else {
                    ReasoningContentPolicy::Unsupported
                },
                reasoning_effort: self.supports_reasoning_effort,
                streaming: !self.disable_upstream_streaming,
                structured_output: self.supports_structured_output,
            },
        })
    }
}
