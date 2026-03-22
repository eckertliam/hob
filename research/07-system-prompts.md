# System Prompt Construction

**Sources**:
- `packages/opencode/src/session/llm.ts` - Final assembly
- `packages/opencode/src/session/system.ts` - Environment and skills
- `packages/opencode/src/session/instruction.ts` - Config file loading
- `packages/opencode/src/agent/agent.ts` - Agent definitions
- `packages/opencode/src/session/prompt/*.txt` - Provider templates
- `packages/opencode/src/agent/prompt/*.txt` - Agent templates

## Assembly Pipeline

The system prompt is assembled from multiple layers, each adding context:

```
Layer 1: Agent or Provider Base Prompt
    │
    ▼
Layer 2: Environment Context
    │
    ▼
Layer 3: Skills List
    │
    ▼
Layer 4: Instruction Files (AGENTS.md, CLAUDE.md)
    │
    ▼
Layer 5: Structured Output Instructions (if applicable)
    │
    ▼
Layer 6: Plugin Hook (experimental.chat.system.transform)
    │
    ▼
Final system prompt array → sent to LLM
```

## Layer 1: Base Prompt

Selected based on agent and model:

```
if agent.prompt exists:
  use agent.prompt
else:
  select by model API:
    OpenAI GPT-4/5/o1/o3  → beast.txt
    OpenAI GPT-3.5         → codex.txt
    Google Gemini           → gemini.txt
    Anthropic Claude        → anthropic.txt
    Trinity                 → trinity.txt
    Default (others)        → default.txt
```

### Provider Prompt Philosophies

**anthropic.txt** (~1,300 chars):
- Emphasizes task management with TodoWrite tool
- Structured planning approach
- Brief, focused instructions

**beast.txt** (~4,800 chars):
- Aggressive autonomous problem-solving mode
- "Do not stop until the task is fully complete"
- Mandates extensive research before changes
- Requires running tests and verification
- Most verbose and demanding prompt

**default.txt** (~3,600 chars):
- Concise, direct responses
- Minimal output tokens
- No explanations unless asked
- Tool-first approach (use tools instead of explaining)

**codex.txt**:
- Similar to default but optimized for GPT models
- Emphasis on conciseness

### Agent-Specific Prompts

- **plan**: Read-only mode enforcement. "You MUST NOT modify any files."
- **explore**: File search specialist. Only Glob, Grep, Read tools.
- **compaction**: Conversation summarization template (Goal, Instructions,
  Discoveries, Accomplished, Relevant files)
- **title**: Thread title generation. "Single line, max 50 chars."
- **summary**: PR description-style summaries

## Layer 2: Environment Context

```
SystemPrompt.environment(model) → string[]

Returns a block like:
  "# Environment
   - Model: Claude Opus 4 (claude-opus-4-20250514)
   - Working directory: /Users/user/project
   - Workspace root: /Users/user/project
   - Git repository: true
   - Platform: darwin
   - Today's date: 2026-03-21"
```

This gives the model awareness of its execution context.

## Layer 3: Skills

```
SystemPrompt.skills(agent) → string | undefined

If agent has available skills:
  "The following skills are available:
   - skill-name: description
   - skill-name: description
   ..."
```

Skills are filtered by agent permissions before listing.

## Layer 4: Instruction Files

```
InstructionPrompt.system() → string[]

Searches for and loads instruction files:

Project-level (searched upward from working directory):
  AGENTS.md
  CLAUDE.md
  CONTEXT.md (deprecated)

Global:
  $OPENCODE_CONFIG_DIR/AGENTS.md
  ~/.opencode/AGENTS.md
  ~/.claude/CLAUDE.md (if enabled)

Remote:
  HTTP/HTTPS URLs from config

Each file's content is added as a separate system prompt string.
```

These files contain project-specific instructions that the user writes
to customize agent behavior (coding conventions, test requirements, etc.).

## Layer 5: Structured Output (Conditional)

