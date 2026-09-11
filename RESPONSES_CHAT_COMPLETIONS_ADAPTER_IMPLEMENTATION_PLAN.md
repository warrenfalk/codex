# Responses-to-Chat-Completions Adapter Implementation Plan

## Goal

Implement a standalone proxy that exposes the subset of `POST /v1/responses` required by Codex
while using an OpenAI-compatible `POST /v1/chat/completions` endpoint for model inference.

The first version should provide practical Codex agent compatibility:

- streamed and buffered assistant text
- ordinary function tools
- custom/freeform tools such as patch application
- multiple parallel tool calls
- replay of calls and tool outputs in later model requests
- structured final output when Codex supplies `text.format`
- text and image inputs when the selected backend supports them
- usage and error translation
- normal Codex retry, cancellation, and local-compaction behavior

Keep the existing Codex inference path Responses-only. Do not restore a Chat Completions variant to
`WireApi` or spread Chat-specific types through `codex-core`.

This plan is based on checkout `412e615b8269cbe0e281e6ebbd96cef0ad49e59f`. Revalidate the
request fields, `ResponseItem` variants, and consumed streaming events before implementation if the
checkout moves materially.

Before landing the fork-local feature, add `wf_features/responses-chat-completions-adapter.md` with
the final observable behavior, supported capability profile, configuration, failure behavior, and
validation expectations. Keep code layout and staged implementation detail in this plan rather than
the feature document.

## Architectural Decision

Create a new Rust crate and binary, tentatively:

```text
codex-rs/responses-chat-completions-proxy
codex-responses-chat-completions-proxy
```

The process boundary is the useful seam:

```text
Codex
  POST /v1/responses
        |
        v
Responses-to-Chat proxy
  decode Responses request
  compile item history into Chat messages
  translate tools and output controls
        |
        v
Chat-compatible backend
  POST /v1/chat/completions
        |
        v
Responses-to-Chat proxy
  aggregate Chat deltas
  synthesize Responses output items and terminal events
        |
        v
Codex consumes Responses SSE
```

Use a library-plus-thin-binary crate shape so request translation, stream translation, and the HTTP
service can be exercised in-process by integration tests.

Do not repurpose `codex-responses-api-proxy`. Its deliberately narrow behavior is to forward
Responses requests unchanged. Reuse its server, secret-handling, process-hardening, dump-redaction,
and shutdown patterns where useful, but preserve its existing contract.

## V1 Compatibility Profile

Run the proxy as a custom provider with a non-OpenAI name:

```toml
[model_providers.chat-proxy]
name = "chat-proxy"
base_url = "http://127.0.0.1:PORT/v1"
wire_api = "responses"
supports_websockets = false
```

This is an intentional part of the design. It gives V1 the simpler third-party-provider behavior:

- HTTP SSE rather than Responses-over-WebSocket
- full logical history on each request rather than `previous_response_id`
- no OpenAI-only concurrent-reasoning stream options
- no Codex-backend zstd request compression
- local Codex compaction rather than remote `/responses/compact` or in-band compaction items
- no requirement to reproduce first-party ChatGPT, Azure, moderation, or turn-state metadata

The proxy should accept correctness-neutral request metadata such as `client_metadata` and
unsupported service-tier hints. `prompt_cache_key` is omitted by default, but a backend dialect may
declare that it accepts the field and receive it unchanged. The proxy must reject a request with a
clear error when ignoring a field or item would discard model-visible information or falsely claim
a capability.

## Explicit V1 Non-Goals

Do not implement these in the first version:

- Responses-over-WebSocket or `previous_response_id`
- remote or encrypted compaction
- encrypted reasoning replay (plaintext Chat `reasoning_content` is handled by the later Kimi
  profile extension)
- OpenAI reasoning-summary parity
- Responses Lite and `additional_tools`
- server-side web search, image generation, code interpreter, or remote MCP execution
- first-party OpenAI, ChatGPT, Azure, rate-limit, moderation, or safety-buffering extensions
- multiple Chat completion choices; always request and consume one generation
- `/v1/models`; use explicit Codex model configuration and an upstream model override initially
- prompt-based imitation of tool calling for a backend that lacks native function calls

These are capability boundaries, not silent degradations. Unsupported requested features should
produce an actionable error or be disabled in the Codex model/provider configuration.

## Stage 0: Freeze the Compatibility Contract

Before building the proxy, turn the current protocol trace into a small executable V1 contract.

