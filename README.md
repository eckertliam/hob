# hob

A native Emacs AI coding agent. Elisp UI drives a Rust subprocess over
newline-delimited JSON on stdin/stdout.

**Status: early scaffolding.** The IPC protocol and subprocess lifecycle are
stubbed out. Nothing works end-to-end yet.

## Architecture

```
┌──────────────────────────────────────────┐
│              Emacs (lisp/)               │
│                                          │
│  hob.el          – entry points          │
│  hob-process.el  – subprocess lifecycle  │
│  hob-ipc.el      – JSON encode/decode    │
│  hob-ui.el       – *hob* buffer display  │
│                                          │
│  stdin/stdout: newline-delimited JSON    │
└───────────────────┬──────────────────────┘
                    │
┌───────────────────▼──────────────────────┐
│           hob-agent (agent/)             │
│                                          │
│  main.rs   – entry point, tracing setup  │
│  ipc.rs    – JSON IPC read/write         │
│  agent.rs  – agent loop (stub)           │
│  api.rs    – Anthropic client (stub)     │
│  tools/    – tool dispatch (stub)        │
└──────────────────────────────────────────┘
```

Communication: one JSON object per line. Emacs sends requests on stdin, the
agent sends responses on stdout. Stderr is for tracing logs only.

## What exists today

- **IPC types defined** (`ipc.rs`): `Request` (task, cancel, ping) and
  `Response` (token, tool_call, tool_result, done, error, pong). The read/write
  loop works but `task` returns a stub error.
- **Elisp subprocess lifecycle** (`hob-process.el`): start/stop/send, process
  filter that accumulates partial lines and dispatches complete JSON.
- **Elisp IPC layer** (`hob-ipc.el`): encodes outgoing requests, decodes and
  dispatches incoming responses to UI handlers.
- **Elisp UI** (`hob-ui.el`): `*hob*` buffer in `special-mode`, appends
  streaming tokens and tool call markers.
- **Agent/API/tools**: stub files that compile but do nothing.

## What needs to be built

See `research/` for detailed architecture docs (reverse-engineered from the
OpenCode agent harness). Read `research/00-overview.md` for the overview, then
01–08 for the core systems.

The major pieces, roughly in order:

1. **Anthropic streaming client** — SSE streaming from the Messages API,
   parsing text deltas and tool calls
2. **Agent loop** — the core `while(true)` that calls the LLM, processes tool
   calls, and decides whether to continue or stop
3. **Tool implementations** — read_file, write_file, shell, list_files (and
   eventually edit, glob, grep, batch)
4. **Permission system** — last-match-wins wildcard rules, async ask flow over
   IPC
5. **Message storage** — SQLite persistence for sessions/messages/parts
6. **Compaction** — prune old tool outputs, summarize when context fills up
7. **Error handling / retry** — classify errors, exponential backoff for rate
   limits, compaction on context overflow

## Building

```
make build        # cargo build --release
make byte-compile # emacs --batch byte-compile lisp/*.el
make install      # copy binary to ~/.local/bin
make clean
```

## Usage

Nothing works yet. Once the agent loop and API client are implemented:

```elisp
(setq hob-api-key "sk-ant-...")
M-x hob-start
M-x hob-task
```

## Project layout

```
├── agent/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs       – entry point, tracing setup
│       ├── ipc.rs        – JSON IPC protocol (implemented)
│       ├── agent.rs      – agent loop (stub)
│       ├── api.rs        – Anthropic client (stub)
│       └── tools/
│           └── mod.rs    – tool dispatch (stub)
├── lisp/
│   ├── hob.el           – package entry, defcustoms
│   ├── hob-process.el   – subprocess lifecycle
│   ├── hob-ipc.el       – JSON protocol
│   └── hob-ui.el        – output buffer rendering
├── research/             – architecture research notes
├── Makefile
└── .github/workflows/ci.yml
```

## License

TBD
