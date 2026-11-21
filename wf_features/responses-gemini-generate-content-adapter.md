# Gemini GenerateContent adapter

Codex can use Gemini 4 Neon with a Google Gemini API key and its usual local tool loop.
The adapter exposes streaming Responses HTTP on loopback and streams inference through
Google's public GenerateContent API. A Codex custom provider can use this route alongside
its other providers.

- Codex owns the conversation, tool execution, approvals, retries, and local compaction.
  A saved Neon conversation can resume through a fresh adapter with the same endpoint
  and credentials. The adapter alone receives the API key.
- Text streams incrementally. Function tools, namespaced tools, and freeform tools keep
  their Codex identities. The model sees parallel tool results in the order of the calls.
  A partial response cannot authorize tools after an upstream failure.
- Gemini's opaque thought signatures survive Responses history in their original association
  with text or tool calls, including signatures on empty text parts and the first call in a
  parallel batch. Signatures travel as opaque replay metadata.
  Malformed replay and unresolved tool results fail explicitly.
- Inline base64 user images and image-bearing tool results are supported. Tool images stay
  attached to the originating tool result. Structured text output uses Google's JSON Schema
  support. Context and thinking controls match the selected Gemini model.
- Token usage accounts for cached input and thinking when Google reports them. Token
  exhaustion is incomplete. Upstream refusals, malformed streams, and timeouts fail.
  The first version supports the synchronous streaming API, local function tools, and
  Google API-key authentication.

Validation requires an end-to-end Codex turn with a signed tool batch and exact replay
of its call order, signatures, IDs, names, and matching outputs. A live Neon check should
complete a tool continuation across a proxy restart and resume a saved Codex conversation
through a fresh proxy. Google-hosted tools, remote conversation state, and Antigravity
subscription authentication require their own compatibility work.