Record:

- the exact Codex `ResponsesApiRequest` fields and omission/null behavior
- every `ResponseItem` accepted as input for V1
- every tool shape Codex can send in the V1 profile
- the minimal output-event grammar Codex consumes
- required terminal behavior, especially `response.completed`
- call/output correlation and history normalization rules
- error and usage shapes consumed by Codex

Create golden fixtures for at least:

- final text without deltas
- streamed final text
- one function call and its later output
- several parallel calls and outputs
- one custom/freeform call
- text plus one or more calls in the same model response
- structured final output
- an incomplete response
- a failed response

Classify each requirement as one of:

- emitted by Codex and therefore accepted by the proxy
- required by Codex and therefore emitted by the proxy
- tolerated by Codex but not required from the proxy
- explicitly unsupported in V1

Use these fixtures as the invariant for subsequent stages rather than implementing against prose
alone.

## Stage 1: Crate, Process, and HTTP Surface

Add a new workspace crate with narrowly separated modules for:

- inbound Responses request decoding
- Responses-item-to-Chat-message compilation
- tool-schema and tool-name translation
- Chat backend request construction
- Chat SSE decoding and accumulation
- Responses event synthesis
- HTTP error mapping
- server configuration and secret handling

The binary should initially expose:

- `POST /v1/responses`
- an optional loopback-only shutdown endpoint for tests and local operation
- optional server-info and redacted trace-dump paths following the existing proxy's conventions

Configuration should cover:

- listen address and port, defaulting to loopback
- upstream `/v1/chat/completions` URL
- upstream bearer credential supplied without placing it in logs or command-line arguments
- optional fixed upstream model override
- request and stream idle timeouts
- declared backend capabilities
- optional redacted request/response dump directory

Reuse the repository HTTP client, SSE support, process hardening, and sensitive-header handling where
they fit. Do not add this subsystem to `codex-core` merely to reuse convenient internals.

## Stage 2: Inbound Responses Decoder

Define adapter-local inbound DTOs instead of making the existing outbound `ResponsesApiRequest`
serve both directions. Reuse shared protocol types only where doing so preserves unknown-field and
error behavior.

Retain each raw input item until its `type` discriminator has been checked. Do not deserialize an
unknown item directly into `ResponseItem::Other` and then silently lose its original contents.

V1 request handling should support:

- `model`
- `instructions`
- `input`
- `tools`
- `tool_choice`
- `parallel_tool_calls`
- optional `reasoning`
- `stream`
- `include`
- optional `service_tier`
- optional `prompt_cache_key`
- optional `text`
- optional `client_metadata`

Require the single-generation semantics Codex expects. Accept `stream: true`; a later compatibility
extension may accept non-streaming inbound requests, but it is not needed by current Codex.

Handle input items as follows:

- map supported message content without loss
- map prior function/custom calls and their outputs
- preserve call IDs exactly
- map plaintext `agent_message` items through a documented text envelope if multi-agent support is
  included in the V1 profile
- reject encrypted agent messages, encrypted reasoning, and compaction items with a clear message
- reject unsupported built-in-tool items rather than dropping them
- reject image input when the configured backend does not declare image support

## Stage 3: History and Request Compilation

Implement a deterministic compiler from the ordered Responses item stream to Chat `messages[]`.

The compiler should:

1. Place `instructions` in one leading `developer` message, or `system` for dialects that do not
   support the developer role.
2. Convert user, developer, and assistant message items into Chat messages.
3. Combine an assistant text item and adjacent call items into one Chat assistant message when they
   belong to the same model step.
4. Group adjacent parallel calls into one assistant message containing several `tool_calls`.
5. Convert each function/custom output into a `role: "tool"` message with the original call ID.
6. Preserve the order of calls and results.
7. Detect orphaned results, missing calls, duplicate call IDs, and invalid role transitions before
   contacting the backend.

Add explicit policy for content that Chat cannot represent directly:

- structured tool output should remain structured only if the dialect supports it
- otherwise, textual content may be joined deterministically
- image-bearing tool results must be rejected unless the dialect has a tested representation
- opaque or encrypted content must never be exposed as ordinary visible prompt text

The HTTP profile can remain stateless because Codex sends full logical history. Do not add a
conversation database for V1.

## Stage 4: Tool Translation

### Function tools

Translate a Responses function tool:

```json
{"type":"function","name":"f","description":"...","strict":true,"parameters":{}}
```

