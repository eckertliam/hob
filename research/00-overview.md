# Agent Harness Architecture

Research on how the OpenCode agent harness works, distilled to what's
relevant for building an Emacs-based equivalent. Excludes: HTTP server
layer, terminal UI, TypeScript-specific patterns, git worktree isolation.

## What the Harness Does

Connects to an LLM, gives it tools (file I/O, shell, search), and runs
an agentic loop: the model calls tools, observes results, and continues
until the task is done.

## Component Map

```
┌────────────────────────────────────────────────────────┐
│                    AGENT LOOP (01)                      │
│                                                        │
│  while true:                                           │
│    load messages from storage                          │
│    check exit conditions                               │
│    handle pending subtasks / compaction                 │
│    resolve tools + build system prompt                  │
│    call LLM via streaming                              │
│    process events → persist parts                      │
│    if tool-calls → continue; if stop → break           │
│                                                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │   Stream     │  │    Tool      │  │  Permission  │ │
│  │ Processor(02)│  │  System (03) │  │  System (04) │ │
│  └──────────────┘  └──────────────┘  └──────────────┘ │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │ Compaction   │  │  Provider    │  │   System     │ │
│  │   (05)       │  │ Abstraction  │  │  Prompts(07) │ │
│  │              │  │   (06)       │  │              │ │
│  └──────────────┘  └──────────────┘  └──────────────┘ │
│  ┌──────────────┐  ┌──────────────┐                   │
│  │ Retry/Error  │  │   Message    │                   │
│  │   (08)       │  │ Storage (09) │                   │
│  └──────────────┘  └──────────────┘                   │
└────────────────────────────────────────────────────────┘
```

## Core Data Model

```
Session
  └─ Messages[]
       └─ Parts[]
            ├─ TextPart         (LLM text output)
            ├─ ToolPart         (tool call with state machine)
            ├─ ReasoningPart    (extended thinking)
            ├─ CompactionPart   (history summarized here)
            ├─ SubtaskPart      (delegated to subagent)
            ├─ StepStartPart    (snapshot before LLM step)
            ├─ StepFinishPart   (snapshot + tokens + cost)
            └─ PatchPart        (files changed in step)

Tool State Machine:
  pending → running → completed | error
```

## Key Design Decisions

**Persistence-first**: Every streaming event persisted immediately. Crash
recovery, real-time UI updates, audit trail.

**Loop reads state from DB each iteration**: No in-memory state across
iterations. The loop is a state machine driven by the message history.

**One loop per session**: Concurrent requests queue and resolve with the
same result. No parallel loops.

**Tool calls stay in the loop**: Tools execute inline. Results fed back
to the LLM automatically. Loop continues on "tool-calls" finish reason.

**Abort signal cascades**: One signal propagates from user cancel through
LLM streaming, tool execution, and subagent sessions.

## Documents

### Core (you must implement these)

| # | Document | What |
|---|----------|------|
| 01 | [Agent Loop](01-agent-loop.md) | Main loop control flow, exit conditions, step counting |
| 02 | [Stream Processor](02-stream-processor.md) | Event processing, tool state machine, doom loop detection |
| 03 | [Tool System](03-tool-system.md) | Tool definition, registry, execution pipeline |
| 04 | [Permission System](04-permission-system.md) | Wildcard matching, async approval, cascade behavior |
| 05 | [Compaction](05-compaction.md) | Pruning, summarization, context window management |
| 06 | [Provider Abstraction](06-provider-abstraction.md) | LLM streaming, token counting, prompt caching |
| 07 | [System Prompts](07-system-prompts.md) | Layered prompt assembly |
| 08 | [Retry and Errors](08-retry-and-errors.md) | Error classification, exponential backoff |
| 09 | [Message Storage](09-message-storage.md) | Data model, event bus, session forking |

### Reference (useful patterns, not all required)

| # | Document | What |
|---|----------|------|
| 10 | [Built-in Tools](10-built-in-tools.md) | Edit cascade, bash AST parsing, output truncation |
| 11 | [Subagents](11-subagents.md) | Child sessions, restricted permissions, abort cascading |
| 12 | [Extension Points](12-extension-points.md) | Hook points for extensibility |
| 13 | [Snapshot/Revert](13-snapshot-revert.md) | Git-based file change tracking and undo |
| 14 | [Configuration](14-configuration.md) | Cascading config loading |
