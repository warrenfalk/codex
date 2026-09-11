# Responses-to-Chat-Completions Adapter

## What it adds

This feature lets Codex use an OpenAI-compatible Chat Completions backend without adding a Chat
wire mode to Codex itself. A local compatibility process presents the Responses HTTP/SSE contract
that Codex expects and performs inference through a configured `/v1/chat/completions` endpoint.

## Final behavior

- Codex remains configured for `wire_api = "responses"` and uses a non-OpenAI custom provider.
- The compatibility process accepts streaming `POST /v1/responses` requests, sends one Chat
  completion request, and returns a Responses SSE stream.
- The process is stateless across requests. Codex sends full logical history, so calls and outputs
  are reconstructed as Chat assistant/tool messages without a conversation database.
- Codex's `reasoning.encrypted_content` include request is accepted as a compatibility hint because
  current Codex sends it for every model. The adapter never fabricates encrypted reasoning, and
  unknown include entries still fail explicitly.
- Instructions use the Chat `system` role by default and may use `developer` only when that backend
  capability is explicitly enabled.
- Text, user images, function tools, namespaced tools, freeform/custom tools, parallel calls, and
  structured JSON output are translated when their corresponding backend capabilities are
  enabled.
- Names that Chat cannot accept are mapped deterministically and reverse-mapped before Codex sees
  a call. Backend calls to undeclared names fail.
- Function and custom call IDs survive streaming, execution, later history, compaction, and retry.
- History is rejected before inference when it contains orphaned outputs, unresolved calls,
  duplicate call IDs, duplicate outputs, or mismatched function/custom output types.
- Assistant text is streamed as Responses text deltas and is also emitted as one complete output
  item. Function and custom calls are emitted as complete output items in stable index order.
- A backend may declare a plaintext-reasoning dialect. In that mode, streamed and buffered Chat
  `reasoning_content` is emitted as a Responses reasoning item and is retained in Codex history.
  On a later tool continuation, that reasoning is restored on the same Chat assistant message as
  the calls it preceded. Reasoning is never flattened into visible assistant or user text.
- A backend may declare that Responses `prompt_cache_key` is accepted by its Chat endpoint. The
  key is then forwarded unchanged and remains stable across a Codex tool continuation. The strict
  default is to omit it.
- Every successful result ends with `response.completed`, including results containing tool calls.
- Chat token usage is translated when present, including cached-input and reasoning-output details
  when the backend explicitly supplies them.
- A backend that cannot stream may be declared as buffered. Codex still receives a protocol-correct
  Responses SSE sequence.
- `stop` and `tool_calls` are successful terminal reasons, `length` is incomplete, policy filtering
  is failed, and unknown terminal reasons are failed rather than guessed.
- Upstream HTTP errors preserve their status and expose only a bounded error message. Failures
  after streaming starts become `response.failed` followed by stream closure.
- The process never retries inference. Codex owns retry policy, so a partial model action cannot be
  duplicated by an independent retry layer.
- A client disconnect drops the upstream response promptly. Request-header and stream-idle waits
  are bounded.
- Custom-provider compaction stays local and is translated like any other full-history sampling
  request. No remote Responses compaction endpoint is required.

## Capability boundary

V1 deliberately does not claim support for Responses WebSockets, previous-response state,
Responses Lite, deferred tool search, server-executed built-ins, encrypted reasoning replay or
agent messages, reasoning-summary replay, remote compaction, multiple Chat choices, verbosity
controls, audio input, or structured/image-bearing tool outputs. Accepting Codex's encrypted
reasoning include hint does not imply encrypted-reasoning support. Plaintext reasoning is a
separate, explicitly enabled backend dialect and does not imply encrypted-reasoning or
reasoning-summary parity.

Unsupported request fields, item types, tools, content, terminal reasons, and requested
capabilities fail explicitly. Correctness-neutral cache keys, client metadata, storage hints, and
service-tier hints may be accepted without affecting the model-visible request.