into the Chat externally tagged form:

```json
{"type":"function","function":{"name":"f","description":"...","strict":true,"parameters":{}}}
```

Preserve schema, description, strictness, tool order, and `parallel_tool_calls` when supported by the
backend.

### Custom/freeform tools

Represent each custom tool as a synthetic Chat function with an `input` string property. Include
the declared freeform syntax and definition in the function description without allowing generated
metadata to exceed a bounded size.

When the backend calls the synthetic function, translate it back to a Responses
`custom_tool_call`, recovering the original name, namespace, raw input, and call ID.

### Namespaces and backend name restrictions

Maintain a deterministic request-local registry from original `(namespace, name)` pairs to safe Chat
function names. The generated names must avoid backend character, length, and collision problems.
Reverse-map every returned call and reject calls to names that were not offered.

The mapping must be deterministic from the union of the current tool set and historical call
identities present in the request. The proxy must not need cross-request state to encode calls that
are already in history, including calls to tools that are no longer currently offered.

### Deferred tools and built-ins

For V1:

- require deferred tool loading and `tool_search` to be disabled, or expand the complete known tool
  set before sending it to Chat
- reject `web_search` and other server-executed built-ins with an actionable capability error
- do not ask the model to imitate unavailable tools through prompt text

Support for client `tool_search` can be added later as a separate extension after the ordinary and
custom tool paths are proven.

## Stage 5: Chat Backend Client and Dialects

Define private Chat request, response, and stream types in the new crate. Do not introduce them into
the shared Codex protocol unless another real consumer emerges.

The baseline dialect should target the documented OpenAI-compatible Chat surface:

- `messages`
- externally tagged function tools
- `tool_choice`
- `parallel_tool_calls`
- `stream: true`
- `stream_options.include_usage` when supported
- `response_format` for structured output
- optional image content

Add a small backend-dialect abstraction for behaviors that are not actually standardized across
"OpenAI-compatible" providers:

- developer versus system role
- model-name override
- structured-output support and schema restrictions
- parallel-tool support
- image representation
- reasoning-effort request fields
- safe reasoning-summary response fields
- streamed usage placement
- provider-specific error bodies

Start with one strict baseline dialect and the actual first target backend. Do not build a broad
provider plugin framework before two concrete backends demonstrate a real variation point.

If the backend does not support streaming, the adapter may issue a non-streaming Chat request and
then emit a valid, buffered Responses SSE sequence. This loses incremental UI but remains protocol
correct.

## Stage 6: Responses Event Synthesis

Generate a fresh Responses response ID and deterministic per-response item IDs.

For assistant text:

1. emit `response.created`
2. emit `response.output_item.added` for a message when text begins
3. translate Chat content fragments into `response.output_text.delta`
4. emit one complete message in `response.output_item.done`

For Chat tool calls:

1. accumulate fragments by Chat tool-call index
2. capture the call ID and function name when first supplied
3. concatenate argument fragments exactly
4. validate the completed call against the request-local tool registry
5. emit one complete Responses function or custom call in `response.output_item.done`

Do not interleave deltas for multiple active Responses items. Stream assistant text while it is the
active item, then emit completed call items in a stable order.

After all output items, always emit:

```text
response.completed
```

Do this even when the result contains tool calls. A downstream Chat `[DONE]` marker is not a valid
replacement for the terminal event Codex requires.

Leave message `phase` absent in V1 unless a backend provides a tested signal. Codex already has
fallback behavior for phase-less messages, and guessing commentary versus final-answer semantics is
worse than omitting the optional field.

Map usage when present:

- Chat `prompt_tokens` to Responses `input_tokens`
- Chat `completion_tokens` to Responses `output_tokens`
- Chat `total_tokens` to Responses `total_tokens`
- cached and reasoning token details only when explicitly supplied by the dialect

## Stage 7: Terminal Reasons, Errors, and Cancellation

Translate Chat terminal reasons deliberately:

- `stop` to successful completion
- `tool_calls` to successful completion after emitting all calls
- `length` to `response.incomplete` with a bounded reason
- content-filter or policy termination to a documented failed or refusal behavior
- unknown terminal reasons to a clear adapter error

Before response streaming begins, map backend HTTP failures to an appropriate HTTP status and a
Responses-shaped error body. Once streaming has begun, emit `response.failed` when possible and
then close the stream so Codex observes the failure.

