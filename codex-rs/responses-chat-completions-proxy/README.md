# Codex Responses-to-Chat-Completions proxy

This proxy lets an unmodified Codex client use a backend that implements
`POST /v1/chat/completions` but not `POST /v1/responses`. Codex continues to speak the Responses
protocol. The proxy translates full conversation history and tools into Chat Completions, then
turns the Chat result back into the Responses SSE events Codex consumes.

This is a compatibility profile, not a general implementation of every Responses feature. Read
the capability and unsupported-feature sections before pointing an existing Codex profile at it.

## Build and launch

From `codex-rs`:

```bash
cargo build -p codex-responses-chat-completions-proxy

export CHAT_BACKEND_API_KEY='...'
./target/debug/codex-responses-chat-completions-proxy \
  --port 60002 \
  --upstream-url 'https://backend.example/v1/chat/completions' \
  --upstream-model 'backend-model-name' \
  --upstream-api-key-env CHAT_BACKEND_API_KEY \
  --supports-parallel-tool-calls \
  --forwards-prompt-cache-key \
  --supports-reasoning-content \
  --supports-reasoning-effort \
  --supports-structured-output
```

The process logs its bound address to stderr. `--port 0`, the default, asks the operating system
for an ephemeral port; use `--server-info /path/to/info.json` to discover that port. The file is a
single JSON line containing `port` and `pid`.

Stop the process with `SIGINT`/Ctrl-C. `GET /healthz` returns HTTP 204 while the process is serving.

The default upstream mode requests Chat SSE. Add `--disable-upstream-streaming` for a backend that
only returns buffered Chat completion JSON. Codex still receives a valid Responses SSE stream, but
without incremental backend latency benefits.

## Codex configuration

Use a provider name other than `OpenAI`. That distinction is functional: it keeps Codex on the
stateless, third-party Responses path with HTTP SSE, full history, and local compaction.

```toml
model = "my-chat-backend-model"
model_provider = "chat-proxy"

# Set this to the backend model's real context window. The fallback below is only an example.
model_context_window = 32768

# V1 does not implement server-side web search.
web_search = "disabled"

[model_providers.chat-proxy]
name = "chat-proxy"
base_url = "http://127.0.0.1:60002/v1"
wire_api = "responses"
supports_websockets = false
request_max_retries = 2
stream_max_retries = 2
stream_idle_timeout_ms = 60000
```

Codex v0.146 sends a reasoning request envelope and asks for encrypted reasoning content for every
model. The proxy accepts the encrypted-content include as a compatibility hint but never fabricates
an encrypted payload. A non-reasoning backend should use a `model_catalog_json` entry with no
default reasoning effort and `supports_reasoning_summary_parameter = false`; other advertised model
capabilities must likewise match both the proxy flags and the real backend. In particular:

- set `supports_reasoning_summary_parameter` true only for a profile deliberately using
  `--supports-reasoning-content`, keep its default summary set to `none`, and keep
  `support_verbosity` false;
- keep `supports_search_tool` and Responses Lite false;
- set `supports_parallel_tool_calls` true only with `--supports-parallel-tool-calls` and a backend
  that actually accepts parallel Chat tool calls;
- select the freeform apply-patch tool only when ordinary Chat function calling is reliable;
- set the real context window and input modalities rather than copying the example values.

Codex must remain configured with `wire_api = "responses"`. Do not add a Chat wire mode to Codex.

### Built-in Kimi K3 profile

This fork also includes a zero-configuration process profile for Kimi K3. Put the credential in:

```dotenv
# ~/.kimi-codex
API_KEY=your-kimi-api-key
```

Then select `Kimi K3` in the model picker or run Codex with `-m kimi-k3`. Codex starts a
session-owned loopback proxy automatically and targets
`https://api.moonshot.ai/v1/chat/completions` with upstream model `kimi-k3`. The profile enables
the backend behaviors already validated for Kimi: image input, parallel calls, structured output,
`max` reasoning effort, plaintext `reasoning_content`, and `prompt_cache_key` forwarding. It does
not enable hosted Responses web search or image generation, and it does not impose a completion
token limit.

The file must contain a non-empty `API_KEY`. Missing or invalid credentials reject model selection
without changing the active thread settings; there is no fallback to OpenAI. Protect this file as
a secret (mode `0600` is recommended on Unix-like systems).

## Authentication

Proxy-owned authentication is the preferred mode:

```text
--upstream-api-key-env CHAT_BACKEND_API_KEY
```

The argument names an environment variable; the secret itself is not placed in the command line.
In this mode Codex sends no upstream credential to the local listener. The process uses the same
pre-main process-hardening policy as the strict Responses proxy, but an environment variable is
still visible to processes with sufficient privilege. Run the proxy and protect its environment
accordingly.

