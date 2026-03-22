# Roadmap

Based on architecture research in `research/` (reverse-engineered from the
OpenCode agent harness). Phases are ordered by dependency — each builds on the
previous.

## Phase 1: Single-turn streaming

The minimum to see something work. Provider abstraction layer with Anthropic
SSE streaming, wired into the agent loop for one LLM call, streaming tokens
back to Emacs over IPC. No tools, no storage, no permissions. Messages live in
memory. At the end of this phase: `M-x hob-task`, type a question, see a
streaming response in the `*hob*` buffer.

- [x] Provider abstraction (`api/mod.rs`): `StreamEvent` enum, `Provider` trait
- [x] SSE parser (`api/sse.rs`): shared between providers
- [x] Anthropic provider (`api/anthropic.rs`): SSE → StreamEvent translation
- [x] Parse `content_block_delta` events for text and tool input
- [x] Agent loop: single-turn (call LLM once, stream tokens, done)
- [x] IPC: send `token` messages as they arrive, `done` when finished
- [x] Basic system prompt (working directory, platform)
- [x] Emacs passes `ANTHROPIC_API_KEY` and `HOB_MODEL` via process environment

## Phase 2: Tool loop

This is where it becomes an agent. The core `while(true)` loop: call the LLM,
if it wants tools execute them and re-prompt, if it says stop then break.

- [x] Tool registry with JSON schemas and dispatch (`tools/mod.rs`)
- [x] `read_file` tool: line numbers, offset/limit, line truncation
- [x] `shell` tool: spawns `sh -c`, timeout, cancel via kill_on_drop
- [x] `list_files` tool: sorted entries, directory markers, 500 entry cap
- [x] Agent loop: `while(true)` — stream response, accumulate tool calls,
      execute on `ToolUse` stop reason, append results, re-prompt
- [x] Tool call accumulation: tracks pending calls by stream index,
      parses partial JSON on `ToolStop`
- [x] IPC: sends `tool_call` and `tool_result` messages to Emacs
- [x] Cancel: kills in-flight API stream + running shell processes
- [x] Output truncation: 50KB cap on tool output

## Phase 3: Persistence

Simplified SQLite persistence — snapshot-based message storage rather than
full DB-driven loop.

- [x] SQLite schema: `sessions` and `messages` tables
- [x] WAL mode + pragmas (busy_timeout, synchronous, foreign_keys)
- [x] Session create/list/delete
- [x] Save full message history as JSON blob on task completion
- [x] Load messages for session resume (API ready, UI not yet wired)
- [x] Store threaded through IPC loop into spawned agent tasks

## Phase 4: Error handling and retry

- [x] ClassifiedError with ApiErrorKind enum
- [x] Error classification: context overflow, rate limit, auth, overloaded
- [x] Exponential backoff (2s × 2^n, cap 30s), respects Retry-After headers
- [x] `status` IPC message for retry feedback in Emacs
- [x] Cancellable retry sleep (cancel token interrupts wait)
- [x] Anthropic provider returns ClassifiedError on HTTP errors

## Phase 5: Permissions

- [x] Last-match-wins wildcard rule evaluation (*, ? patterns)
- [x] Three actions: allow, deny, ask
- [x] Default rules: allow reads, ask for bash/edit
- [x] permission_request / permission_response IPC messages
- [x] Async ask: oneshot channels block tool until user responds
- [x] Cascade on reject: all pending permissions denied
- [x] "Always" adds session-level allow rule
- [x] Emacs UI: read-char-choice (y=once, !=always, n=reject)

## Phase 6: More tools

- [x] `write_file`: creates parent dirs, overwrites existing
- [x] `edit_file`: 4-level fuzzy match cascade (exact, whitespace-normalized,
      indentation-flexible, context-anchored)
- [x] `glob`: ripgrep with find fallback, 100 result cap
- [x] `grep`: ripgrep with grep fallback, 100 match cap
- [x] Output truncation applied to all tools (50KB cap)

## Phase 7: Compaction

- [x] Phase 1 prune: clear old tool outputs, protect last 2 user turns
- [x] Phase 2 summarize: LLM call with structured template
- [x] Auto-trigger when input tokens approach context limit
- [x] compact() replaces history with summary + last user replay
- [x] Model context limit map (Claude 200K, GPT-4 128K, default 200K)

## Phase 8: System prompt assembly

- [x] Base prompt with behavioral guidelines
- [x] Environment context (model, cwd, platform, shell, git repo/branch)
- [x] Project instruction files (.hob.md searched upward from cwd)
- [ ] Prompt caching markers (deferred — optimization, not correctness)

## Future work

- **Batch tool**: parallel tool execution (up to 25 at once)
- **Subagents**: child sessions with restricted permissions
- **Snapshot/revert**: git-based file change tracking
- **Multi-session UI**: session switching, history browsing
- **Read-before-write invariant**: track which files have been read
- **AST-based bash permissions**: tree-sitter command parsing
- **Prompt caching**: Anthropic cache_control on stable content
- **Session resume UI**: Emacs-side session list and selection
