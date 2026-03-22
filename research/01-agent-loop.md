# The Main Agent Loop

**Source**: `packages/opencode/src/session/prompt.ts`

This is the heart of the harness. Everything else exists to serve this loop.

## Entry Point

The public API is `SessionPrompt.prompt()`. It:
1. Creates a user message with parts (text, files, agent references) in the DB
2. Touches the session timestamp
3. Calls `SessionPrompt.loop()`

## Concurrency: One Loop Per Session

```
SessionPrompt.loop(sessionID, resume_existing?)
  │
  ├─ If resume_existing: get existing AbortSignal
  │  Else: try to create new AbortController
  │
  ├─ If session already running (start() returns undefined):
  │     Queue this caller in callbacks array
  │     Return a Promise that resolves when the running loop finishes
  │
  └─ Otherwise: we own the loop
       Register defer(cancel) for guaranteed cleanup
```

The state is stored per-session:
```
state[sessionID] = {
  abort: AbortController,
  callbacks: [{ resolve, reject }]
}
```

Key invariant: **one active loop per session at any time**. Additional
callers get queued and resolved with the same final result.

## The Loop: Iteration by Iteration

```
let step = 0
let structuredOutput = undefined

while (true) {
  // ──── PHASE 1: READ STATE ────
  set session status → "busy"
  if abort.aborted → break

  msgs = filterCompacted(MessageV2.stream(sessionID))

  // Scan backward through messages to find:
  // - lastUser: most recent user message
  // - lastAssistant: most recent assistant message
  // - lastFinished: most recent assistant with a finish reason
  // - tasks[]: pending CompactionParts or SubtaskParts

  // ──── PHASE 2: CHECK EXIT ────
  if lastAssistant.finish exists
     AND finish ∉ {"tool-calls", "unknown"}
     AND lastAssistant is newer than lastUser:
    → break (model is done)

  step++
  if step == 1 → fire async title generation

  // ──── PHASE 3: HANDLE PENDING TASKS ────
  task = tasks.pop()

  if task.type == "subtask":
    → Execute subagent inline (see Subtask Handling below)
    → continue

  if task.type == "compaction":
    → Run SessionCompaction.process()
    → If result == "stop": break
    → continue

  // ──── PHASE 4: AUTO-COMPACTION CHECK ────
  if lastFinished exists
     AND lastFinished.summary != true
     AND tokens exceed context window:
    → Create CompactionPart
    → continue (will process next iteration)

  // ──── PHASE 5: NORMAL PROCESSING ────
  agent = Agent.get(lastUser.agent)
  maxSteps = agent.steps ?? Infinity
  isLastStep = step >= maxSteps

  // Insert plan/build mode reminders
  msgs = insertReminders(msgs, agent, session)

  // Create new assistant message in DB
  processor = SessionProcessor.create(newAssistantMessage)

  // Resolve available tools
  tools = resolveTools(agent, session, model)
  if JSON schema requested → inject StructuredOutput tool

  // Plugin hook: transform messages
  Plugin.trigger("experimental.chat.messages.transform", {}, msgs)

  // Build system prompt
  system = [
    ...SystemPrompt.environment(model),
    ...skills,
    ...InstructionPrompt.system()
  ]

  // Convert messages to LLM format
  modelMessages = MessageV2.toModelMessages(msgs, model)
  if isLastStep → append MAX_STEPS reminder as assistant content

  // ──── PHASE 6: CALL LLM ────
  result = processor.process(system, modelMessages, tools, model)

  // ──── PHASE 7: HANDLE RESULT ────
  if structuredOutput was captured → save, break
  if result == "stop" → break
  if result == "compact" → create CompactionPart, continue
  continue → next iteration (tool-calls need processing)
}

// ──── PHASE 8: CLEANUP ────
SessionCompaction.prune(sessionID)
Resolve all queued callbacks with final assistant message
return finalMessage
```

## Exit Conditions (When Does the Loop Stop?)

| Condition | How Detected | Result |
|-----------|-------------|--------|
| Model says "stop" | `lastAssistant.finish == "stop"` | Natural completion |
| Max tokens reached | `lastAssistant.finish == "length"` | Truncated |
| Content filter | `lastAssistant.finish == "content-filter"` | Blocked |
| Structured output captured | `structuredOutput !== undefined` | JSON result |
| Processor returns "stop" | Permission rejection or blocked | Halted |
| Abort signal fired | `abort.aborted` checked at loop top | Cancelled |
| Compaction returns "stop" | Compaction decides to halt | Compacted & stopped |
| Non-retryable error | Error thrown, caught by defer cleanup | Error exit |

The loop **continues** when:
- `lastAssistant.finish == "tool-calls"` (model wants to use more tools)
- `lastAssistant.finish == "unknown"` (unclear state, keep going)
- Processor returns `"continue"` (normal step completion)
- Processor returns `"compact"` (create compaction, loop back)

## Subtask Handling (Subagents)

When a SubtaskPart is found in the pending tasks:

```
1. Look up the subagent configuration (e.g., "explore", "general")
2. Create a new assistant message tagged with subagent name
3. Create a ToolPart in "running" state
4. Build tool context with:
   - The subagent's permissions (merged with session permissions)
   - abort signal (cascading from parent)
   - ask() for permission requests
5. Execute the Task tool with the subtask's prompt
6. On success: update ToolPart to "completed" with output
   On failure: update ToolPart to "error" with message
7. If subtask had a command:
   Create synthetic user message: "Summarize the task tool output
   above and continue with your task."
8. continue → loop picks up the result in next iteration
```

The key insight: subtasks execute **inline within the parent loop**, not as
separate processes. They use the same session, same message history, same
abort controller. The subagent's output becomes a tool result that the parent
agent can read.

## Step Counter

- Incremented at the start of each loop iteration
- On step 1: fires async title generation for the session
- Compared against `agent.steps` (max steps config, defaults to Infinity)
- When `isLastStep`:
  - A MAX_STEPS reminder is appended to the messages as synthetic assistant
    content, telling the model it's on its last step and should wrap up
  - The model gets one more chance to produce a final response

## AbortController Cascade

```
HTTP cancel request
      │
      ▼
SessionPrompt.cancel(sessionID)
      │
      ├─ abort.abort()  ──→  propagates to:
      │                       ├─ LLM streaming (fetch aborted)
      │                       ├─ Tool execution (ctx.abort signal)
      │                       └─ Permission.ask (deferred rejected)
      │
      ├─ delete state[sessionID]
      └─ set status → "idle"
```

The `defer(() => cancel(sessionID))` at loop entry guarantees cleanup even
on unhandled exceptions.

## Message Wrapping on Multi-Step

When `step > 1` and there are unaddressed user messages after the last
finished assistant message, those user messages get wrapped:

```xml
<system-reminder>
The user sent the following message:
{original text}

Please address this message and continue with your tasks.
</system-reminder>
```

This ensures the model notices follow-up messages that arrived while it was
processing tools.

## Callback Queue Resolution

When the loop finally breaks:

```
1. Prune old compactions
2. Stream messages from session
3. Find the final assistant message
4. For each queued callback: resolve(finalMessage)
5. Return finalMessage
```

All callers who were queued while the loop was running receive the same
result. This is a fan-out pattern: N callers, 1 execution, N identical
responses.
