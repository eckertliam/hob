# Built-In Tool Implementations

**Source**: `packages/opencode/src/tool/`

## Tool Summary

| Tool | Purpose | Key Insight |
|------|---------|-------------|
| bash | Shell execution | AST-parsed commands for granular permissions |
| read | File reading | Binary detection, pagination, image support |
| edit | Find-and-replace | 9 cascading fuzzy matchers for robustness |
| write | File creation/overwrite | Requires prior read (prevents blind writes) |
| glob | File finding | Ripgrep-powered, sorted by modification time |
| grep | Content search | Ripgrep, grouped by file, line-truncated |
| batch | Parallel execution | Up to 25 tools in parallel via Promise.all |
| task | Subagent launching | Creates child session, restricted permissions |

## Bash Tool

The most complex tool. Spawns shell processes with full lifecycle management.

**Parameters:**
- `command`: string - the command to run
- `timeout?`: number - ms (default 120,000 = 2 min)
- `workdir?`: string - working directory
- `description`: string - 5-10 word description (shown in UI)

**Permission strategy:**
```
1. Parse command with tree-sitter into AST
2. Extract accessed paths from AST nodes:
   - cd, rm, cp, mv, mkdir, touch, chmod, chown, cat arguments
3. For each path outside project:
   → Request "external_directory" permission
4. Request "bash" permission for the command itself
```

AST-level analysis means the harness knows which files a command will touch
before executing it. This enables granular permissions instead of blanket
"allow all bash" rules.

**Execution:**
- `detached: true` on Unix (process group for clean kill)
- stdout/stderr piped and accumulated in real-time
- Metadata updated during execution (streaming output to UI)
- Graceful shutdown: SIGTERM → wait → SIGKILL
- Timeout with 100ms grace period

**Output truncation:**
- Metadata capped at 30KB
- Full output returned to LLM
- Timeout/abort info appended in `<bash_metadata>` tags

## Edit Tool (Find-and-Replace)

The edit tool uses **9 cascading replacer strategies** to find the target
text, even when the LLM's output doesn't exactly match:

```
1. SimpleReplacer       → exact string match
2. LineTrimmedReplacer  → ignore line-level whitespace
3. BlockAnchorReplacer  → match first/last lines as anchors,
                          fuzzy match middle with Levenshtein
                          (threshold: 0.0 single match, 0.3 multiple)
4. WhitespaceNormalized → collapse all whitespace to single spaces
5. IndentationFlexible  → ignore indentation differences
6. EscapeNormalized     → handle escape sequences (\n, \t, etc.)
7. TrimmedBoundary      → try trimmed versions
8. ContextAware         → use surrounding lines as anchors,
                          50%+ middle-line match requirement
9. MultiOccurrence      → all exact matches (for replaceAll)
```

Each replacer is tried in order. First match wins. This cascade makes edits
robust to the common failure modes where the LLM generates slightly wrong
whitespace, indentation, or escape sequences.

**Additional features:**
- File lock prevents concurrent edits
- CRLF/LF detection and preservation
- LSP validation after edit (shows up to 20 errors)
- Diff in metadata for UI display

## Read Tool

**Smart file handling:**
- Text files: readline with line numbers, 2000-line default limit
- Images/PDFs: returned as base64 attachments
- Directories: sorted listing with pagination
- Binary files: detected and rejected with helpful message

**Binary detection:**
```
1. Check file extension against known binary formats
2. If unknown: read sample of content
3. Check for null bytes or >30% non-printable characters
4. If binary → error with type identification
```

**Output limits:**
- 2000 chars per line (truncated with `...`)
- 50KB total output
- Pagination via offset/limit parameters
- "File not found" shows similar filename suggestions

## Write Tool

Simple but with a critical safety feature:

```
write(filePath, content):
  1. FileTime.assert(filePath)
     → Verifies the file was previously READ in this session
     → Prevents blind overwrites of files the agent hasn't seen

  2. Compute diff against existing content (or empty if new)
  3. Write content
  4. Publish events
  5. LSP validation
```

The "must read before write" invariant prevents a common failure mode where
the agent overwrites a file it hasn't actually looked at.

## Glob Tool

- Uses ripgrep for file enumeration (fast, respects .gitignore)
- Results sorted by modification time (newest first)
- Hard cap at 100 results
- Returns absolute paths

## Grep Tool

- Spawns ripgrep with regex pattern
- Results grouped by file with line numbers
- 2000-char per-line truncation
- Hard cap at 100 matches
- Handles broken symlinks and inaccessible paths gracefully

## Batch Tool

Enables the LLM to parallelize I/O:

```
batch({
  tool_calls: [
    { tool: "read", parameters: { filePath: "a.ts" } },
    { tool: "read", parameters: { filePath: "b.ts" } },
    { tool: "grep", parameters: { pattern: "TODO" } },
  ]
})
```

- Max 25 calls per batch
- All execute in parallel (Promise.all)
- Each gets its own ToolPart with state tracking
- Partial failures don't block other calls
- "batch" cannot be nested (no batch-in-batch)
- Returns combined title: "Batch execution (X/Y successful)"

## Task Tool (Subagent Launching)

Creates a child session for delegated work:

```
task({
  description: "Search for auth implementations",
  prompt: "Find all files related to authentication...",
  subagent_type: "explore",
  task_id?: "resume_previous_session"
})
```

**Session creation:**
- Child session linked to parent via `parentID`
- Inherits working directory (same Instance.directory)
- Restricted permissions: no todoread/todowrite, optionally no nested tasks
- Gets subagent's system prompt and tool restrictions

**Result:**
- Output wrapped in `<task_result>` tags
- Returns `task_id` for potential resumption
- Full child session history preserved independently
- Parent agent sees result as a tool output

**Key insight: subagents share the parent's working directory.** They are
NOT isolated in separate worktrees by default. Git worktrees are a separate
feature for user-initiated isolation.

## Output Truncation System

All tools go through a truncation layer:

```
Limits:
  2000 lines OR 50KB (whichever hits first)
  Direction: "head" (default) or "tail"

When truncated:
  1. Save full output to ~/.local/share/opencode/truncation/{tool_id}
  2. Return truncated output with message:
     "[output truncated, full output saved to {path}]"
  3. If agent has task permission:
     Suggest delegating to subagent for large outputs
  4. Cleanup: files older than 7 days pruned hourly
```

## Design Insights for Emacs

1. **The edit cascade is the most valuable pattern**: LLMs often generate
   imperfect find-and-replace strings. The 9-level cascade (exact → fuzzy)
   dramatically improves edit reliability. Implement at least 3-4 levels.

2. **AST-based permission for bash**: Parsing the command into an AST before
   executing lets you ask targeted permission questions ("this command will
   modify /etc/hosts, allow?") instead of blanket "allow bash?".

3. **Read-before-write invariant**: Simple but prevents catastrophic data
   loss. Track which files have been read and block writes to unread files.

4. **Batch tool reduces round-trips**: Each LLM call has latency. The batch
   tool lets the model request 25 operations at once instead of one at a
   time. Critical for performance on tasks that need many file reads.

5. **Output truncation with save**: Don't just truncate. Save the full
   output for reference. The LLM might need it later (via a subagent that
   reads the saved file).

6. **Ripgrep for glob/grep**: Don't shell out to `find` and `grep`. Ripgrep
   is orders of magnitude faster and handles .gitignore automatically.