Inbound forwarding is an explicit alternative:

```text
--forward-inbound-authorization
```

With that flag, the proxy forwards Codex's incoming `Authorization` header. Configure `env_key` on
the Codex provider if you use this mode. It cannot be combined with `--upstream-api-key-env`.
Without either option, no authorization header is sent upstream, which is useful for an unauthenticated
loopback backend.

The upstream URL must use HTTP or HTTPS and must not embed credentials.

## Supported V1 behavior

| Behavior | Support and requirements |
| --- | --- |
| Text input and streamed text output | Always supported. |
| Buffered Chat backend | Supported with `--disable-upstream-streaming`, or when a backend ignores `stream: true` and returns JSON. |
| Function tools | Translated to externally tagged Chat functions. Schemas, descriptions, strictness, order, arguments, and call IDs are preserved. |
| Namespace tools | Flattened to deterministic Chat-safe names and reverse-mapped on output. |
| Custom/freeform tools | Wrapped as strict synthetic functions with one string field named `input`; the syntax definition is included with a hard size limit. |
| Parallel tool calls | Requires `--supports-parallel-tool-calls` and matching Codex model metadata. |
| Call/output history | Full history is validated, grouped into assistant/tool messages, and correlated by call ID. |
| Structured output | Requires `--supports-structured-output`; Responses `text.format` becomes Chat `response_format.json_schema`. |
| User image input | Requires `--supports-image-input`; image URLs and declared detail are passed as Chat image content. |
| Developer role | Instructions use `system` by default; `--supports-developer-role` selects `developer`. |
| Reasoning effort request | `--supports-reasoning-effort` forwards the Responses effort string as Chat `reasoning_effort`. |
| Plaintext reasoning | `--supports-reasoning-content` maps Chat `reasoning_content` into Responses reasoning events/items and restores it to Chat assistant history for tool continuations. It does not provide encrypted-reasoning or summary parity. |
| Prompt caching | `--forwards-prompt-cache-key` forwards Responses `prompt_cache_key` unchanged. The default omits it because this field is not portable across Chat-compatible backends. |
| Usage | Prompt, completion, total, cached-input, and reasoning-output token counts are mapped when the backend supplies them. |
| Local compaction | Supported. Compaction is another full-history Responses request translated to Chat; no `/responses/compact` endpoint is used. |
| Cancellation | Dropping the Codex response drops the upstream request body promptly. |

Every successful stream ends with `response.completed`, including tool-call responses. Chat
`finish_reason=length` becomes `response.incomplete`; content filtering, malformed chunks, idle
timeouts, and midstream transport failures become a failed Responses stream. An upstream HTTP
error keeps its status and is returned in a Responses-shaped error body. The proxy does not retry
model requests; Codex owns request and stream retry policy.

## Explicitly unsupported

The proxy rejects these instead of silently discarding model-visible information:

- Responses WebSocket transport and `previous_response_id`;
- encrypted reasoning, reasoning-summary replay, and plaintext-reasoning replay unless
  `--supports-reasoning-content` is enabled;
- remote/in-band compaction and compaction input items;
- Responses Lite, `additional_tools`, `tool_search`, and deferred tool loading;
- server-executed web search, image generation, code interpreter, and remote MCP built-ins;
- audio input, encrypted agent messages, image-bearing assistant messages, and structured/image
  tool outputs;
- Responses verbosity controls;
- multiple Chat choices and unknown Chat finish reasons;
- non-streaming inbound Responses requests.

Unknown Responses input-item types and unknown top-level request fields fail closed. Tool results
with missing, duplicate, orphaned, or type-mismatched call IDs are rejected before inference.

## Limits and security boundaries

- Inbound request bodies are capped at 64 MiB.
- Buffered upstream responses are capped at 64 MiB; upstream error bodies are capped at 64 KiB.
- Generated custom-tool descriptions are capped at 32,768 characters.
- The proxy does not log prompt bodies, tool outputs, cookies, or authorization values and has no
  full-body trace-dump option.
- Binding to a non-loopback address is possible but exposes an inference and tool-schema endpoint.
  Add an appropriate authenticated transport boundary before doing so.

## Validation

Run the complete proxy suite from `codex-rs`:

```bash
just test -p codex-responses-chat-completions-proxy
```

The suite covers pure translation, in-process HTTP behavior, cancellation, buffered and streamed
backends, and real `TestCodex` turns for parallel functions, freeform patching, structured output,
local compaction, retry after a failed stream, and plaintext-reasoning capture/replay across a tool
continuation.
