# CLAUDE.md

## Project

hob is a native Emacs AI coding agent. Two halves:

- **`agent/`** — Rust binary (`hob-agent`). Runs a multi-turn agent loop:
  streams LLM responses via a provider abstraction (Anthropic implemented,
  OpenAI-ready), executes tools (read_file, shell, list_files), feeds results
  back, and re-prompts until the model stops. Communicates with Emacs over
  newline-delimited JSON on stdin/stdout. No persistence yet (messages in memory).
- **`lisp/`** — Emacs Lisp package. Subprocess lifecycle, JSON IPC
  encode/decode, and a `*hob*` output buffer that streams tokens and shows
  tool call/result markers.

## Build & test

```bash
make build                                    # cargo build --release
make byte-compile                             # emacs --batch byte-compile lisp/*.el
cargo check --manifest-path agent/Cargo.toml  # type-check without full build
cargo test --manifest-path agent/Cargo.toml   # run rust tests
```

## Testing requirements

**Every code change must include tests.** This is mandatory, not optional.

### When to write tests

- **New functions/methods**: Add unit tests covering the happy path and at least
  one error/edge case.
- **Bug fixes**: Add a regression test that fails without the fix and passes
  with it.
- **New tools**: Add tests for `definition()` (schema is valid) and `execute()`
  (correct output for valid input, proper error for invalid input).
- **IPC message types**: Add round-trip serde tests (serialize → deserialize →
  assert equality).
- **Refactors**: Existing tests must still pass. If you change a public API,
  update its tests to match.

### Where tests live

- Rust unit tests go in a `#[cfg(test)] mod tests` block at the bottom of the
  file being tested. This is the standard Rust convention — do not create
  separate test files for unit tests.
- Integration tests (if needed) go in `agent/tests/`.

### How to write good tests

- Test behavior, not implementation. Assert on outputs and side effects, not
  internal state.
- Use descriptive test names: `test_read_file_returns_error_for_missing_path`,
  not `test1`.
- Keep tests focused — one logical assertion per test.
- Do not skip writing tests because the code "seems simple." If it can break, it
  needs a test.

### Running tests

Always run `cargo test --manifest-path agent/Cargo.toml` after making changes
and confirm all tests pass before considering the work done.

## Code conventions

### Rust (agent/)

- Rust 2021 edition, stable toolchain.
- `anyhow::Result` for error propagation. Define domain error types only when
  callers need to match on variants.
- `tokio` for async. The IPC loop, agent loop, API client, and tools are all
  async.
- `tracing` for logging (`info!`, `debug!`, `error!`). Never write to stdout
  except IPC messages — stdout is the Emacs pipe.
- `serde` with `#[serde(tag = "type", rename_all = "snake_case")]` for all IPC
  message types so they serialize as `{"type": "token", ...}`.
- Tool implementations go in separate files under `agent/src/tools/`. Each tool
  exports `definition() -> ToolDef` and `execute(Value) -> Result<String>`.
  The registry in `tools/mod.rs` collects definitions and dispatches by name.
- Tool output is truncated at 50KB. The truncation happens in `tools/mod.rs`
  after execution.

### Elisp (lisp/)

- `lexical-binding: t` in every file.
- Prefix all symbols with `hob-` (public) or `hob--` (internal).
- `defcustom` for user-facing config, `defvar` for internal state.
- The `*hob*` buffer uses `special-mode` (read-only). Mutations go through
  `let ((inhibit-read-only t))`.
- Process filter accumulates partial lines in `hob--output-buffer`, splits on
  `\n`, dispatches complete JSON lines to `hob-ipc-dispatch`.

## Current IPC protocol

All messages are single-line JSON with a `"type"` field. This is what's
currently defined in `agent/src/ipc.rs`.

### Emacs → agent (Request)

| type | fields | status |
|------|--------|--------|
| `task` | `id`, `prompt` | spawns agent loop on a tokio task |
| `cancel` | `id` | fires CancellationToken for the task |
| `ping` | | replies with pong |

### Agent → Emacs (Response)

| type | fields | status |
|------|--------|--------|
| `token` | `id`, `content` | streamed during LLM response |
| `tool_call` | `id`, `tool`, `input` | sent before tool execution |
| `tool_result` | `id`, `tool`, `output` | sent after tool execution |
| `done` | `id` | sent when task completes |
| `error` | `id`, `message` | sent on errors or cancellation |
| `pong` | | reply to ping |

## Research

`research/` contains detailed architecture docs reverse-engineered from the
OpenCode agent harness. This is the reference for how to build out the agent.

Read order: `00-overview.md` first, then 01–08 (core systems) in order.
09–14 are reference docs to consult as needed.

Key design decisions from the research:

- **Agent loop** (01): `while(true)`, reads state from DB each iteration,
  continues on `tool_calls` finish reason, breaks on `stop`.
- **Stream processor** (02): SSE events → tool state machine
  (pending→running→completed|error), doom loop detection.
- **Tools** (03, 10): functions with JSON schemas, permission check before
  execution, output truncation. The edit tool uses a multi-level fuzzy match
  cascade.
- **Permissions** (04): last-match-wins wildcard rules, async ask flow,
  rejection cascades.
- **Compaction** (05): prune old tool outputs first (cheap), then summarize via
  LLM call (expensive).
- **Provider abstraction** (06): we're Anthropic-only for now but the streaming
  interface should be clean enough to swap later.
- **System prompts** (07): layered assembly — base prompt, environment context,
  instruction files.
- **Retry** (08): context overflow → compact, rate limit → backoff (2s×2^n, cap
  30s), auth → stop.
- **Storage** (09): SQLite, WAL mode, session→message→part hierarchy.
