# Fork-Specific Codex Work

This repository is a fork of the main Codex project. In this fork, treat every commit after the
most recent `rust-v*` tag reachable from `HEAD` as one of our fork-specific commits/features.
Any reference in these instructions to "our features", "our commits", or "my changes" refers to
that commit range.

When making changes here, do not optimize for upstreamability or broad shareability. This fork is
for our own use, so diverging from upstream behavior is acceptable. The goal is to keep our
fork-specific changes straightforward to rebase onto newer upstream versions.

When designing fork-local CLI commands, app-server APIs, or UI surfaces, prefer the command or
interface that is directly useful for this fork. Do not hide features under debug/experimental
paths solely to preserve a public-product stability contract; use hidden or experimental surfaces
only when they are genuinely more useful for our workflow or when the user asks for them.

Every new fork-local feature must include a corresponding document in `wf_features/`. The document
should describe the feature intent and the final behavior that must be preserved with enough detail
to rebuild the feature from scratch on a different base. Keep these documents focused on observable
behavior, user value, edge cases, and validation expectations; do not describe the current
implementation, architecture, or code layout. Feature documents are not required for bug fixes or
maintenance-only changes that do not add a feature.

Upstream or non-fork bug fixes use a paired commit structure instead of feature documents. Create
two adjacent commits: first a `[bug test] ...` commit containing the failing regression test that
demonstrates the bug, then a `[bug fix] ...` commit containing the fix. Keep the pair test-first
and adjacent in history so replaying or dropping the fix remains straightforward during future
rebases.

Fixes to fork-local features do not use `[bug test]` / `[bug fix]` pairs. Commit the regression
test and fix together as a `fixup!` commit targeting the fork feature commit that introduced or
owns the behavior. If the observable behavior changes, update that feature's `wf_features/`
document in the same fixup.

# Rust/codex-rs

In the codex-rs folder where the rust code lives:

- Crate names are prefixed with `codex-`. For example, the `core` folder's crate is named `codex-core`
- When using format! and you can inline variables into {}, always do that.
- Install any commands the repo relies on (for example `just`, `rg`, or `cargo-insta`) if they aren't already available before running instructions here.
- Never add or modify any code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
  - You operate in a sandbox where `CODEX_SANDBOX_NETWORK_DISABLED=1` will be set whenever you use the `shell` tool. Any existing code that uses `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` was authored with this fact in mind. It is often used to early exit out of tests that the author knew you would not be able to run given your sandbox limitations.
  - Similarly, when you spawn a process using Seatbelt (`/usr/bin/sandbox-exec`), `CODEX_SANDBOX=seatbelt` will be set on the child process. Integration tests that want to run Seatbelt themselves cannot be run under Seatbelt, so checks for `CODEX_SANDBOX=seatbelt` are also often used to early exit out of tests, as appropriate.
- Always collapse if statements per https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if
- Always inline format! args when possible per https://rust-lang.github.io/rust-clippy/master/index.html#uninlined_format_args
- Use method references over closures when possible per https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls
- Avoid bool or ambiguous `Option` parameters that force callers to write hard-to-read code such as `foo(false)` or `bar(None)`. Prefer enums, named methods, newtypes, or other idiomatic Rust API shapes when they keep the callsite self-documenting.
- When you cannot make that API change and still need a small positional-literal callsite in Rust, follow the `argument_comment_lint` convention:
  - Use an exact `/*param_name*/` comment before opaque literal arguments such as `None`, booleans, and numeric literals when passing them by position.
  - A method's sole non-self argument is exempt when the method and parameter names match, such as `.enabled(false)` for `fn enabled(&self, enabled: bool)`.
  - Do not add these comments for string or char literals unless the comment adds real clarity; those literals are intentionally exempt from the lint.
  - If you add one of these comments, the parameter name must exactly match the callee signature.
- When possible, make `match` statements exhaustive and avoid wildcard arms.
- Newly added traits should include doc comments that explain their role and how implementations are expected to use them.
- Discourage both `#[async_trait]` and `#[allow(async_fn_in_trait)]` in Rust traits.
  - Prefer native RPITIT trait methods with explicit `Send` bounds on the returned future, as in `3c7f013f9735` / `#16630`.
  - Preferred trait shape:
    `fn foo(&self, ...) -> impl std::future::Future<Output = T> + Send;`
  - Implementations may still use `async fn foo(&self, ...) -> T` when they satisfy that contract.
  - Do not use `#[allow(async_fn_in_trait)]` as a shortcut around spelling the future contract explicitly.
