# Tool System

**Sources**:
- `packages/opencode/src/tool/tool.ts` - Tool definition interface
- `packages/opencode/src/tool/registry.ts` - Tool discovery and management
- `packages/opencode/src/session/prompt.ts` - Tool resolution and execution context

## Tool Definition

A tool is defined with `Tool.define(id, init)`:

```
Tool.define("my_tool", async () => ({
  description: "What this tool does",
  parameters: z.object({          // Zod schema
    arg1: z.string(),
    arg2: z.number().optional(),
  }),
  async execute(args, ctx) {      // The function
    // args: validated by Zod schema
    // ctx: Tool.Context (see below)
    return {
      title: "Short description of result",
      output: "The actual result string",
      metadata: { ... },           // Optional structured metadata
      attachments: [FilePart],     // Optional file attachments
    }
  },
  formatValidationError(error) {  // Optional custom error formatting
    return "Friendly error message"
  }
}))
```

The `init` function is async, allowing tools to perform setup (read config,
discover capabilities) before returning their definition.

## Tool Context

Every tool execution receives a `Tool.Context`:

```
{
  sessionID      // Which session this is part of
  messageID      // Which assistant message owns this tool call
  callID         // Unique ID for this specific invocation
  agent          // Which agent triggered this (e.g., "build", "explore")
  abort          // AbortSignal - check or listen for cancellation
  messages       // Full conversation history (read-only)
  extra          // Arbitrary data (e.g., { bypassAgentCheck: true })

  metadata(input)  // Update tool's in-progress metadata
                   // Called during execution to show progress to UI
                   // e.g., ctx.metadata({ title: "Reading file..." })

  ask(request)     // Request permission from user
                   // Blocks until user approves/rejects
                   // Throws if denied
}
```

## Tool Registry

The registry discovers and manages available tools from multiple sources:

### Discovery Sources (in order)

1. **Built-in tools**: Hardcoded list
   - `bash` - Shell command execution
   - `read` - Read files
   - `glob` - Find files by pattern
   - `grep` - Search file contents
   - `edit` - Edit files (find-and-replace)
   - `write` - Create/overwrite files
   - `task` - Launch subagents
   - `webfetch` - Fetch URLs
   - `websearch` - Web search
   - `codesearch` - Semantic code search
   - `skill` - Load skills into context
   - `apply_patch` - Apply unified diffs (for GPT models)
   - `lsp` - Language server protocol queries
   - `batch` - Execute multiple tools in parallel
   - `plan` - Enter/exit plan mode
   - `question` - Ask user a question
   - `invalid` - Placeholder for unresolvable tool calls

2. **Config directory tools**: Files in `{tool,tools}/*.{ts,js}` relative to
   project config directories

3. **Plugin-provided tools**: From each plugin's `tool` hook

### Filtering

Not all tools are available to all agents/models:

```
ToolRegistry.tools(agent, model) → Record<string, ToolInfo>
```

- Model-specific: `websearch` / `codesearch` only for certain providers
- Model family: GPT models get `apply_patch` instead of `edit`/`write`
- Agent-specific: "explore" agent only gets read-only tools
- Permission-based: tools filtered by agent permission ruleset

## Tool Execution Pipeline

When the LLM calls a tool, the following happens inside the stream processor:

```
1. LLM emits tool-call event with { toolName, input, toolCallId }

2. Stream processor looks up tool in resolved tools map

3. AI SDK validates input against JSON schema (converted from Zod)

4. AI SDK calls the tool wrapper function:

   a. Create Tool.Context with:
      - sessionID, messageID from current session
      - abort signal from loop
      - ask() function that merges agent + session permissions

   b. Plugin hook: tool.execute.before
      - Can modify args before execution
      - Receives: { tool, sessionID, callID }
      - Modifies: { args }

   c. Permission check via ctx.ask():
      - Tool calls ask() with permission type and patterns
      - Permission.ask() evaluates against rulesets
      - If "allow" → continue
      - If "deny" → throw DeniedError
      - If "ask" → block, publish event, wait for user
        - User approves → continue
        - User rejects → throw RejectedError

   d. Execute tool function:
      - tool.execute(args, ctx)
      - May call ctx.metadata() during execution for progress
      - Returns { title, output, metadata, attachments }

   e. Plugin hook: tool.execute.after
      - Can modify result
      - Receives: { tool, sessionID, callID, args }
      - Modifies: { title, output, metadata }

   f. Output truncation:
      - If output > threshold: truncate and save full output to file
      - Prevents enormous tool results from filling the context

5. Result fed back to LLM via AI SDK
   - AI SDK includes tool results in next API call
   - Model sees the result and decides next action
```

## Tool Result Format

```
{
  title: string          // Short description shown in UI
                         // e.g., "Read 150 lines from src/main.ts"

  output: string         // The actual result content
                         // This is what the LLM sees

  metadata?: Record      // Structured metadata (for UI display)
                         // e.g., { lines: 150, path: "src/main.ts" }

  attachments?: FilePart[]  // File references (images, etc.)
                            // Shown inline in UI, sent to LLM if supported
}
```

## The Batch Tool

A special meta-tool that executes multiple tools in parallel:

```
batch({
  tools: [
    { name: "read", args: { path: "file1.ts" } },
    { name: "read", args: { path: "file2.ts" } },
    { name: "grep", args: { pattern: "TODO" } },
  ]
})
```

- Max 25 tools per batch
- Each tool gets its own ToolPart in the message
- Results aggregated and returned to LLM
- Enables the model to read multiple files simultaneously

## Tool Schema Transformation

The Zod schema goes through transformations before reaching the LLM:

```
1. Zod schema → z.toJSONSchema() → JSON Schema

2. ProviderTransform.schema(jsonSchema, model)
   - OpenAI strict mode: enforces additionalProperties: false
   - Some providers need flattened schemas
   - Tool definitions may be modified by plugin hook: tool.definition

3. Sent to LLM as part of the tools array in the API call
```

## Key Design Insights for Emacs Implementation

1. **Tools are just functions with schemas**: Define input schema, implement
   execute function, return structured result. Simple interface.

2. **Permission is external to the tool**: Tools call `ctx.ask()` but don't
   implement permission logic. The harness handles evaluation.

3. **Tools are stateless**: Each call gets a fresh context. No persistent
   state between calls. State lives in the session/message DB.

4. **Output truncation is critical**: Without it, a tool returning a huge
   file would blow the context window. The harness truncates and stores
   the full output separately.

5. **The batch tool is an optimization**: Lets the model parallelize I/O
   without multiple round-trips to the LLM.

6. **Plugin hooks wrap execution**: Before/after hooks allow modifying args
   and results without changing tool implementations.
