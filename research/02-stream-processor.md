# Stream Processor

**Source**: `packages/opencode/src/session/processor.ts`

The stream processor sits between the LLM SDK and the persistence layer. It
consumes a stream of events from the LLM, persists each event as a Part in
the database, publishes real-time deltas to the UI, and handles errors/retries.

## Architecture

```
LLM.stream()
    │
    │  yields: text-delta, tool-call, tool-result, finish-step, error, ...
    │
    ▼
SessionProcessor.process()
    │
    ├─ for await (event of stream.fullStream)
    │     switch (event.type) {
    │       case "text-start":       → create TextPart
    │       case "text-delta":       → append to TextPart, publish delta
    │       case "text-end":         → finalize TextPart, run plugin hook
    │       case "reasoning-start":  → create ReasoningPart
    │       case "reasoning-delta":  → append, publish delta
    │       case "reasoning-end":    → finalize ReasoningPart
    │       case "tool-input-start": → create ToolPart (pending)
    │       case "tool-call":        → transition to running, execute tool
    │       case "tool-result":      → transition to completed
    │       case "tool-error":       → transition to error
    │       case "start-step":       → capture snapshot
    │       case "finish-step":      → record tokens/cost, compute patch
    │       case "error":            → throw (caught by outer handler)
    │     }
    │
    └─ Returns "continue" | "stop" | "compact"
```

## The Processing Loop

The processor wraps the stream consumption in a `while (true)` loop to
support retries:

```
let attempt = 0
let needsCompaction = false
let blocked = false
let snapshot = undefined

while (true) {
  try {
    stream = LLM.stream(...)
    for await (event of stream.fullStream) {
      if (abort.aborted) throw abort.reason
      // ... handle event (see switch above)
    }
    // Stream finished normally
    break

  } catch (error) {
    if (ContextOverflowError):
      needsCompaction = true
      break

    if (retryable error):
      attempt++
      delay = exponentialBackoff(attempt)
      set status → "retry" with next attempt time
      sleep(delay)
      continue  // retry the while loop

    else:
      // Non-retryable: store error, publish, break
      assistantMessage.error = error
      break
  }

  // After stream ends, clean up incomplete tool parts
  for each part in message:
    if part.type == "tool" and not completed/error:
      mark as error: "Tool execution aborted"

  // Return decision
  if needsCompaction → return "compact"
  if blocked → return "stop"
  return "continue"
}
```

## Tool Call State Machine

Each tool call transitions through strict states:

```
                    tool-input-start
                         │
                         ▼
                    ┌─────────┐
                    │ PENDING  │  input = {}, raw = ""
                    └────┬────┘
                         │  tool-call (full args parsed)
                         ▼
                    ┌─────────┐
                    │ RUNNING  │  input = parsed args, time.start set
                    └────┬────┘
                         │
              ┌──────────┴──────────┐
              │                     │
         tool-result           tool-error
              │                     │
              ▼                     ▼
        ┌───────────┐        ┌─────────┐
        │ COMPLETED  │        │  ERROR   │
        │            │        │          │
        │ output     │        │ error    │
        │ title      │        │ message  │
        │ metadata   │        │          │
        │ time.end   │        │ time.end │
        │ attachments│        └──────────┘
        └────────────┘
```

**Key invariants:**
- Transitions are one-directional (no going back)
- Only transitions from "running" to completed/error (checked explicitly)
- Tool calls are tracked by ID in a `toolcalls: Record<string, ToolPart>` map
- Removed from map after completion/error
- Incomplete tool parts are cleaned up at end of stream

## Text Accumulation

```
text-start:
  currentText = new TextPart { text: "", time.start }
  persist to DB

text-delta:
  currentText.text += delta.text
  publish PartDelta event (for real-time streaming to UI)

text-end:
  currentText.text = text.trimEnd()
  run Plugin.trigger("experimental.text.complete") → may modify text
  set time.end
  persist final text to DB
  currentText = undefined
```

Only one TextPart active at a time. Deltas are streamed to clients via bus
events without waiting for the full text to complete.

## Reasoning Token Handling

Same pattern as text, but tracked separately:

```
reasoningMap: Record<providerID, ReasoningPart> = {}

reasoning-start:
  if already in map → skip (deduplicate)
  create new ReasoningPart { text: "" }
  store in reasoningMap[provider_id]

reasoning-delta:
  part.text += delta.text
  publish PartDelta event

reasoning-end:
  trim, set time.end, persist
  delete from reasoningMap
```