- When writing tests, prefer comparing the equality of entire objects over fields one by one.
- Do not add tests for values that are statically defined.
- Do not add negative tests for logic that was removed.
- Do not add general product or user-facing documentation to the `docs/` folder. The official Codex documentation lives elsewhere. The exception is app-server API documentation, which is covered by the app-server guidance below.
- Prefer private modules and explicitly exported public crate API.
- If you change `ConfigToml` or nested config types, run `just write-config-schema` to update `codex-rs/core/config.schema.json`.
- When working with MCP tool calls, prefer using `codex-rs/codex-mcp/src/mcp_connection_manager.rs` to handle mutation of tools and tool calls. Aim to minimize the footprint of changes and leverage existing abstractions rather than plumbing code through multiple levels of function calls.
- Do not call `reset_client_session` unnecessarily; let the incremental check logic decide whether to reuse the previous request.
- Do not use Bazel maintenance commands in this fork. Do not run `just bazel-lock-update` or
  `just bazel-lock-check` unless the user explicitly asks.
- Do not spend time updating `BUILD.bazel`, `MODULE.bazel`, or other Bazel metadata in this fork
  unless the user explicitly asks.
- Do not create small helper methods that are referenced only once.
- For tracing async work, instrument the function or method definition with
  `#[tracing::instrument(...)]` instead of attaching spans to futures with
  `.instrument(...)` at call sites. Before adding instrumentation, check whether the callee—or
  the implementation method it immediately delegates to—is already instrumented.
- Avoid large modules:
  - Prefer adding new modules instead of growing existing ones.
  - Target Rust modules under 500 LoC, excluding tests.
  - If a file exceeds roughly 800 LoC, add new functionality in a new module instead of extending
    the existing file unless there is a strong documented reason not to.
  - This rule applies especially to high-touch files that already attract unrelated changes, such
    as `codex-rs/tui/src/app.rs`, `codex-rs/tui/src/bottom_pane/chat_composer.rs`,
    `codex-rs/tui/src/bottom_pane/footer.rs`, `codex-rs/tui/src/chatwidget.rs`,
    `codex-rs/tui/src/bottom_pane/mod.rs`, `codex-rs/tui/src/resume_picker.rs`,
    `codex-rs/tui/src/connected_app_server.rs`, and similarly central orchestration modules.
  - When extracting code from a large module, move the related tests and module/type docs toward
    the new implementation so the invariants stay close to the code that owns them.
  - Avoid adding new standalone methods to `codex-rs/tui/src/chatwidget.rs` unless the change is
    trivial; prefer new modules/files and keep `chatwidget.rs` focused on orchestration.
- When running Rust commands (e.g. `just fix` or `just test`) be patient with the command and never try to kill them using the PID. Rust lock can make the execution slow, this is expected.

## Repository overview

This repository is a monorepo with:

- `codex-rs`: primary Rust workspace for the CLI, TUI, and core libraries.
- `codex-cli`: TypeScript npm wrapper.
- `sdk`: TypeScript SDK for building agents.
- `shell-tool-mcp`: MCP server for shell tool integration.

The root `justfile` targets `codex-rs` via `working-directory := "codex-rs"` for Rust-focused recipes.

## Additional useful commands

Beyond crate-specific `cargo` commands, these are commonly useful:

- Run interactive Codex: `just codex "your prompt"`
- Run non-interactive mode: `just exec "your prompt"`
- Faster test runner wrapper: `just test`
- Workspace lint pass: `just clippy`
- Run MCP server locally: `just mcp-server-run`
- Tail Codex logs: `just log`
- Do not use Bazel workflows in this fork.
- Nix workflows: `nix build`, `nix develop`
- Root formatting for non-Rust files: `npm run format`, `npm run format:fix`

## Runtime paths and tooling

- User config: `~/.codex/config.toml`
- TUI log file: `~/.codex/log/codex-tui.log` (controlled by `RUST_LOG`)
- Local state and SQLite data: `~/.codex/`
- TypeScript workspace toolchain: `pnpm@10.28.2` minimum and Node.js 22+

## Rebases

