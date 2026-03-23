# Architecture Plan

How hob differentiates from existing coding agents. Based on analysis of
the 2026 agent landscape, production failure modes, and user complaints.

## Core thesis

Model capability is a commodity. The moat is in scaffolding, context
engineering, and tool design. Identical models score 17 percentage points
apart on SWE-bench Pro depending on the harness. hob wins by building
the tightest possible loop between LLM generation and compiler feedback.

## Priority 1: Compiler-in-the-loop verification ✅

The single biggest architectural advantage. No shipping agent treats
compilation and static analysis as real-time feedback during generation.
They compile after generating, then retry.

**What to build:**
- After every edit_file/write_file, run `cargo check` (Rust), `clang`
  (C/C++), `go vet` (Go), `tsc` (TypeScript), `python -m py_compile`
  (Python) and feed diagnostics directly into the agent's context as
  tool results
- Use pass/fail as a binary signal for the current trajectory — if the
  edit broke the build, the agent knows immediately and can fix it in
  the same turn rather than discovering it later
- For Rust specifically: the borrow checker is an extraordinarily precise
  oracle. A wrong edit surfaces immediately with actionable error messages.
  Run `clippy` for deeper analysis after successful compilation
- Run the project's test suite after multi-file changes and feed results
  back before reporting "done"

**Current state:** Basic LSP diagnostics appended to tool output. Needs
to become a core feedback loop, not an afterthought.

## Priority 2: Deterministic loop detection and budget enforcement ✅

The "loop of death" is the most wasteful failure mode across all agents.
No shipping agent has robust detection or per-subtask budget enforcement.

**What to build:**
- Track per-subtask token spend and wall-clock time
- If the agent calls the same tool with identical args 3+ times, force
  escalation: ask the user or switch strategy
- Per-task token budget with a warning at 80% and forced wrap-up at 100%
- Track edit attempts per file — if the same file has been edited 3+
  times without a successful build, force the agent to re-read the file
  and reconsider its approach

**Current state:** Not implemented.

## Priority 3: Multi-sample with verifier ✅

For complex tasks, sample N solutions in parallel and select the best
one using a compiler-based verifier. DeepSWE goes from 42% pass@1 to
59% at pass@16. Even pass@3 with a compiler verifier significantly
outperforms single-shot generation.

**What to build:**
- `/hard` mode or auto-detect for complex tasks
- Sample 3-5 solutions in parallel (separate agent runs)
- Run the compiler and test suite against each
- Select the one that compiles, passes tests, and has fewest warnings
- Especially powerful for Rust where the type system is a precise oracle

**Current state:** Not implemented. Requires the compiler-in-the-loop
infrastructure from Priority 1.

## Priority 4: Plan/act modal separation ✅

Explicit read-only exploration mode vs execution mode. Prevents the
"agent goes rogue" failure mode — the most emotionally charged user
complaint across all coding agents.

**What to build:**
- `/plan` command enters read-only mode: agent can read files, grep,
  run non-destructive commands, but cannot write/edit/delete
- Agent produces a structured plan with specific files and changes
- `/act` switches to execution mode and follows the plan
- Plan is visible in the TUI and can be edited before execution
- Background planning agent that continuously refines strategy while
  the execution model handles short-term actions

**Current state:** Not implemented. The permission system provides some
safety but doesn't enforce a planning phase.

## Priority 5: Aggressive context engineering (partial ✅)

Target 40-60% context utilization. Beyond ~40% fill, output quality
measurably degrades. A 200K context window effectively gives 80-120K
tokens of useful capacity.

**Done:** Compaction now triggers at 50% utilization instead of near the limit.
**Remaining:** Subagent delegation, AST-aware repo map, pre-computed context.

**What to build:**
- Auto-compaction at 40% utilization, not at the limit
- Protected recent window (last ~40K tokens never pruned)
- Subagent delegation: isolate exploratory searches in separate context
  windows that return summaries, not raw output
- AST-aware repo map using tree-sitter with PageRank-style ranking,
  dynamically sized based on available context budget
- Pre-compute cross-file context at indexing time (SpecAgent approach)
  to eliminate inference-time retrieval latency

**Current state:** Basic compaction with prune + summarize. Triggers
near the limit, not at optimal utilization. No subagent delegation,
no AST-aware repo map, no pre-computed context.

## Priority 6: OS-level sandboxing ✅

Enable autonomous operation without per-step approval prompts. This is
~500 lines of Rust and dramatically changes the trust model.

**What to build:**
- Landlock LSM on Linux: restrict filesystem access to the workspace
- Seatbelt profiles on macOS: equivalent sandboxing
- Three modes: read-only, workspace-write, full-access
- In sandbox mode, skip permission prompts entirely — the OS enforces
  the policy. This makes the agent much faster for trusted tasks
- Network policy: allow API calls, block everything else by default

**Current state:** Permission system with wildcard rules and async ask
flow. No OS-level enforcement.

## Priority 7: Git-native operation with auto-checkpointing ✅

Every edit step should create a git commit. This gives free rollback,
free diff visualization, and a complete audit trail.

**What to build:**
- Auto-commit after each successful tool execution that modifies files
- Commit messages generated from the tool call context
- `/revert` to undo the last N commits
- Use git worktrees for parallel agent isolation in multi-sample mode
- Diff visualization in the TUI after each commit

**Current state:** Snapshot system using a separate git repo. Not
commit-based, not visible in the TUI as diffs.

## Priority 8: Transparent cost tracking ✅

Display real-time USD cost in the UI. Address the most emotionally
charged user complaint (cost opacity).

**What to build:**
- Pricing table per model (input/output/cache per million tokens)
- Real-time USD cost display in the status bar
- Per-session and per-task cost breakdown
- Cost alerts at configurable thresholds
- Support local models as first-class citizens (cost = $0)

**Current state:** Token tracking (input/output counts). No USD
conversion, no pricing table.

## Priority 9: Architect/editor split

Separate the planning model from the edit-application model. The
reasoning model describes what to change; a specialized smaller model
or deterministic AST-aware tooling mechanically applies diffs.

**What to build:**
- Main model produces change descriptions in natural language or
  structured format
- Small local model or deterministic tooling applies the edits
- This eliminates a class of edit-parsing failures
- The edit applicator can be much faster than the planning model
- AST-aware edit application for languages with tree-sitter grammars

**Current state:** Single model does both planning and editing via the
edit_file tool's search-and-replace with fuzzy matching.

## Anti-patterns to avoid

- **Don't over-index on context window size.** Models effectively use
  8K-50K tokens regardless of advertised capacity. More context ≠
  better results.
- **Don't trust the model to generate secure code.** 45% of AI-generated
  code contains OWASP Top 10 vulnerabilities. Run static analysis.
- **Don't build a complex tool hierarchy.** Minimal tool sets (edit,
  grep, find, cat, run_command) outperform complex ones. Each tool
  must justify its existence.
- **Don't let the agent compile after generating.** Compile during
  generation. The tighter the feedback loop, the better.

## Implementation order

1. Compiler-in-the-loop (highest impact, leverages existing LSP infra)
2. Loop detection (trivial to implement, prevents worst failure mode)
3. Plan/act separation (addresses top user complaint)
4. Aggressive context engineering (improves output quality)
5. Multi-sample with verifier (requires 1, big win for hard problems)
6. OS-level sandboxing (enables autonomous operation)
7. Git-native auto-checkpointing (safety + audit trail)
8. Cost tracking in USD (user trust)
9. Architect/editor split (optimization, can come later)