Do not retry after any model output has been received. Codex already owns sampling retries, and an
independent proxy retry after partial generation risks duplicating model actions. A bounded retry
before response headers may be considered only for transport failures known not to have reached the
backend.

Propagate client disconnect and cancellation to the backend request promptly. Apply bounded request,
connect, and stream-idle timeouts. Redact authorization, cookies, tool output, and prompt bodies from
ordinary logs; make full trace capture explicit and opt-in.

## Stage 8: Codex Configuration and Capability Control

Document a complete custom-provider example using:

- a non-OpenAI provider name
- the proxy's loopback `/v1` base URL
- `wire_api = "responses"`
- `supports_websockets = false`
- an explicit model name and correct context-window metadata
- model capability flags that do not advertise unsupported reasoning, verbosity, image, tool-search,
  or structured-output behavior

Support an upstream model override because the Codex-visible model slug and the backend model slug
will often differ.

Keep authentication ownership explicit. Prefer the hardened pattern where the local proxy owns the
upstream credential and Codex does not need to send it through the local HTTP request. If an inbound
authorization forwarding mode is added, make it opt-in and never combine it ambiguously with a
proxy-owned credential.

## Stage 9: Tests

### Unit tests

Cover pure translation boundaries with whole-object equality:

- instructions and message-role conversion
- text and image content conversion
- function tool wrapping
- deterministic namespace/name encoding and reverse mapping
- custom-tool wrapping and raw-input recovery
- adjacent and parallel call grouping
- function/custom output conversion
- structured-output conversion
- unsupported item and capability errors
- Chat text-delta accumulation
- interleaved Chat tool-call argument fragments by index
- terminal-reason, usage, and error conversion

### Proxy integration tests

Run the HTTP service in-process against a fake Chat Completions server. Verify exact upstream Chat
requests and exact downstream Responses event sequences.

Cover:

- buffered final text
- streamed final text split at arbitrary byte boundaries
- final text plus tool calls
- one ordinary function call
- several parallel function calls
- one custom/freeform call
- tool success, tool failure, and Codex's synthetic `aborted` output
- structured final output
- supported and rejected image input
- backend 4xx and 5xx responses
- early backend stream closure
- `length` and policy terminal reasons
- cancellation after streaming starts

### End-to-end Codex conformance tests

Configure an unmodified `TestCodex` or spawned Codex binary to use the proxy as a custom Responses
provider, with a scripted Chat backend behind it.

Prove that Codex can:

- complete an ordinary turn
- display streamed assistant text
- execute a function tool and send its result on the next sampling request
- execute multiple parallel tools
- execute a custom tool such as a patch operation
- recover from a model-visible tool error
- produce output matching an explicit final JSON schema
- compact locally and continue the conversation
- retry after a failed sampling stream without corrupting call/output history

Hold onto every mock so tests can inspect the exact `/v1/chat/completions` request rather than only
asserting user-visible output.

## Stage 10: Packaging and Documentation

Add the new crate to the Rust workspace and make its binary available through the development and
packaging paths actually used by this fork. Do not update Bazel metadata unless explicitly requested.

Document:

- launch and shutdown commands
- custom-provider configuration
- upstream URL, model mapping, and credential handling
- the V1 capability matrix
- explicit unsupported-feature errors
- security implications of opt-in trace dumps
- how to run the conformance suite

Add or update the required `wf_features/` document in the first commit that exposes usable behavior.

## Stage 11: Plaintext Reasoning and the Kimi K3 Inference Profile

Implement this as a provider-specific expansion after the generic V1 adapter is independently
complete:

1. Add typed adapter policies for plaintext Chat `reasoning_content` and Chat
   `prompt_cache_key`. Keep both disabled by default.
2. Convert streamed or buffered `reasoning_content` into a Responses reasoning output item with
   reasoning-text delta/done events. Preserve it in Codex history and restore it on the same Chat
   assistant message as the tool calls it preceded.
3. Accept Codex's unconditional encrypted-reasoning include as a compatibility hint for every
   profile, without fabricating an encrypted payload. Accept summary request controls only for the
   plaintext profile, and continue to reject encrypted or summary-bearing history.
4. Add a small inference-profile runtime outside `codex-core`. For model `kimi-k3`, it owns one
   embedded loopback adapter per session, reads `API_KEY` from `~/.kimi-codex`, and targets Kimi's
   documented Chat Completions endpoint and model.
5. Add trusted local Kimi model metadata to the model catalog and prevent a remote OpenAI catalog
   from hiding or overriding it. Make model and provider switching atomic and restore the baseline
   provider when switching away.