Rebasing our in-flight changes onto the latest upstream is frequent in this repo. Treat rebase
conflict resolution as a first-class task, not a mechanical cleanup step.

- Preserve the intent of both sides: our commit and the incoming upstream change.
- Before resolving a conflict, read the surrounding code and inspect both stage variants (`:2`/`:3`)
  so the merge decision is based on behavior, not just marker placement.
- Prefer merges that minimize future conflicts:
  - optimize for easy future rebases, not for patches that would be suitable to send upstream,
  - preserve newer shared abstractions and call patterns from upstream when they subsume our older
    local code,
  - avoid unnecessary churn in high-conflict files,
  - keep file-local conventions aligned with the latest upstream style when behavior is equivalent.
- If a conflict spans source files plus generated artifacts, resolve the source intent first, then
  regenerate or reconcile generated files from that resolved source.
- When a conflict reveals that a fix really belongs to multiple historical commits, split the
  follow-up cleanly instead of forcing an inaccurate single fixup target.
- After finishing a rebase and before declaring it done, verify builds in this order:
  1. `cargo build` for the Rust workspace (use `nix develop -c cargo build` if `cargo` is not on `PATH`).
  2. If that succeeds, run `nix build` from the repo root.
- If `nix build` fails, capture the exact failure and treat it as part of the rebase follow-up; do
  not assume a successful Cargo build is sufficient.
- Any fixes required to make `cargo build` or `nix build` pass after a rebase should be committed
  as separate `fixup!` commits against the appropriate commits from our rebased work, not folded
  into an arbitrary final conflict-resolution commit.
- After creating those `fixup!` commits, do not run another rebase/autosquash locally just to apply
  them. Leave the fixup commits in history and report them clearly; the user will decide when to
  autosquash them later.
- Only fix up our own commits:
  - define "our commits" as commits in the range `<most-recent-rust-v-tag>..HEAD`,
  - identify that tag from the current history as the most recent reachable `rust-v*` tag,
  - inspect candidate targets with `git log --oneline <most-recent-rust-v-tag>..HEAD`.
- Choose the appropriate fixup target by asking which commit introduced the behavior, API usage, or
  test expectation that now needs correction:
  - use `git blame` on the affected lines,
  - use `git log -S <symbol-or-field>` or `git log -- <path>` to find the introducing commit,
  - prefer the most specific commit in our rebased range that introduced the broken behavior,
  - if runtime code and tests were introduced by different commits, split follow-up fixups so each
    targets the commit that actually introduced that part,
  - do not target upstream commits that are outside `<base>..HEAD`, even if they originally
    introduced the concept before our branch rebased onto them.

Run `just fmt` (in the `codex-rs` directory) automatically after you have finished making code changes anywhere in this repository; do not ask for approval to run it. Additionally, run the tests:

1. Do not run `cargo test` directly. Use `just test` so test execution follows the repo defaults.
2. Run the test for the specific project that was changed. For example, if changes were made in `codex-rs/tui`, run `just test -p codex-tui`.
3. Once those pass, if any changes were made in common, core, or protocol, run the complete test suite with `just test`. Avoid `--all-features` for routine local runs because it expands the build matrix and can significantly increase `target/` disk usage; use it only when you specifically need full feature coverage. project-specific or individual tests can be run without asking the user, but do ask the user before running the complete test suite.
- If you change `flake.nix` or `codex-rs/default.nix`, run `nix build` from the repo root before finalizing so Nix environment regressions are caught locally.

Before finalizing a large change to `codex-rs`, run `just fix -p <project>` (in `codex-rs` directory) to fix any linter issues in the code. Prefer scoping with `-p` to avoid slow workspace‑wide Clippy builds; only run `just fix` without `-p` if you changed shared crates. Do not re-run tests after running `fix` or `fmt`.

Do not run `just argument-comment-lint` in this fork unless the user explicitly asks.

## The `codex-core` crate

Over time, the `codex-core` crate (defined in `codex-rs/core/`) has become bloated because it is the largest crate, so it is often easier to add something new to `codex-core` rather than refactor out the library code you need so your new code neither takes a dependency on, nor contributes to the size of, `codex-core`.

To that end: **resist adding code to codex-core**!

Particularly when introducing a new concept/feature/API, before adding to `codex-core`, consider whether:

- There is an existing crate other than `codex-core` that is an appropriate place for your new code to live.
- It is time to introduce a new crate to the Cargo workspace for your new functionality. Refactor existing code as necessary to make this happen.

