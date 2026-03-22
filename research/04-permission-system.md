# Permission System

**Sources**:
- `packages/opencode/src/permission/index.ts` - Main service
- `packages/opencode/src/permission/evaluate.ts` - Evaluation algorithm
- `packages/opencode/src/util/wildcard.ts` - Pattern matching

## Purpose

The permission system gates dangerous tool operations behind user approval.
It's the safety layer between the LLM's tool calls and actual execution.

## Rule Structure

A permission rule has three fields:

```
{
  permission: string   // What category: "read", "edit", "bash", etc.
  pattern: string      // What resource: file path, command, query, etc.
  action: "allow" | "deny" | "ask"
}
```

A ruleset is an ordered array of rules: `Rule[]`

## Evaluation Algorithm

```
evaluate(permission, pattern, ...rulesets):
  rules = rulesets.flat()   // Concatenate all rulesets
  match = rules.findLast(   // LAST match wins
    rule => wildcard_match(permission, rule.permission)
         && wildcard_match(pattern, rule.pattern)
  )
  return match ?? { action: "ask", permission, pattern: "*" }
```

**Key property**: Last-match-wins. This means rules later in the array
override earlier ones. Since rulesets are concatenated in order
(defaults → agent → user → session), more specific/later configs take
precedence.

## Wildcard Matching

Patterns use glob-style wildcards:

| Pattern | Matches |
|---------|---------|
| `*` | Any string (including empty) |
| `?` | Any single character |
| `*.env` | Any file ending in .env |
| `*/src/*` | Any path containing /src/ |
| `rm *` | "rm" followed by anything |

Implementation: wildcards are converted to regex:
- `*` → `.*`
- `?` → `.`
- Special regex chars escaped
- Backslashes normalized to forward slashes
- Case-insensitive on Windows, case-sensitive on Unix

## Ruleset Merge Order

Rulesets are merged by simple concatenation (last-match-wins):

```
Layer 1: Defaults (hardcoded)
  "*": allow              ← allow everything by default
  "doom_loop": ask        ← ask about infinite loops
  "external_directory": ask  ← ask about outside-project access
  "question": deny        ← don't let model ask user questions
  "read.*.env": ask       ← ask before reading .env files

Layer 2: Agent-specific overrides
  e.g., "build" agent adds:
    "question": allow     ← build agent can ask questions
    "plan_enter": allow   ← build agent can enter plan mode

  e.g., "explore" agent adds:
    "*": deny             ← deny everything by default
    "grep": allow         ← only allow read-only tools
    "glob": allow
    "read": allow

Layer 3: User config (opencode.json)
  Whatever the user configured in their permission section

Layer 4: Session-specific
  Rules stored on the session object

Merged: [...defaults, ...agent, ...user, ...session]
Last matching rule wins
```

## The Ask Flow

When evaluation returns `action: "ask"`, the permission system blocks the
tool and prompts the user:

```
Tool calls ctx.ask({
  permission: "edit",
  patterns: ["/path/to/file.ts"],
  always: ["*"],              ← patterns to auto-approve if "always"
  metadata: { ... }           ← context for UI display
})
    │
    ▼
Permission.ask() evaluates each pattern:
    │
    ├─ If any pattern evaluates to "deny" → throw DeniedError
    │
    ├─ If all patterns evaluate to "allow" → return (tool proceeds)
    │
    └─ If any pattern evaluates to "ask":
         │
         ▼
    Create Deferred promise
    Add to pending map: { id, info, deferred }
    Publish Permission.Asked event → UI shows prompt
    await Deferred (tool blocks here)
         │
         ▼
    User responds:
    ┌─────────┬─────────────┬──────────────┐
    │         │             │              │
  "once"   "always"     "reject"
    │         │             │
    ▼         ▼             ▼
  Resolve   Add rules    Fail deferred
  deferred  to approved  (RejectedError)
            ruleset
            Resolve      Cascade: reject
            deferred     ALL other pending
                         in same session
            Auto-check
            other pending
            in same session
```

## Persistence

**"Once"**: No persistence. Approved for this one call only. Next identical
request will prompt again.

**"Always"**: The patterns from `request.always` are added to an in-memory
`approved` ruleset (stored in PermissionTable, keyed by project ID). These
auto-approve matching requests for the rest of the session. When OpenCode
restarts, they're gone.

**User config**: Permanent rules in `opencode.json` survive restarts.

## Cascade Behavior

When user rejects a permission:

```
1. Fail the requesting tool's deferred → tool gets RejectedError
2. Find ALL other pending permissions in the same session
3. Reject them all too
4. This effectively halts all in-flight tool execution for the session
```

When user approves with "always":

```
1. Add patterns to approved ruleset
2. Resolve the requesting tool's deferred
3. Check ALL other pending permissions in the same session:
   For each: re-evaluate with updated approved rules
   If all patterns now pass → auto-approve (resolve deferred)
```

This means approving one "always" permission can automatically approve
similar pending requests without prompting again.

## Common Permission Types

| Permission | Patterns | Controls |
|-----------|----------|----------|
| `bash` | Command string | Shell execution |
| `read` | File path | File reading |
| `edit` / `write` | File path | File modification |
| `external_directory` | Directory path | Access outside project |
| `websearch` | Search query | Web searches |
| `webfetch` | URL | HTTP requests |
| `skill` | Skill name | Loading skills |
| `doom_loop` | Tool name | Breaking out of infinite loops |
| `question` | Question text | Model asking user questions |
| `task` | Agent name | Launching subagents |

## Error Types

| Error | Meaning | Effect on Loop |
|-------|---------|---------------|
| `DeniedError` | Rule explicitly denies | Tool fails, loop may continue |
| `RejectedError` | User clicked reject | Tool fails, may halt loop |
| `CorrectedError` | User rejected with feedback | Like rejected but includes message |

Whether rejection halts the loop depends on `config.continue_loop_on_deny`:
- `true`: only the individual tool fails, loop continues
- `false` (default): sets `blocked = true`, processor returns "stop"

## Design Insights for Emacs

1. **Last-match-wins is simple and powerful**: No complex priority system.
   Just append rules and the last one wins. Easy to reason about.

2. **Deferred/Promise pattern for async approval**: The tool blocks on a
   promise. The UI resolves it. Clean separation of concerns.

3. **Cascade on rejection is important UX**: When a user rejects, they
   probably want everything to stop, not just one tool. The cascade
   handles this without the user clicking reject N times.

4. **"Always" with auto-check is clever**: One approval can unlock multiple
   pending requests. Reduces prompt fatigue.

5. **Permission config format is ergonomic**: Simple string for uniform
   action, object for per-pattern rules:
   ```json
   { "read": "allow", "edit": { "*": "ask", "*.md": "allow" } }
   ```
