# Message and Session Storage

**Sources**:
- `packages/opencode/src/session/index.ts` - Session management
- `packages/opencode/src/session/message-v2.ts` - Message storage
- `packages/opencode/src/session/session.sql.ts` - Database schema
- `packages/opencode/src/bus/index.ts` - Event system

## Database Schema

Three core tables in SQLite (via Drizzle ORM):

### SessionTable
```
id           TEXT PK    ← descending order (newest first)
project_id   TEXT FK    → ProjectTable (CASCADE delete)
workspace_id TEXT FK    → optional workspace
parent_id    TEXT FK    → self-referential (for forks)
slug         TEXT       ← human-readable identifier
directory    TEXT       ← working directory path
title        TEXT       ← session name
version      TEXT       ← installation version
share_url    TEXT       ← optional sharing link
permission   JSON       ← Permission.Ruleset (session-specific rules)
revert       JSON       ← { messageID, partID, snapshot, diff }
summary_*    JSON       ← additions, deletions, files, diffs
time_created INTEGER
time_updated INTEGER
time_compacting INTEGER
time_archived INTEGER
```

### MessageTable
```
id           TEXT PK    ← ascending order (oldest first)
session_id   TEXT FK    → SessionTable (CASCADE delete)
data         JSON       ← full message metadata (see Message schema)
time_created INTEGER
time_updated INTEGER

Index: (session_id, time_created, id) ← efficient reverse-chrono queries
```

### PartTable
```
id           TEXT PK    ← ascending order
message_id   TEXT FK    → MessageTable (CASCADE delete)
session_id   TEXT       ← denormalized for query optimization
data         JSON       ← full part content (see Part types)
time_created INTEGER
time_updated INTEGER

Index: (message_id, id)
Index: (session_id)
```

## Message Schema

Messages are discriminated by role:

### User Message
```
{
  role: "user",
  id: MessageID,
  sessionID: SessionID,
  agent: string,                    ← which agent to use
  model: { providerID, modelID },   ← which model
  system?: string,                  ← custom system prompt override
  format?: { type: "text" | "json_schema", schema? },
  tools?: Record<string, boolean>,  ← tool availability overrides
  variant?: string,                 ← model variant name
  time: { created: number }
}
```

### Assistant Message
```
{
  role: "assistant",
  id: MessageID,
  sessionID: SessionID,
  parentID: MessageID,              ← references parent user message
  agent: string,                    ← which agent generated this
  modelID: ModelID,
  providerID: ProviderID,
  path: { cwd, root },             ← working directory at time of generation
  cost: number,                     ← accumulated USD cost
  tokens: {
    input: number,
    output: number,
    reasoning: number,
    cache: { read: number, write: number }
  },
  finish?: string,                  ← "stop", "tool-calls", "length", etc.
  error?: NamedError,               ← if something went wrong
  summary?: boolean,                ← true if this is a compaction summary
  structured?: any,                 ← JSON output if structured mode
  time: { created: number, completed?: number }
}
```

## Part Types

Parts are the atomic content units within messages:

| Type | Fields | Purpose |
|------|--------|---------|
| TextPart | `text, time, synthetic?, metadata?` | LLM-generated text |
| ReasoningPart | `text, time, metadata?` | Extended thinking (Claude) |
| FilePart | `mime, url, filename?, source?` | Uploaded files/images |
| ToolPart | `tool, callID, state, metadata?` | Tool invocation |
| StepStartPart | `snapshot?` | Beginning of LLM step |
| StepFinishPart | `reason, snapshot?, cost, tokens` | End of LLM step |
| PatchPart | `hash, files[]` | File changes within a step |
| CompactionPart | `auto, overflow?` | Marks compaction point |
| SubtaskPart | `prompt, description, agent, model?, command?` | Subagent work |
| RetryPart | `attempt, error, time` | Error recovery tracking |
| AgentPart | `name, source?` | Reference to another agent |

### Tool State Machine (within ToolPart)

```
Pending:   { status: "pending", input: {}, raw: "" }
Running:   { status: "running", input: Record, time: { start } }
Completed: { status: "completed", input, output, title, metadata,
             time: { start, end, compacted? }, attachments? }
Error:     { status: "error", input, error: string, time: { start, end } }
```

## Message Loading and Pagination