Likewise, when reviewing code, do not hesitate to push back on PRs that would unnecessarily add code to `codex-core`.

## Code Review Rules

### Crate API surface

Keep crate API surfaces as small as possible. Avoid proliferating test-only helpers.

### Model visible context

Codex maintains a context (history of messages) that is sent to the model in inference requests.

1. No history rewrite - the context must be built up incrementally.
2. Avoid frequent changes to context that cause cache misses.
3. No unbounded items - everything injected in the model context must have a bounded size and a hard cap.
4. No items larger than 10K tokens.
5. Highlight new individual items that can cross >1k tokens as P0. These need an additional manual review.
6. All injected fragments must be defined as structs in `core/context` and implement ContextualUserFragment trait

### Breaking changes

Search for breaking changes in external integration surfaces:

- app-server APIs
- raw response item events (`rawResponseItem/*`), even while experimental
- CLI parameters
- configuration loading
- resuming sessions from existing rollouts

### Test authoring guidance

For agent changes prefer integration tests over unit tests. Integration tests are under `core/suite` and use `test_codex` to set up a test instance of codex.

Features that change the agent logic MUST add an integration test:

- Provide a list of major logic changes and user-facing behaviors that need to be tested.

If unit tests are needed, put them in a dedicated test file (\*\_tests.rs).
Avoid test-only functions in the main implementation.

Check whether there are existing helpers to make tests more streamlined and readable.

### Change size guidance (800 lines)

Unless the change is mechanical the total number of changed lines should not exceed 800 lines.
For complex logic changes the size should be under 500 lines.

If the change is larger, explore whether it can be split into reviewable stages and identify the smallest coherent stage to land first.
Base the staging suggestion on the actual diff, dependencies, and affected call sites.

## TUI style conventions

See `codex-rs/tui/styles.md`.

## TUI code conventions

- Use concise styling helpers from ratatui’s Stylize trait.
  - Basic spans: use "text".into()
  - Styled spans: use "text".red(), "text".green(), "text".magenta(), "text".dim(), etc.
  - Prefer these over constructing styles with `Span::styled` and `Style` directly.
  - Example: patch summary file lines
    - Desired: vec!["  └ ".into(), "M".red(), " ".dim(), "tui/src/app.rs".dim()]

### TUI Styling (ratatui)

- Prefer Stylize helpers: use "text".dim(), .bold(), .cyan(), .italic(), .underlined() instead of manual Style where possible.
- Prefer simple conversions: use "text".into() for spans and vec![…].into() for lines; when inference is ambiguous (e.g., Paragraph::new/Cell::from), use Line::from(spans) or Span::from(text).
- Computed styles: if the Style is computed at runtime, using `Span::styled` is OK (`Span::from(text).set_style(style)` is also acceptable).
- Avoid hardcoded white: do not use `.white()`; prefer the default foreground (no color).
- Chaining: combine helpers by chaining for readability (e.g., url.cyan().underlined()).
- Single items: prefer "text".into(); use Line::from(text) or Span::from(text) only when the target type isn’t obvious from context, or when using .into() would require extra type annotations.
- Building lines: use vec![…].into() to construct a Line when the target type is obvious and no extra type annotations are needed; otherwise use Line::from(vec![…]).
- Avoid churn: don’t refactor between equivalent forms (Span::styled ↔ set_style, Line::from ↔ .into()) without a clear readability or functional gain; follow file‑local conventions and do not introduce type annotations solely to satisfy .into().
- Compactness: prefer the form that stays on one line after rustfmt; if only one of Line::from(vec![…]) or vec![…].into() avoids wrapping, choose that. If both wrap, pick the one with fewer wrapped lines.

### Text wrapping

- Always use textwrap::wrap to wrap plain strings.
- If you have a ratatui Line and you want to wrap it, use the helpers in tui/src/wrapping.rs, e.g. word_wrap_lines / word_wrap_line.
- If you need to indent wrapped lines, use the initial_indent / subsequent_indent options from RtOptions if you can, rather than writing custom logic.
- If you have a list of lines and you need to prefix them all with some prefix (optionally different on the first vs subsequent lines), use the `prefix_lines` helper from line_utils.

## Tests

### Test module organization

