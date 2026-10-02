# Responses-to-Chat-Completions Adapter Status

## Objective

Expose the Responses request/event surface required by Codex while performing inference through a
Chat Completions backend. The current follow-up adds a built-in Kimi K3 inference profile with
plaintext reasoning capture/replay, stable prompt-cache-key forwarding, automatic local lifecycle,
trusted model metadata, and atomic provider switching.

## Baseline and scope

- original implementation baseline: `412e615b8269cbe0e281e6ebbd96cef0ad49e59f`
- Kimi follow-up baseline: `ae8fae7be5`
- status date: 2026-09-20
- adapter crate/binary: `codex-rs/responses-chat-completions-proxy`
- profile runtime: `codex-rs/inference-profiles`
- built-in model/profile: `kimi-k3` / provider `kimi`

The generic adapter remains strict by default. Plaintext reasoning and prompt-cache-key forwarding
are typed backend policies, not assumptions made for every Chat-compatible server. Encrypted
reasoning, reasoning-summary replay, Responses WebSockets, remote compaction, Responses Lite, and
server-executed Responses built-ins remain outside the compatibility profile.

## Current status

| Area | Status | Current behavior |
| --- | --- | --- |
| Generic Responses-to-Chat adapter | Complete | Text, images, function/custom/namespaced tools, parallel calls, structured output, usage, errors, cancellation, retry ownership, and local compaction remain covered. |
| Plaintext reasoning | Complete | Chat `reasoning_content` becomes Responses reasoning events/items and is replayed on the next Chat assistant message beside its original tool calls. Unexpected reasoning fails closed. |
| Prompt caching | Complete | Backends opt into forwarding `prompt_cache_key`; otherwise it is omitted. Conformance proves a stable non-empty key across a tool continuation. |
| Embedded lifecycle | Complete | One loopback adapter is started lazily per Kimi session and aborted when that session-owned runtime is dropped. Startup requires an active Tokio runtime and reports errors instead of panicking. |
| Kimi credential handling | Complete | `~/.kimi-codex` must contain a non-empty `API_KEY`. The adapter owns the bearer; Codex does not send it to the loopback listener. Missing/invalid credentials are actionable errors with no provider fallback. |
| Kimi model profile | Complete | Trusted model-picker entry, 1,048,576-token window, image input, parallel tools, shell/freeform patch tools, structured output, and `max` reasoning. Hosted Responses web/image tools are bounded off. |
| Provider switching | Complete | Model/provider changes are validated before state mutation. Switching away restores the configured baseline provider; previous-model compaction and first-turn prewarm use the correct provider. |
| Image tool results | Complete | Text/image parts remain on their original Chat tool message, including call ID and image detail. The Kimi profile enables this separately declared backend capability. |
| Kimi cold resume | Complete | A persisted `kimi-k3` / `kimi` pair rebuilds the embedded adapter without a static provider entry; the configured baseline remains available. |
| TUI integration | Complete | Kimi is visible in the model picker and does not trigger OpenAI-login gating inherited from the baseline provider. |
| Documentation | Complete | The feature contract, adapter README, implementation plan, and this status document describe the final profile and downgrade boundaries. |

## Kimi protocol profile

- upstream URL: `https://api.moonshot.ai/v1/chat/completions`
- upstream model: `kimi-k3`
- Chat streaming: enabled
- instructions role: `system`
- image input, parallel tool calls, structured output, and `reasoning_effort`: enabled
- reasoning dialect: sibling assistant/delta `reasoning_content`
- reasoning effort: `max`
- prompt caching: forward Responses `prompt_cache_key` unchanged
- completion-token limit: omitted by the adapter
- Responses encrypted-reasoning include: accepted as a compatibility hint for every profile because
  current Codex sends it unconditionally; the adapter never fabricates an encrypted payload
- Responses summary controls: accepted only as a plaintext-profile downgrade; emitted reasoning has
  an empty summary and no encrypted payload

## Validation completed

### September 20 regression follow-up