### Streaming (for the agent loop)

```
MessageV2.stream(sessionID) → AsyncGenerator<MessageV2.WithParts>
```

Uses cursor-based pagination internally (50 messages per page). Yields
messages in chronological order. Each message includes its hydrated parts.

### Pagination (for the UI)

```
MessageV2.page({ sessionID, limit, before? })
  → { items: WithParts[], more: boolean, cursor?: string }
```

Cursor format: `{ id, time }` encoded as base64url JSON.
Query: `DESC time_created, DESC id` (newest first for efficient loading).
Returns `limit + 1` rows to detect if more exist.

### Hydration Process

```
1. Load message rows from MessageTable WHERE session_id = X
2. Extract all messageIDs
3. Load all parts from PartTable WHERE message_id IN (...)
4. Group parts by messageID using Map
5. Combine: each message gets its parts array
```

## Event System (Real-Time Updates)

The event bus is a pub/sub system that notifies clients of changes.

### Architecture

```
Service writes to DB
    │
    ├─ Database.effect(() => Bus.publish(event))
    │    ↑ runs AFTER transaction commits
    │
    ▼
Bus distributes to subscribers
    │
    ├─ Plugin event hooks
    ├─ SSE endpoint (GET /event)
    └─ Internal services
```

### Event Types

**Session events:**
- `session.created` - New session
- `session.updated` - Metadata changed
- `session.deleted` - Session removed
- `session.diff` - File changes published
- `session.error` - Error occurred
- `session.compacted` - History summarized

**Message events:**
- `message.updated` - Message metadata changed
- `message.removed` - Message deleted
- `message.part.updated` - Part fully updated
- `message.part.delta` - Incremental text delta (streaming)
- `message.part.removed` - Part deleted

### Delta Streaming

For real-time text streaming to the UI:

```
updatePartDelta({
  sessionID, messageID, partID,
  field: "text",
  delta: "the new chunk of text"
})
```

This publishes a `message.part.delta` event WITHOUT updating the database.
The full text is only persisted when the text-end event arrives. This
reduces DB writes during rapid streaming.

### SSE Endpoint

```
GET /event → Server-Sent Events stream

- Heartbeat every 10 seconds
- AsyncQueue for buffering events
- All events serialized as JSON
- Graceful disconnect handling
```

## Session Operations

### Create
```
Session.create({ agent, model }) → Session.Info
- Generates descending ID (newest sessions have smallest IDs)
- Sets version to current installation version
- Publishes session.created event
```

### Fork
```
Session.fork({ sessionID, messageID? }) → Session.Info
- Creates new session with parent_id pointing to source
- Copies all messages up to optional messageID
- Clones all parts for each message
- Remaps parentID references to new message IDs
- Title: "Original Title (fork #N)"
```

### Revert
```
SessionRevert.revert({ sessionID, messageID, partID? })
- Captures workspace snapshot
- Applies patches (undoes file changes)
- Stores diff for preview
- Removes reverted messages/parts from history
- Publishes message.removed / part.removed events
```

## Transaction Pattern

```
Database.use((db) => {
  // Perform DB operations
  const row = db.update(Table).set(data).where(...).returning().get()

  // Queue event for AFTER transaction commits
  Database.effect(() => Bus.publish(Event.Updated, info))
})
```

Events are queued during the transaction and only fire after commit. This
prevents publishing events for changes that get rolled back.

## Design Insights for Emacs

1. **JSON blobs for flexibility**: Storing messages and parts as JSON blobs
   in SQLite allows schema evolution without migrations. New fields can be
   added without breaking old sessions.

2. **Denormalized session_id on parts**: Avoids joins. Fast queries for
   "all parts in this session" without going through messages.

3. **Ascending message IDs, descending session IDs**: Messages are read
   oldest-first (natural conversation order). Sessions are listed
   newest-first (most relevant first).

4. **Delta streaming separate from persistence**: Don't update the DB for
   every streaming chunk. Publish deltas to the UI in real-time, persist
   only the final result. Reduces I/O dramatically during streaming.

5. **Event-after-commit pattern**: Prevents phantom events for rolled-back
   transactions. Simple but critical for consistency.

6. **Forking as message copying**: No complex branching data structure.
   Just copy messages and remap references. Simple and reliable.