- When adding a new test module, define its contents in a separate sibling file rather than inline in the implementation file.
- Use an explicit `#[path = "..._tests.rs"]` attribute so the test filename is descriptive and easy to locate:

  ```rust
  #[cfg(test)]
  #[path = "parser_tests.rs"]
  mod tests;
  ```

- This applies only when introducing a new test module. Do not move or rewrite existing inline `#[cfg(test)] mod tests { ... }` modules solely to follow this convention.

### Snapshot tests

This repo uses snapshot tests (via `insta`), especially in `codex-rs/tui`, to validate rendered output.

**Requirement:** any change that affects user-visible UI (including adding new UI) must include
corresponding `insta` snapshot coverage (add a new snapshot test if one doesn't exist yet, or
update the existing snapshot). Review and accept snapshot updates as part of the PR so UI impact
is easy to review and future diffs stay visual.

When UI or text output changes intentionally, update the snapshots as follows:

- Run tests to generate any updated snapshots:
  - `just test -p codex-tui`
- Check what’s pending:
  - `cargo insta pending-snapshots -p codex-tui`
- Review changes by reading the generated `*.snap.new` files directly in the repo, or preview a specific file:
  - `cargo insta show -p codex-tui path/to/file.snap.new`
- Only if you intend to accept all new snapshots in this crate, run:
  - `cargo insta accept -p codex-tui`

If you don’t have the tool:

- `cargo install --locked cargo-insta`

- Never commit `assertion_line:` churn in snapshot files.
  - For a newly added snapshot file, remove the `assertion_line:` header before staging it.
  - For an existing snapshot file that does not already contain `assertion_line:`, do not add that header.
  - For an existing snapshot file that already contains `assertion_line:`, do not change or remove that line unless the user explicitly asks for snapshot-header cleanup.
  - If the only diff in a snapshot file is `assertion_line:`, discard that diff instead of accepting it.

- On branches where `codex-rs/Cargo.toml` still uses the placeholder
  `0.0.0`, the Nix package build handles this by patching `Cargo.toml` in
  `codex-rs/default.nix` before building.
- Keep version-sensitive snapshots stable. When a TUI snapshot includes the
  CLI version, normalize it to the test placeholder instead of baking in a
  release tag or expecting the shell to override Cargo's compile-time value.

### Benchmarks

cargo benchmarks can be run with `just bench`, use the divan crate to write new ones.

Use `just bench-smoke` to dry-run the benchmark for a single iteration to ensure it works.

### Test assertions

- Tests should use pretty_assertions::assert_eq for clearer diffs. Import this at the top of the test module if it isn't already.
- Prefer deep equals comparisons whenever possible. Perform `assert_eq!()` on entire objects, rather than individual fields.
- Avoid mutating process environment in tests; prefer passing environment-derived flags or dependencies from above.
- When changing Linux sandbox or seccomp behavior, add or update end-to-end coverage in `codex-rs/exec/tests/suite/sandbox.rs` so the policy change is exercised under the real sandbox harness.

### Spawning workspace binaries in tests

- Prefer `codex_utils_cargo_bin::cargo_bin("...")` over `assert_cmd::Command::cargo_bin(...)` or `escargot` when tests need to spawn first-party binaries.
- When locating fixture files or test resources, prefer `codex_utils_cargo_bin::find_resource!` over `env!("CARGO_MANIFEST_DIR")` so tests keep using the repo helpers rather than ad hoc path logic.

### Integration tests (core)

- Prefer the utilities in `core_test_support::responses` when writing end-to-end Codex tests.

- All `mount_sse*` helpers return a `ResponseMock`; hold onto it so you can assert against outbound `/responses` POST bodies.
- Use `ResponseMock::single_request()` when a test should only issue one POST, or `ResponseMock::requests()` to inspect every captured `ResponsesRequest`.
- `ResponsesRequest` exposes helpers (`body_json`, `input`, `function_call_output`, `custom_tool_call_output`, `call_output`, `header`, `path`, `query_param`) so assertions can target structured payloads instead of manual JSON digging.
- Build SSE payloads with the provided `ev_*` constructors and the `sse(...)`.
- Prefer `wait_for_event` over `wait_for_event_with_timeout`.
- Prefer `mount_sse_once` over `mount_sse_once_match` or `mount_sse_sequence`

