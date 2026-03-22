# Subagents

## Subagent Architecture

Subagents are child sessions that execute within the parent's loop:

```
Parent Session (build agent)
    │
    │  LLM calls task tool
    │
    ▼
Task Tool creates child session
    │
    ├─ parentID = parent session
    ├─ agent = "explore" or "general"
    ├─ directory = SAME as parent (shared filesystem)
    ├─ permissions = restricted (no todo, optionally no nested tasks)
    │
    ├─ SessionPrompt.prompt() runs the child
    │   ├─ Child gets its own system prompt
    │   ├─ Child gets restricted tool set
    │   ├─ Child runs its own loop iterations
    │   └─ Child produces a response
    │
    ▼
Result returned to parent
    ├─ Output wrapped in <task_result> tags
    ├─ Parent agent sees it as tool output
    └─ Child session preserved (can be resumed via task_id)
```

## Built-In Subagent Types

### "explore" Agent
- Purpose: Code search and codebase analysis
- Tools: grep, glob, read, bash, webfetch, websearch, codesearch
- Key restriction: **NO write/edit permissions**
- System prompt emphasizes thoroughness levels

### "general" Agent
- Purpose: General multi-step task execution
- Tools: most tools except todo operations
- Can read and write files
- More permissive than explore

## Agent Modes

```
"primary"  → user-facing (build, plan)
"subagent" → called via task tool (explore, general)
"all"      → available everywhere (compaction, title, summary)
```

The task tool only lists agents with `mode: "subagent"`. Primary agents
cannot be launched as subagents.

## Subagent Permissions

Child sessions get restricted permissions:

```
Permission ruleset for child:
  todowrite: deny      ← no todo list modification
  todoread: deny       ← no todo list reading

  If parent agent doesn't have "task" permission:
    task: deny         ← prevent recursive subagent spawning

  If agent has experimental.primary_tools:
    Those tools also allowed
```

This prevents:
- Subagents modifying shared todo state
- Infinite subagent recursion (unless explicitly allowed)

## Session Resumption

Subagent sessions can be resumed:

```
First call:
  task({ prompt: "...", subagent_type: "explore" })
  → Creates session, returns task_id: "session_abc123"

Later call (same parent session):
  task({ prompt: "continue searching...", task_id: "session_abc123" })
  → Resumes existing session with full history
  → New message added to existing child session
```

This allows multi-turn subagent conversations without losing context.

## Abort Signal Cascading

```
Parent abort → child abort

Implementation in task.ts:
  const listener = () => SessionPrompt.cancel(session.id)
  ctx.abort.addEventListener("abort", listener)
  // ... execute ...
  ctx.abort.removeEventListener("abort", listener)
```

When the parent session is cancelled, the child is cancelled too.