Reasoning parts are stored as separate Parts from text parts, allowing the
UI to show/hide thinking content independently.

## Doom Loop Detection

**Algorithm**: After each tool call transitions to "running", check for
repetition:

```
DOOM_LOOP_THRESHOLD = 3

parts = MessageV2.parts(currentAssistantMessage.id)
lastThree = parts.slice(-3)

if lastThree.length == 3
   AND all are ToolParts
   AND all call the same tool name
   AND all have identical JSON-stringified input
   AND none are pending:

  → Request "doom_loop" permission from user
  → If user rejects: throws RejectedError → tool marked as error
  → If user approves: continue (model might have a reason)
```

This catches the common failure mode where the model keeps calling the same
tool with the same arguments, getting the same result, and trying again
endlessly.

## Snapshot / Diff Tracking

Each LLM reasoning step is bookended by snapshots:

```
start-step:
  snapshot = Snapshot.track()  // git add . && git write-tree → hash
  create StepStartPart { snapshot: hash }

finish-step:
  newSnapshot = Snapshot.track()
  create StepFinishPart { snapshot: newSnapshot, tokens, cost }

  if old snapshot exists:
    patch = Snapshot.patch(oldSnapshot)
    // git diff --name-only oldHash → list of changed files
    if any files changed:
      create PatchPart { hash, files: [changed paths] }
  clear snapshot
```

The snapshot system uses a **separate git repository** (not the user's repo)
in `~/.opencode/data/snapshot/{projectID}`. This allows tracking changes
without polluting the user's git history.

**Purpose**: Enables the "revert" feature -- any step's changes can be
undone by reverting to its snapshot.

## Token and Cost Tracking

At each `finish-step` event:

```
usage = Session.getUsage({
  model,
  usage: { inputTokens, outputTokens, reasoningTokens, cachedInputTokens },
  metadata: providerMetadata  // provider-specific cache info
})
```

**Token calculation**:
- Anthropic/Bedrock: `inputTokens` excludes cached tokens (must add manually)
- Others: `inputTokens` includes cached (must subtract)
- `adjustedInput = inputTokens - cacheRead - cacheWrite` (for non-Anthropic)
- Reasoning tokens tracked separately

**Cost calculation**:
```
cost = (input × input_rate / 1M)
     + (output × output_rate / 1M)
     + (cache_read × cache_read_rate / 1M)
     + (cache_write × cache_write_rate / 1M)
     + (reasoning × output_rate / 1M)  // reasoning billed as output
```

Special case: if `input + cache_read > 200K tokens` and model has
`experimentalOver200K` pricing, use the higher rate.

Tokens and cost are accumulated on the assistant message and persisted after
each step.

## Error Handling and Retry Logic

```
Error received
    │
    ├─ Is it a ContextOverflowError?
    │   YES → set needsCompaction = true, break
    │
    ├─ Is it retryable? (checked via SessionRetry.retryable())
    │   │  Retryable errors: rate limits, overloaded, 5xx, timeouts
    │   │
    │   YES → attempt++
    │         delay = INITIAL_DELAY × 2^(attempt-1)
    │         // Also checks Retry-After header if present
    │         set status → "retry" { attempt, next: now + delay }
    │         sleep(delay, abort)  // cancellable sleep
    │         continue (retry the while loop)
    │
    └─ Non-retryable → store error on message, publish, break
```

**Non-retryable errors**: auth failures, aborts, structured output errors,
unknown errors.

## Special Error Types

| Error | Source | Effect |
|-------|--------|--------|
| `Permission.RejectedError` | User denied permission | Sets `blocked = true` if configured |
| `Permission.DeniedError` | Rule explicitly denies | Tool marked as error, loop continues |
| `Question.RejectedError` | User rejected question | Sets `blocked = true` if configured |
| `ContextOverflowError` | Input too long for model | Triggers compaction |
| `AbortedError` | User cancelled | Loop exits via abort check |

The `shouldBreak` config (from `continue_loop_on_deny`) controls whether
permission/question rejections halt the entire loop or just fail the
individual tool call.

## Auto-Compaction Trigger

At `finish-step`, after recording tokens:

```
if !assistantMessage.summary
   AND SessionCompaction.isOverflow(tokens, model):
  needsCompaction = true
```

The overflow check: `tokens.total >= (model.limit.input - reserved)`
where `reserved = min(20000, maxOutputTokens)`.

This causes the processor to return `"compact"`, which the main loop handles
by creating a CompactionPart for the next iteration.