Backend capability switches are assertions, not emulation. Enabling parallel calls, images,
structured output, the developer role, reasoning effort, plaintext reasoning, or prompt-cache-key
forwarding means the selected backend is known to accept the translated Chat shape.

## Built-in Kimi K3 profile

- `kimi-k3` is a trusted local model entry in the model picker. A remote OpenAI model catalog
  cannot hide it or replace its metadata.
- Selecting it starts one embedded loopback adapter for that Codex session and routes Responses
  inference to `https://api.moonshot.ai/v1/chat/completions` with upstream model `kimi-k3`.
- The adapter reads a non-empty `API_KEY` entry from `~/.kimi-codex`. A missing or invalid file is
  an actionable error; Codex does not silently fall back to the previously configured provider.
- Model selection and provider selection change together. A rejected Kimi selection leaves the
  prior thread settings intact. Switching back to another model restores the thread's configured
  baseline provider, including for previous-model compaction.
- The TUI does not require an OpenAI login merely because the baseline provider does when the
  selected model is `kimi-k3`.
- The profile advertises a 1,048,576-token context window, text and image input, parallel tool
  calls, the shell-command tool, freeform patching, and `max` reasoning effort. It does not expose
  hosted Responses web search or image generation.
- The Kimi Chat dialect uses `system` instructions, native streaming, structured output, image
  input, parallel calls, plaintext `reasoning_content`, `reasoning_effort`, and forwarded
  `prompt_cache_key`. It deliberately omits an adapter-imposed completion-token limit.
- Codex may request encrypted reasoning inclusion or a reasoning summary as part of its normal
  Responses shape. For this profile those controls are accepted as a compatibility downgrade:
  the adapter returns and replays plaintext reasoning, an empty summary, and no encrypted payload.

## Configuration and security

- The custom Codex provider uses a loopback `/v1` base URL, Responses wire API, disabled Responses
  WebSockets, an explicit model slug, and accurate model/context capability metadata.
- The upstream model name may differ from the Codex-visible model and can be fixed at proxy launch.
- The preferred credential mode is proxy-owned bearer authentication read from a named environment
  variable. Inbound Authorization forwarding is a separate explicit mode; the two cannot be
  combined.
- Upstream URLs containing credentials are rejected. Prompt bodies, tool results, cookies, and
  authorization values are not written to ordinary logs.
- The built-in Kimi credential is held by the session-owned adapter and is never forwarded by
  Codex to the loopback listener. The credential file should be readable only by its owner.
- Request bodies, buffered responses, error bodies, and generated custom-tool descriptions have
  hard size bounds.
- Loopback is the default listener. Exposing the process beyond the local host requires an
  independently authenticated transport boundary.

## Validation expectations

- Whole-object translation tests cover roles, content, schemas, tools, deterministic names,
  history grouping, capability failures, stream accumulation, terminal reasons, and usage.
- HTTP tests cover exact Chat requests, exact Responses event ordering, buffered backends, status
  mapping, secret redaction, timeouts, malformed streams, and downstream cancellation.
- An unmodified Codex test client must complete streamed text, parallel function calls, a
  freeform patch call, structured JSON output, local compaction, and retry after a translated
  stream failure.
- Plaintext-reasoning conformance must prove that reasoning is captured as a Responses item and
  replayed on the next Chat assistant message beside its original parallel calls. It must also
  prove that the same non-empty cache key reaches both Chat requests.
- Kimi profile tests must cover trusted catalog metadata, embedded-provider selection, missing-key
  rejection without a partial settings update, and OpenAI-login bypass.
- A real backend smoke test must use only capabilities that backend explicitly documents and must
  confirm a text turn plus at least one tool round trip before the profile is relied on for normal
  work.

## Why it matters

Many useful local and hosted inference servers implement Chat Completions but not Responses. The
adapter provides a narrow compatibility seam while preserving the stronger invariant that Codex's
inference client has one Responses-shaped protocol and one structured agent loop.
