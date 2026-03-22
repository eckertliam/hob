# CLAUDE.md

## Project

hob is a terminal AI coding agent. Single Rust binary with a ratatui TUI.

- `agent/src/` — Everything: TUI, agent loop, provider abstraction, tools,
  permissions, compaction, storage.
- `research/` — Architecture research (reverse-engineered from OpenCode).

## Build & test

```bash
make build                                    # cargo build --release
cargo check --manifest-path agent/Cargo.toml  # type-check
cargo test --manifest-path agent/Cargo.toml   # run tests
```

## Testing requirements

**Every code change must include tests.** This is mandatory, not optional.

- Unit tests go in `#[cfg(test)] mod tests` at the bottom of each file.
- Test behavior, not implementation.
- Descriptive test names: `test_read_file_returns_error_for_missing_path`.
- Always run `cargo test --manifest-path agent/Cargo.toml` before finishing.

**Provider neutrality**: Never hardcode a single LLM provider in defaults, config
names, or documentation. Abstractions must have at least two implementations to
be considered complete. When in doubt, use generic names (`HOB_API_KEY`, not
`ANTHROPIC_API_KEY`).

## Code conventions

- Rust 2021 edition, stable toolchain.
- `anyhow::Result` for errors. `ClassifiedError` for API errors that need
  matching (retry vs bail).
- `tokio` for async. Agent loop, API client, tools all async.
- `tracing` for logging to `/tmp/hob.log`. Never write to stdout/stderr
  directly — the TUI owns the terminal.
- Tool implementations in separate files under `agent/src/tools/`. Each exports
  `definition() -> ToolDef` and `execute(Value) -> Result<String>`.
- Tool output truncated at 50KB in `tools/mod.rs`.
- Permission checks in the agent loop, not in tools.

## Source files

| File | Purpose |
|------|---------|
| `main.rs` | Entry: detect provider, open store, launch TUI |
| `tui.rs` | Ratatui terminal UI: chat, input, status, permissions |
| `events.rs` | Channel-based event system (agent ↔ TUI) |
| `agent.rs` | Multi-turn agent loop: stream → tools → re-prompt |
| `api/mod.rs` | Provider trait, StreamEvent, Message, ContentBlock |
| `api/anthropic.rs` | Anthropic SSE → StreamEvent |
| `api/openai.rs` | OpenAI SSE → StreamEvent |
| `api/sse.rs` | Shared SSE parser |
| `prompt.rs` | Layered system prompt: base + environment + .hob.md |
| `error.rs` | Error classification, exponential backoff |
| `permission.rs` | Wildcard rule evaluation, async ask flow |
| `compaction.rs` | Prune old tool outputs, summarize via LLM |
| `store.rs` | SQLite session/message persistence |
| `tools/mod.rs` | Tool registry, dispatch, truncation |
| `tools/*.rs` | Individual tools (read, write, edit, shell, etc.) |
