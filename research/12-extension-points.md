# Extension Points and Hook System

This documents the hook/extension points OpenCode uses throughout the agent
loop. You don't need a full plugin loading system, but you should design
your harness with these extension points in mind. In Emacs, these map to
hook variables and advice functions.

## Hook Points Reference

### Where hooks fire in the agent loop

```
User sends message:
  │
  ├─ hook: message-received         ← modify user message/parts
  │
  ▼
Build message history:
  │
  ├─ hook: messages-transform       ← transform message history
  │
  ├─ hook: tool-definition          ← modify tool schemas
  │
  ▼
Call LLM:
  │
  ├─ hook: system-prompt-transform  ← modify system prompt
  │
  ├─ hook: llm-params              ← modify temperature, topP, etc.
  │
  ▼
Tool Execution:
  │
  ├─ hook: tool-before             ← modify tool args
  │
  ├─ [tool executes]
  │
  ├─ hook: tool-after              ← modify tool result
  │
  ▼
Text Finalization:
  │
  └─ hook: text-complete           ← modify final text output

Session Compaction:
  │
  └─ hook: compaction-prompt       ← customize summarization prompt
```

### Full hook reference

| Hook | When | What it can modify |
|------|------|--------------------|
| message-received | User message arrives | Message content, attached parts |
| messages-transform | Before LLM call | Full message history array |
| system-prompt-transform | Before LLM call | System prompt strings |
| llm-params | Before LLM call | temperature, topP, topK, provider options |
| tool-definition | Sending tools to LLM | Tool description, parameter schema |
| tool-before | Before tool runs | Tool arguments |
| tool-after | After tool runs | Tool title, output, metadata |
| text-complete | After text finalized | Final text content |
| compaction-prompt | During compaction | Summarization prompt and context |
| permission-check | Permission evaluation | Override allow/deny/ask decision |
| shell-env | Shell execution | Environment variables |

### Trigger mechanism

```
trigger(hookName, input, output):
  for each registered hook:
    hook(input, output)    // mutates output in-place
  return output
```

Sequential execution. Each hook sees the output of the previous one.
Mutation-based (no return values to manage).

## Skills (Loadable Context)

Skills are markdown files that get injected into the system prompt on demand.

```
Format:
  ---
  name: MySkill
  description: What this skill teaches the agent
  ---

  Instructions and knowledge content...

Discovery:
  ~/.config/agent/skills/**/SKILL.md    (global)
  .agent/skills/**/SKILL.md             (project)
  Custom paths from config

Loading:
  Agent calls "skill" tool → content added to system prompt
  Available for remainder of session
```

Skills are filtered by agent permissions (some agents can't load certain
skills).
