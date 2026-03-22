Review recent changes in this repository and provide a prioritized list of findings.

## Step 1: Gather the diff

First, run `git status` to check for uncommitted changes.

- **If there are uncommitted changes**: run `git diff HEAD` to get all unstaged and staged changes. Also run `git diff --cached` to see what's staged separately.
- **If the working tree is clean** (no uncommitted changes): review the latest commit instead. Run `git log -1 --format="%H %s"` to identify it, then `git diff HEAD~1..HEAD` to get the diff.

In either case, read any changed files in full so you have surrounding context, not just the diff hunks.

## Step 2: Rust code review (agent/)

For every changed Rust file, check for:

### Performance
- **Unnecessary `.clone()`** — flag any clone where a borrow or reference would work. Check whether the cloned value is used after the clone site; if not, it's needless.
- **Unneeded allocations** — `String` where `&str` suffices, `Vec` built just to iterate, `format!()` for static strings, `to_string()` on literals that could be `&'static str`.
- **Avoidable copies of large types** — passing large structs by value instead of by reference.
- **Hot-path `collect()` into a `Vec` that's immediately iterated** — suggest chaining iterators instead.
- **Boxing or `Arc`/`Rc` without clear shared-ownership need.**

### Correctness & robustness
- **`unwrap()` / `expect()` on fallible operations** that could realistically fail at runtime (network, IO, parsing user input). `unwrap()` is fine in tests or on infallible operations.
- **Missing error context** — bare `?` without `.context()` in functions where the caller would lose important info.
- **Off-by-one errors, integer overflow, or silent truncation.**
- **Async pitfalls** — holding a lock across an `.await`, blocking calls inside async functions.

### Tests
- If new public functions, structs, or IPC message types were added, **flag if no tests were added.**
- If existing tests were modified, **verify the modification is not weakening assertions or commenting out checks to make a failing test pass.** This is high priority — compare the old and new test carefully.
- If a bug was fixed, flag if there's no regression test.

### Style (only flag if it violates CLAUDE.md conventions)
- stdout writes outside IPC (CLAUDE.md: "Never write to stdout except IPC messages").
- Missing `serde` tag conventions on new IPC types.
- `println!` instead of `tracing` macros.

## Step 3: Elisp code review (lisp/)

For every changed Elisp file, check for:

### Correctness
- **Missing `lexical-binding: t`** in file headers.
- **Symbol prefix violations** — public symbols must use `hob-`, internal symbols `hob--`.
- **Buffer mutation without `inhibit-read-only`** when modifying special-mode buffers.
- **Process filter correctness** — partial line handling, JSON parsing error handling.

### Style & safety
- **`setq` on let-bound variables** outside their scope.
- **Missing `unwind-protect`** for cleanup that must run (process teardown, temp buffer cleanup).
- **Dynamic variable shadowing** that could confuse callers.
- **Unused `require` statements** added by the change.

### Tests
- Same philosophy as Rust: new public-facing functions should have tests; modified tests should not be weakened.

## Step 4: CLAUDE.md check

Read the current CLAUDE.md. If the diff introduces any of the following, flag that CLAUDE.md should be updated:
- New IPC message types
- New tool implementations
- Changed build commands or dependencies
- New conventions or architectural decisions
- Changes to the project structure (new directories, moved files)

If CLAUDE.md already reflects the changes, or the changes are minor internal refactors, no flag needed.

## Step 5: Output

Produce a single prioritized list of findings. Use this format:

```
## Review findings

### Critical (must fix before commit)
- [file:line] Description of issue

### Warning (should fix)
- [file:line] Description of issue

### Suggestion (nice to have)
- [file:line] Description of issue

### CLAUDE.md
- [update needed / up to date] — Description of what needs updating, if anything.
```

Rules for prioritization:
- **Critical**: correctness bugs, test weakening, security issues, data loss risks
- **Warning**: performance issues (needless clones/allocations), missing tests for new public API, missing error context, convention violations from CLAUDE.md
- **Suggestion**: minor style improvements, optional refactors, documentation gaps

If there are no findings at a given level, omit that section. If the diff is clean, say so.

Do NOT suggest changes to code that was not modified in the diff. Only review what changed.