6. Bound provider-specific tools: preserve ordinary/namespaced client tools and image input, but do
   not advertise hosted Responses web search or image generation.
7. Validate with adapter conformance tests, profile/session tests, model-catalog and login-gating
   tests, a direct Kimi request, and an actual Codex tool continuation through the embedded adapter.

## Reviewable Landing Sequence

Keep each non-mechanical stage below the repository's change-size limits. A reasonable landing order
is:

1. **Foundation:** new crate, configuration, HTTP skeleton, private wire DTOs, and contract fixtures.
2. **Text path:** history compilation for messages, Chat client, text streaming, completion, usage,
   errors, and text-only end-to-end coverage.
3. **Agent path:** function/custom tools, call/result history, parallel calls, and agent-loop tests.
4. **Structured and multimodal path:** output schemas, declared image support, and capability errors.
5. **Hardening:** cancellation, timeout behavior, redacted tracing, packaging, feature documentation,
   and full V1 conformance coverage.

Land Stage 11 separately from those V1 stages so plaintext reasoning and Kimi-specific model/runtime
policy remain reviewable as a compatibility-profile expansion.

Do not combine speculative WebSocket, remote-compaction, built-in-tool, or reasoning work with the V1
landing.

## Validation Commands

After Rust changes, run the scoped tests from `codex-rs`:

```bash
just test -p codex-responses-chat-completions-proxy
```

Before finalizing, run the scoped fixer and formatter:

```bash
just fix -p codex-responses-chat-completions-proxy
just fmt
```

Do not rerun tests after `fix` or `fmt`, per repository policy.

Run the relevant Codex integration-test project if end-to-end fixtures live outside the new crate.
If shared `common`, `core`, or `protocol` code changes, run the scoped tests first and ask before the
complete workspace test suite.

If `flake.nix` or `codex-rs/default.nix` changes, run from the repository root:

```bash
nix build
```

Do not run Bazel maintenance workflows for this feature.

## Definition of Done

V1 is complete when an unmodified Codex binary, configured with the custom provider, can pass the
declared compatibility suite against a scripted Chat backend and a real target Chat-compatible
provider.

Specifically:

- ordinary and streamed text turns work
- ordinary, custom, and parallel tool calls round-trip correctly
- call IDs and tool outputs survive subsequent requests and retries
- explicit structured final output works when advertised
- images work only when advertised and fail clearly otherwise
- every successful stream ends with `response.completed`
- backend failures and incomplete output are visible and correctly classified
- cancellation stops the upstream request
- local compaction works without a remote compaction endpoint
- unsupported Responses-only capabilities fail explicitly
- an explicitly enabled plaintext-reasoning backend captures and replays reasoning without exposing
  it as visible prompt text
- the built-in Kimi profile can complete a real tool continuation with a stable forwarded cache key
- no Chat-specific wire path is added to Codex core
- configuration, feature behavior, security boundaries, and conformance commands are documented

## Later Extensions

Treat each of these as a separate design and compatibility-profile expansion:

- client `tool_search` and deferred tool loading
- encrypted reasoning and backend-provided reasoning-summary parity
- additional provider-specific reasoning dialects and effort controls
- server-executed web search or other built-in tools
- Responses-over-WebSocket and adapter-owned previous-response state
- remote or encrypted compaction
- `/v1/models` and dynamic capability discovery
- multiple concrete Chat backend dialects

Each extension should add golden traces and end-to-end conformance cases before it is advertised.

## References

- `codex-rs/model-provider-info/src/lib.rs`: Responses-only provider contract and capability flags
- `codex-rs/core/src/client.rs`: current Responses request construction and provider-specific behavior
- `codex-rs/protocol/src/models.rs`: `ResponseItem`, content, call, output, reasoning, and compaction types
- `codex-rs/codex-api/src/sse/responses.rs`: the Responses events and terminal behavior Codex consumes
- `codex-rs/core/src/context_manager/normalize.rs`: call/output history invariants
- `codex-rs/core/src/tools/router.rs`: client-executed function, custom, and tool-search routing
- `codex-rs/responses-api-proxy`: reusable HTTP proxy and secret-handling patterns
- <https://developers.openai.com/api/docs/guides/migrate-to-responses>
- <https://developers.openai.com/api/docs/guides/function-calling>
- <https://developers.openai.com/api/docs/guides/structured-outputs>
