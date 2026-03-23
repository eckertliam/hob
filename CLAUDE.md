# CLAUDE.md

## Project

hob is a terminal AI coding agent. Single Rust binary with a ratatui TUI.

Standard Rust project layout: `Cargo.toml` at root, source in `src/`.
`research/` has architecture reference docs from OpenCode.

## Build & test

```bash
cargo build --release
cargo test
cargo run --release
```

## Working principles

**Search before assuming.** If you're unsure about something factual — current
model names, API formats, library APIs, version numbers — do a web search. Do
not guess or rely on potentially stale training data. This applies to anything
that changes over time.

## Testing requirements

**Every code change must include tests.** This is mandatory, not optional.

- Unit tests go in `#[cfg(test)] mod tests` at the bottom of each file.
- Test behavior, not implementation.
- Descriptive test names: `test_read_file_returns_error_for_missing_path`.
- Always run `cargo test` before finishing.

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
- Tool implementations in separate files under `src/tools/`. Each exports
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
| `models.rs` | Known model definitions, context limits |
| `config.rs` | Persistent config (~/.config/hob/config.json) |
| `prompt.rs` | Layered system prompt: base + environment + .hob.md |
| `error.rs` | Error classification, exponential backoff |
| `permission.rs` | Wildcard rule evaluation, async ask flow |
| `compaction.rs` | Prune old tool outputs, summarize via LLM |
| `store.rs` | SQLite session/message persistence |
| `snapshot.rs` | Git-based file change tracking and undo |
| `theme.rs` | Color themes for the TUI |
| `tools/mod.rs` | Tool registry, dispatch, truncation |
| `tools/*.rs` | Individual tools (read, write, edit, shell, etc.) |
