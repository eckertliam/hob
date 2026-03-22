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

SQLite message storage. The agent loop switches from in-memory to reading state
from the DB each iteration — no in-memory state carried across iterations.

- [ ] SQLite schema: `sessions`, `messages`, `parts` tables
- [ ] WAL mode + pragmas for concurrent access
- [ ] Agent loop reads messages from DB at start of each iteration
- [ ] Stream processor persists parts as they arrive
- [ ] Session create/list/delete
- [ ] Delta streaming: publish token chunks to Emacs without DB writes,
      persist only on text completion

## Phase 4: Error handling and retry

Classify API errors and handle them correctly instead of crashing.

- [ ] Error classification: context overflow, rate limit, auth, server error
- [ ] Exponential backoff for rate limits (2s x 2^n, cap 30s)
- [ ] Respect `Retry-After` headers
- [ ] `status` IPC message so Emacs shows "retrying in 8s..."
- [ ] Context overflow triggers compaction (or error until phase 7)
- [ ] Cancellable retry sleep (abort signal interrupts the wait)

## Phase 5: Permissions

Safety layer between the LLM's tool calls and actual execution.

- [ ] Permission rule evaluator (last-match-wins wildcard matching)
- [ ] Three actions: allow, deny, ask
- [ ] `permission_request` / `permission_response` IPC messages
- [ ] Emacs-side permission UI (child frame or minibuffer prompt)
- [ ] Cascade: reject one pending permission → reject all pending in session
- [ ] "Always" approval auto-resolves matching pending requests
- [ ] Default rules: allow reads, ask for writes and shell commands

## Phase 6: More tools

Flesh out the tool set now that the infrastructure is solid.

- [ ] `write_file` with read-before-write invariant
- [ ] `edit_file` with fuzzy match cascade (exact → whitespace-normalized →
      indentation-flexible → context-anchored)
- [ ] `glob` (shell out to ripgrep, sorted by mtime)
- [ ] `grep` (ripgrep, grouped by file with line numbers)
- [ ] Output truncation for all tools (~50KB cap, save full to disk)
- [ ] `batch` tool for parallel execution (up to 25 tools)

## Phase 7: Compaction

Context window management for long sessions.

- [ ] Phase 1 prune: clear old tool outputs, replace with
      "[Old tool result cleared]"
- [ ] Phase 2 summarize: LLM call with structured template
      (Goal, Instructions, Discoveries, Accomplished, Files)
- [ ] Auto-trigger when tokens approach context limit
- [ ] `filterCompacted`: only send messages from latest compaction boundary
      forward to the model
- [ ] Protected content: never prune instruction/skill outputs

## Phase 8: System prompt assembly

Full layered prompt construction.

- [ ] Base prompt (agent identity, behavioral guidelines)
- [ ] Environment context (cwd, platform, git status, date)
- [ ] Project instruction files (`.hob.md` or `AGENTS.md` in project root)
- [ ] Prompt caching markers (Anthropic `cache_control` on stable content)

## Open questions

- **Phases 1+2 as one phase?** The agent loop structure is fundamentally about
  tool calls. A single-turn-only loop that gets retrofitted with tool handling
  might be throwaway work. Could jump straight to "streaming + tool loop" as
  one phase.
- **Subagents**: child sessions with restricted permissions and tool sets.
  Useful but not required for a working agent. Implement after the core is
  solid.
- **Snapshot/revert**: git-based file change tracking using a separate repo
  with the same worktree. Nice for undo but a lot of machinery. Defer until
  the tool system is proven.
- **Multi-session UI**: session switching, history browsing, forking. The
  storage layer supports it from phase 3 but the Elisp UI would need work.