- Typical pattern:

  ```rust
  let mock = responses::mount_sse_once(&server, responses::sse(vec![
      responses::ev_response_created("resp-1"),
      responses::ev_function_call(call_id, "shell", &serde_json::to_string(&args)?),
      responses::ev_completed("resp-1"),
  ])).await;

  codex.submit(Op::UserTurn { ... }).await?;

  // Assert request body if needed.
  let request = mock.single_request();
  // assert using request.function_call_output(call_id) or request.json_body() or other helpers.
  ```

## App-server API Development Best Practices

These guidelines apply to app-server protocol work in `codex-rs`, especially:

- `app-server-protocol/src/protocol/common.rs`
- `app-server-protocol/src/protocol/v2.rs`
- `app-server/README.md`

### Core Rules

- All active API development should happen in app-server v2. Do not add new API surface area to v1.
- Follow payload naming consistently:
  `*Params` for request payloads, `*Response` for responses, and `*Notification` for notifications.
- Expose RPC methods as `<resource>/<method>` and keep `<resource>` singular (for example, `thread/read`, `app/list`).
- Always expose fields as camelCase on the wire with `#[serde(rename_all = "camelCase")]` unless a tagged union or explicit compatibility requirement needs a targeted rename.
- Always expose string enum values as camelCase on the wire with matching serde and TS `rename_all = "camelCase"` annotations unless an explicit compatibility requirement needs targeted renames.
- Exception: config RPC payloads are expected to use snake_case to mirror config.toml keys (see the config read/write/list APIs in `app-server-protocol/src/protocol/v2.rs`).
- Always set `#[ts(export_to = "v2/")]` on v2 request/response/notification types so generated TypeScript lands in the correct namespace.
- Never use `#[serde(skip_serializing_if = "Option::is_none")]` for v2 API payload fields.
  Exception: client->server requests that intentionally have no params may use:
  `params: #[ts(type = "undefined")] #[serde(skip_serializing_if = "Option::is_none")] Option<()>`.
- Keep Rust and TS wire renames aligned. If a field or variant uses `#[serde(rename = "...")]`, add matching `#[ts(rename = "...")]`.
- For discriminated unions, use explicit tagging in both serializers:
  `#[serde(tag = "type", ...)]` and `#[ts(tag = "type", ...)]`.
- Prefer plain `String` IDs at the API boundary (do UUID parsing/conversion internally if needed).
- Timestamps should be integer Unix seconds (`i64`) and named `*_at` (for example, `created_at`, `updated_at`, `resets_at`).
- For experimental API surface area:
  use `#[experimental("method/or/field")]`, derive `ExperimentalApi` when field-level gating is needed, and use `inspect_params: true` in `common.rs` when only some fields of a method are experimental.

### Client->server request payloads (`*Params`)

- Every optional field must be annotated with `#[ts(optional = nullable)]`. Do not use `#[ts(optional = nullable)]` outside client->server request payloads (`*Params`).
- Optional collection fields (for example `Vec`, `HashMap`) must use `Option<...>` + `#[ts(optional = nullable)]`. Do not use `#[serde(default)]` to model optional collections, and do not use `skip_serializing_if` on v2 payload fields.
- When you want omission to mean `false` for boolean fields, use `#[serde(default, skip_serializing_if = "std::ops::Not::not")] pub field: bool` over `Option<bool>`.
- For new list methods, implement cursor pagination by default:
  request fields `pub cursor: Option<String>` and `pub limit: Option<u32>`,
  response fields `pub data: Vec<...>` and `pub next_cursor: Option<String>`.

### Development Workflow

- Update app-server docs/examples when API behavior changes (at minimum `app-server/README.md`).
- Regenerate schema fixtures when API shapes change:
  `just write-app-server-schema`
  (and `just write-app-server-schema --experimental` when experimental API fixtures are affected).
- Validate with `just test -p codex-app-server-protocol`.
- Avoid boilerplate tests that only assert experimental field markers for individual
  request fields in `common.rs`; rely on schema generation/tests and behavioral coverage instead.

## Python Development Best Practices

### Ignore Python 2 compatibility

This project uses Python 3+. You should not use the `__future__` module.

If you need to worry about feature compatibility between different 3.xx point releases, check the
closest `pyproject.toml`'s `requires-python` field to see what minimum runtime version is supported.

## Platform Support

Tests and features must support Linux, macOS and Windows unless feature is explicitly OS-specific.
