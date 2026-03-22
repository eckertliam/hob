# Compaction and Context Management

**Source**: `packages/opencode/src/session/compaction.ts`

## The Problem

LLMs have finite context windows. A long coding session accumulates messages,
tool calls, and tool outputs that eventually exceed the model's input limit.
The harness needs a strategy to manage this.

## Two-Phase Strategy

OpenCode uses two complementary approaches:

### Phase 1: Pruning (Clearing Old Tool Outputs)

Prune doesn't lose information about *what* happened, just the *raw output*
of old tool calls. The model can still see that a tool was called and what
arguments were used.

```
Algorithm: SessionCompaction.prune()

Walk backward through message parts (newest → oldest):
  Skip messages that contain summary annotations
  Skip the last 2 user turns (preserve recent work)

  For each completed ToolPart (excluding protected tools):
    Accumulate the tool output size in "pruned" counter
    Mark part.state.time.compacted = Date.now()

  Stop accumulating when total tokens >= PRUNE_PROTECT (40,000)
  Only actually prune if pruned > PRUNE_MINIMUM (20,000)
```

**Protected tools**: `["skill"]` -- skill outputs are never pruned because
they contain system instructions that might still be needed.

**Effect on message conversion**: When converting messages to LLM format,
compacted tool parts show `"[Old tool result content cleared]"` instead of
the original output.

### Phase 2: Compaction (Summarization)

When pruning isn't enough (or context overflow is hit), the system
summarizes the entire conversation history.

## Compaction Triggers

Compaction can be triggered three ways:

### 1. Auto-detection in the main loop
```
After each assistant message finishes:
  if lastFinished.summary != true
     AND tokens.total >= (model.limit.input - reserved):
    → Create CompactionPart { auto: true }
    → Continue loop (compaction processed next iteration)
```

### 2. Context overflow from LLM
```
Stream processor catches ContextOverflowError:
  → Returns "compact" to main loop
  → Main loop creates CompactionPart { auto: true, overflow: true }
```

### 3. Explicit CompactionPart in pending tasks
```
Main loop finds a CompactionPart in the task queue:
  → Calls SessionCompaction.process()
```

## The Compaction Algorithm

```
SessionCompaction.process(messages, parentID, abort, sessionID, auto, overflow)
  │
  ├─ 1. DETERMINE REPLAY POINT
  │     If overflow:
  │       Find last user message WITHOUT a CompactionPart
  │       before the target message
  │       This becomes the "replay" message (re-sent after summary)
  │
  ├─ 2. BUILD SUMMARIZATION PROMPT
  │     Default template:
  │       "Provide a detailed prompt for continuing our conversation.
  │        Focus on what we did, what we're doing, which files,
  │        and what to do next.
  │
  │        Template:
  │        ## Goal
  │        ## Instructions
  │        ## Discoveries
  │        ## Accomplished
  │        ## Relevant files / directories"
  │
  │     Plugin hook: experimental.session.compacting
  │       Can provide context strings or replace prompt entirely
  │
  ├─ 3. CALL SUMMARIZATION AGENT
  │     Uses "compaction" agent (or user's agent as fallback)
  │     Sends ALL previous messages (stripped of media)
  │     Sends the summarization prompt as final user message
  │     Model generates a summary
  │
  │     The assistant message is marked: summary = true
  │     This prevents it from being compacted again
  │
  ├─ 4. HANDLE REPLAY (if overflow)
  │     ├─ If replay message exists:
  │     │    Create NEW user message replaying original request
  │     │    Copy all parts except media (replaced with placeholders)
  │     │    Skip CompactionParts
  │     │
  │     ├─ If no replay but overflow:
  │     │    Create synthetic message: "Media was too large,
  │     │    summarize and continue"
  │     │
  │     └─ If normal compaction (not overflow):
  │          No replay needed
  │
  └─ 5. RETURN
       If compaction succeeded → "continue" (loop continues)
       If compaction failed → "stop" (loop breaks)
```

## Message Filtering After Compaction

`MessageV2.filterCompacted()` controls what the loop sees:

```
Iterate through messages:
  Track a "completed" set of assistant message IDs
    (where assistant.summary == true)

  When we find a user message with CompactionPart:
    If there's a completed summary after it → stop here
    Only return messages AFTER this compaction boundary

Result: the loop only sees messages from the latest compaction forward
```

This means after compaction, the model sees:
1. The summary message (from compaction agent)
2. Any replay of the original request
3. New messages from that point forward

Everything before the compaction boundary is invisible to the model.

## Media Handling During Compaction

```
When building the summary request:
  MessageV2.toModelMessages(msgs, model, { stripMedia: true })

Effect:
  Images/files → "[Attached image/png: screenshot.png]"
  Tool attachments → stripped
  Text → preserved as-is
  CompactionParts → "What did we do so far?"
```

## Overflow Constants

```
COMPACTION_BUFFER = 20,000 tokens
PRUNE_PROTECT    = 40,000 tokens
PRUNE_MINIMUM    = 20,000 tokens

reserved = config.compaction.reserved ?? min(COMPACTION_BUFFER, maxOutputTokens)
overflow_threshold = model.limit.input - reserved
```

## Data Flow Diagram

```
Normal operation:
  User → Model → Tools → Model → ... → Stop

Context filling up:
  [msg1] [msg2] [msg3] ... [msg100]
                                    ↑ tokens approaching limit

Auto-compaction triggered:
  [msg1] [msg2] ... [msg100] [CompactionPart]

Next iteration processes compaction:
  1. Send all messages to compaction agent
  2. Compaction agent writes summary
  3. If overflow: replay last user message

After compaction, model sees:
  [Summary: "We were working on X. Files modified: Y. Next: Z."]
  [Replay: "Original user request"]
  [New messages from here...]

Everything before the summary is hidden by filterCompacted()
```

## Design Insights for Emacs

1. **Two-phase approach is elegant**: Pruning is cheap (just clear outputs)
   and buys time. Compaction is expensive (LLM call) but comprehensive.
   Do pruning first, compaction when needed.

2. **Summary + replay is key for overflow recovery**: When the model can't
   even fit the current request, summarize everything, then replay just
   the request. The model gets a fresh start with context.

3. **The compaction boundary as a filter**: After compaction, simply don't
   send old messages. The summary captures what matters.

4. **Protected content matters**: Some tool outputs (like skills/instructions)
   should never be pruned because they contain system-level information.

5. **Summarization template is important**: A structured template (Goal,
   Instructions, Discoveries, Accomplished, Files) ensures the summary
   captures the right information for continuation.
