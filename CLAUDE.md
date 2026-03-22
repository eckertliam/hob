# CLAUDE.md

## Project

hob is a native Emacs AI coding agent, early stage. Two halves:

- **`agent/`** — Rust binary (`hob-agent`). Currently has IPC message types and
  a read/write loop over stdin/stdout. Agent loop, API client, tools, and
  storage are all stubs.
- **`lisp/`** — Emacs Lisp package. Subprocess lifecycle, JSON IPC
  encode/decode, and a `*hob*` output buffer. All implemented, but there's no
  working agent to drive yet.

## Build & test

```bash
make build                                    # cargo build --release
make byte-compile                             # emacs --batch byte-compile lisp/*.el
cargo check --manifest-path agent/Cargo.toml  # type-check without full build
cargo test --manifest-path agent/Cargo.toml   # run rust tests
```

## Code conventions

### Rust (agent/)

- Rust 2021 edition, stable toolchain.
- `anyhow::Result` for error propagation. Define domain error types only when
  callers need to match on variants.
- `tokio` for async. The IPC loop is async; the agent loop and API client will
  be too.
- `tracing` for logging (`info!`, `debug!`, `error!`). Never write to stdout
  except IPC messages — stdout is the Emacs pipe.
- `serde` with `#[serde(tag = "type", rename_all = "snake_case")]` for all IPC
  message types so they serialize as `{"type": "token", ...}`.
- Tool implementations go in separate files under `agent/src/tools/`.

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
| `task` | `id`, `prompt` | parsed, returns stub error |
| `cancel` | `id` | parsed, logged, no-op |
| `ping` | | works, replies with pong |

### Agent → Emacs (Response)

| type | fields | status |
|------|--------|--------|
| `token` | `id`, `content` | defined, never sent yet |
| `tool_call` | `id`, `tool`, `input` | defined, never sent yet |
| `tool_result` | `id`, `tool`, `output` | defined, never sent yet |
| `done` | `id` | defined, never sent yet |
| `error` | `id`, `message` | sent as stub reply to task |
| `pong` | | works |

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