- Fixed the adapter rejection after `view_image`: structured text results are supported, and an
  explicit image-tool-output capability forwards multimodal parts on the original Chat tool
  message. The Kimi profile enables it; generic backends remain opt-in.
- Fixed cold resume of a persisted `kimi-k3` / `kimi` pair. Configuration loading keeps the
  configured baseline provider instead of looking up the runtime-derived `kimi` label as a
  static provider; session startup creates a fresh embedded adapter. Rollouts and user config
  need no migration.
- Direct Kimi protocol probes accepted a synthetic red image in a tool result, both without
  detail and with `detail: "original"`, and answered `Red`.
- All 50 focused adapter/profile/configuration tests passed, including a real Codex `view_image`
  continuation and replay on the next turn, and cold resume from the saved effective provider.
- A source-built adapter completed two live Kimi requests with the synthetic image: the tool
  continuation and a subsequent full-history replay both answered `Red`, captured reasoning,
  and ended with `response.completed`.
- The affected-crate run executed 4,336 tests: 4,023 passed, 313 failed, and 8 were skipped.
  All 504 core configuration tests, all adapter/profile tests, and all new regressions passed.
  The broader core run is not green;
  failures include read-only home paths, missing `test_stdio_server`, and hook/notification/tool
  harness failures. A representative read-only-home failure passed when rerun outside the sandbox.
- A source-built app-server resumed an isolated copy of rollout
  `01a0bfb6-3d6e-7523-8932-8e6091da57c5` through `thread/resume`, first with the saved Kimi pair
  explicitly supplied, then in a fresh process with no model/provider override. Both returned
  `kimi-k3` / `kimi`. No inference was requested, and the original rollout checksum was unchanged.
- Build artifacts were relocated to the checkout filesystem after the `/tmp` build mount filled.
- Scoped `just fix` completed successfully; unrelated pre-existing cleanup suggestions were
  reverted. `just fmt` completed successfully. Tests were not rerun after these final passes.
  The duplicate build cache was removed; validation logs and the smoke-tested executables remain
  under `.cache/kimi-validation/`.

### Original profile validation

- `cargo check --tests` passed for the adapter, inference profile, model provider, model manager,
  core, and TUI after the profile wiring changes.
- 132/132 adapter, inference-profile, model-provider, and model-manager tests pass with no skips.
  This includes an unmodified Codex tool continuation that verifies exact reasoning replay and a
  stable cache key on both Chat requests.
- Both `codex-core` Kimi integration tests pass without sandbox skip guards: embedded-provider
  selection at startup and missing-key rejection without a partial settings update.
- The full `codex-core` crate run executed 2,952 tests: 2,864 passed, one unrelated test was flaky,
  and 88 failed for existing environment/harness reasons such as read-only `~/.codex`, missing
  `test_stdio_server`, shell/PATH assumptions, and notification timing. The new Kimi/profile and
  provider-prewarm tests passed in that run.
- The TUI run executed 3,098 tests. The expected model-picker snapshot was reviewed and updated;
  the Kimi login test and updated snapshot pass. One existing connected-footer test still fails
  alone in code untouched by this feature.
- A direct buffered Kimi request using `kimi-k3`, `reasoning_effort=max`, and a cache key returned
  the expected text plus non-empty reasoning content and reasoning-token usage.
- A source-built Codex process selected provider `kimi`, called the shell tool through the embedded
  adapter, replayed the tool continuation, and produced the requested final marker.
- The final scoped `just fix` pass completed successfully for the adapter, inference profile,
  model-provider, model-manager, core, and TUI crates.
- `just fmt` completed the Rust formatting pass. Its repository-wide wrapper still exits nonzero
  when the Python SDK and scripts steps try to execute Ruff's generic dynamically linked binary on
  NixOS; neither Python tree is touched by this feature.

## Remaining validation boundary

The complete workspace `just test` has not been run; the user explicitly requested validation
scoped to affected crates. The core crate's broader suite still has the failures recorded above.

The existing `~/.kimi-codex` file was observed with Unix mode `0644`. The implementation does not
mutate files outside the workspace; changing it to `0600` is recommended.
