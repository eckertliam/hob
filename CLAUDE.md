# CLAUDE.md

## Project

hob is a native Emacs AI coding agent. Two halves:

- **`agent/`** — Rust binary (`hob-agent`). Multi-turn agent loop with tool
  execution, permission gating, error retry, context compaction, and SQLite
  persistence. Provider abstraction supports Anthropic (implemented) and
  OpenAI (interface ready). Communicates with Emacs over newline-delimited
  JSON on stdin/stdout.
- **`lisp/`** — Emacs Lisp package. Subprocess lifecycle, JSON IPC
  encode/decode, streaming output in `*hob*` buffer, and permission prompts.

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
- `anyhow::Result` for error propagation. `ClassifiedError` in `error.rs` for
  API errors that need to be matched on (retry vs bail).
- `tokio` for async. The IPC loop, agent loop, API client, and tools are all
  async.
- `tracing` for logging (`info!`, `debug!`, `error!`). Never write to stdout
  except IPC messages — stdout is the Emacs pipe.
- `serde` with `#[serde(tag = "type", rename_all = "snake_case")]` for all IPC
  message types so they serialize as `{"type": "token", ...}`.
- Tool implementations go in separate files under `agent/src/tools/`. Each tool
  exports `definition() -> ToolDef` and `execute(Value) -> Result<String>`.
  The registry in `tools/mod.rs` collects definitions and dispatches by name.
- Tool output is truncated at 50KB in `tools/mod.rs` after execution.
- Permission checks happen in the agent loop before tool execution, not in the
  tools themselves.

### Elisp (lisp/)

- `lexical-binding: t` in every file.
- Prefix all symbols with `hob-` (public) or `hob--` (internal).
- `defcustom` for user-facing config, `defvar` for internal state.
- The `*hob*` buffer uses `special-mode` (read-only). Mutations go through
  `let ((inhibit-read-only t))`.
- Process filter accumulates partial lines in `hob--output-buffer`, splits on
  `\n`, dispatches complete JSON lines to `hob-ipc-dispatch`.

## Source files

### agent/src/

| File | Purpose |
|------|---------|
| `main.rs` | Entry point: parse env vars, create provider + store, run IPC loop |
| `agent.rs` | Multi-turn agent loop: stream → tools → re-prompt → compaction |
| `api/mod.rs` | Provider trait, StreamEvent, Message, ContentBlock, ToolDef |
| `api/anthropic.rs` | Anthropic SSE → StreamEvent, classified error handling |
| `api/sse.rs` | Shared SSE parser for Anthropic and OpenAI |
| `ipc.rs` | JSON IPC: Request/Response enums, stdin/stdout, task spawning |
| `prompt.rs` | Layered system prompt: base + environment + .hob.md files |
| `error.rs` | Error classification, exponential backoff, retry logic |
| `permission.rs` | Wildcard rule evaluation, async ask flow, cascade |
| `compaction.rs` | Prune old tool outputs, summarize via LLM, compact |
| `store.rs` | SQLite session/message persistence (WAL mode) |
| `tools/mod.rs` | Tool registry, dispatch, output truncation |
| `tools/read_file.rs` | Read with line numbers, offset/limit |
| `tools/write_file.rs` | Write with mkdir -p |
| `tools/edit_file.rs` | Find-and-replace with 4-level fuzzy cascade |
| `tools/shell.rs` | sh -c with timeout and cancel |
| `tools/list_files.rs` | Directory listing |
| `tools/glob.rs` | ripgrep --files --glob |
| `tools/grep.rs` | ripgrep search |

## Current IPC protocol

All messages are single-line JSON with a `"type"` field.

### Emacs → agent (Request)

| type | fields | purpose |
|------|--------|---------|
| `task` | `id`, `prompt` | start agent task |
| `cancel` | `id` | cancel in-flight task |
| `permission_response` | `request_id`, `decision` | answer permission ask |
| `ping` | | health check |

### Agent → Emacs (Response)

| type | fields | purpose |
|------|--------|---------|
| `token` | `id`, `content` | streaming text |
| `tool_call` | `id`, `tool`, `input` | tool being executed |
| `tool_result` | `id`, `tool`, `output` | tool finished |
| `permission_request` | `id`, `request_id`, `tool`, `resource` | needs user approval |
| `status` | `id`, `message` | retry/status feedback |
| `done` | `id` | task complete |
| `error` | `id`, `message` | error or cancellation |
| `pong` | | health check reply |

## Research

`research/` contains architecture docs reverse-engineered from OpenCode.
Read `00-overview.md` first, then 01–08 for core systems.
