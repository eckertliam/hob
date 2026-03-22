# Research: OpenCode Agent Harness Architecture

Based on opencode source at commit `13bac9c91` (effectify Pty service #18572), branch `dev`.

This is a deep analysis of how the opencode agent harness works, written for someone building an equivalent system in Emacs. It does NOT cover the terminal UI, HTTP server, or TypeScript-specific patterns. It covers algorithms, data flow, and design decisions.

## Reading order

Start with `00-overview.md` for the component map and data model. Then read the core docs (01-08) in order -- they build on each other. The reference docs (09-14) can be read as needed.

## Document map

### Core algorithms (must implement)

- `01-agent-loop.md` — The main `while(true)` loop. How it reads state from DB each iteration, checks exit conditions, handles subtasks/compaction, resolves tools, calls the LLM, and decides whether to continue or stop. Start here after the overview.
- `02-stream-processor.md` — Sits between the LLM stream and storage. Processes text deltas, tool calls, reasoning tokens. Contains the tool state machine (pending→running→completed/error), doom loop detection (same tool + same args 3x), retry logic with exponential backoff, and snapshot tracking.
- `03-tool-system.md` — How tools are defined (id, schema, execute function), registered, resolved per agent/model, and executed with permission checks and output truncation. The execution pipeline: validate → permission → before-hook → execute → after-hook → truncate.
- `04-permission-system.md` — Last-match-wins wildcard rule evaluation. Three actions: allow/deny/ask. The async approval flow using deferred promises. Cascade behavior on reject (all pending in session rejected). "Always" approval auto-resolves other pending requests.
- `05-compaction.md` — Two-phase context management. Phase 1: prune old tool outputs (cheap). Phase 2: summarize conversation history via a dedicated LLM call (expensive). Triggered automatically when tokens approach context limit. Includes the summarization prompt template.
- `06-provider-abstraction.md` — Wraps LLM APIs behind a uniform streaming interface. Provider-specific quirks: token counting differences (Anthropic excludes cached tokens, others include them), prompt caching markers, reasoning token budgets, tool call ID sanitization.
- `07-system-prompts.md` — Layered prompt assembly: base prompt (per provider) → environment context (directory, platform, date) → skills → instruction files (AGENTS.md, CLAUDE.md) → structured output instructions → plugin hooks. Different base prompts for different models.
- `08-retry-and-errors.md` — Error classification: context overflow → compact, rate limit → retry with backoff, auth error → stop. Exponential backoff: 2s × 2^attempt, capped at 30s. Respects Retry-After headers. No max retry limit (retries forever until abort).

### Data model

- `09-message-storage.md` — Schema: Session → Message → Part. Messages are user or assistant. Parts are the atomic units (text, tool call, reasoning, compaction marker, etc.). Event bus for real-time UI updates. Delta streaming (publish chunks without DB writes). Session forking as message copying.

### Reference patterns (useful, not all required)

- `10-built-in-tools.md` — Implementation details for core tools. Most important: the edit tool's 9-level fuzzy matching cascade (exact → whitespace-normalized → indentation-flexible → context-anchored). Also: bash tool's AST-based permission extraction, read-before-write invariant, batch tool for parallel execution, output truncation with file save.
- `11-subagents.md` — Child sessions with restricted permissions. Same loop, same tools, different agent config. Session resumption via task_id. Abort signal cascading from parent to child. Subagents share the parent's filesystem (not isolated).
- `12-extension-points.md` — Hook points throughout the loop: message-received, messages-transform, system-prompt-transform, llm-params, tool-before, tool-after, text-complete, compaction-prompt, permission-check. Sequential execution, mutation-based. Also covers skills (loadable markdown → system prompt).
- `13-snapshot-revert.md` — Git-based file change tracking using a separate repo with the same work tree. `git write-tree` for cheap snapshots, `git checkout {hash} -- {file}` for per-file revert. Handles three cases: modified (restore), created (delete), deleted (restore).
- `14-configuration.md` — Cascading config from 8 sources (remote → global → project → managed). Deep merge for objects, concatenation for arrays. Project discovery via git root commit hash. SQLite pragmas for concurrent access (WAL mode).