```
If user requested JSON schema output:
  "IMPORTANT: The user has requested structured output.
   You MUST use the StructuredOutput tool to provide your
   final response. Do NOT respond with plain text..."
```

## Layer 6: Plugin Hook

```
Plugin.trigger("experimental.chat.system.transform",
  { sessionID, model },
  { system: systemArray })

// Plugins can modify the system array in-place:
//   Add prompts, remove prompts, rewrite prompts
```

## Final Assembly in LLM.stream()

```
system = []

// Base prompt
if (agent.prompt):
  system = [agent.prompt]
else:
  system = [providerPrompt(model)]

// Custom system from loop
system.push(...input.system)   // environment, skills, instructions

// User-specific system
system.push(...user.system)

// For OpenAI OAuth providers:
//   Pass as "instructions" parameter (not system role messages)
// For all others:
//   Prepend as system role messages in the message array
```

## Ephemeral Injections

Beyond the main system prompt, the loop injects ephemeral content:

### Plan/Build Mode Reminders
```
insertReminders(messages, agent, session):
  If switching from plan to build mode:
    Insert BUILD_SWITCH prompt reminding model of the plan
  If in plan agent:
    Insert PROMPT_PLAN before user messages
```

### Max Steps Warning
```
If step >= maxSteps (agent's step limit):
  Append to messages as synthetic assistant content:
    "You are on your last step. Wrap up your work and
     provide a final response to the user."
```

### User Message Wrapping (Multi-Step)
```
If step > 1 and there are unaddressed user messages:
  Wrap each in:
    <system-reminder>
    The user sent the following message:
    {original text}

    Please address this message and continue with your tasks.
    </system-reminder>
```

## Message Conversion to LLM Format

`MessageV2.toModelMessages()` converts the internal message/part structure
to the format expected by the AI SDK:

```
User messages:
  TextPart → { type: "text", text: "..." }
  FilePart → { type: "image", image: URL } or { type: "file", ... }
  CompactionPart → { type: "text", text: "What did we do so far?" }
  AgentPart → skipped (metadata only)

Assistant messages:
  TextPart → { type: "text", text: "..." }
  ReasoningPart → reasoning content with provider metadata
  ToolPart (completed) → tool-call + tool-result pair
  ToolPart (error) → tool-call + error result
  ToolPart (compacted) → tool-call + "[Old tool result content cleared]"

Provider-specific handling:
  Some providers don't support media in tool results
  → Extract media, inject as separate user message
```

## Prompt Caching Markers

To optimize API costs, certain messages are marked for provider-side caching:

```
First 2 system messages: marked for caching (stable content)
Last 2 non-system messages: marked for caching (recent context)

Format varies by provider:
  Anthropic: providerOptions.anthropic.cacheControl = { type: "ephemeral" }
  Bedrock: providerOptions.bedrock.cachePoint = { type: "default" }
  OpenAI: providerOptions.openai.cache_control = { type: "ephemeral" }
```

## Design Insights for Emacs

1. **Layered prompt construction**: Don't try to build one monolithic prompt.
   Layer it: base behavior, environment context, project instructions,
   skill content, mode-specific overrides. Each layer is independently
   manageable.

2. **Instruction files are powerful**: AGENTS.md / CLAUDE.md let users
   customize agent behavior per-project without code changes. In Emacs,
   this could be `.dir-locals.el` style files or a `.agent-instructions`
   file in the project root.

3. **Environment context matters**: Tell the model where it is (directory,
   platform, date). Models make better decisions with spatial awareness.

4. **Provider-specific prompt styles**: Different models respond better to
   different prompting styles. The beast.txt (aggressive autonomous) vs
   default.txt (concise) distinction is worth preserving.

5. **Ephemeral injections for state**: Mode switches, step limits, and
   multi-step user messages are handled by injecting content into the
   message stream, not by modifying the system prompt. This keeps the
   system prompt stable (good for caching) while adapting behavior.
