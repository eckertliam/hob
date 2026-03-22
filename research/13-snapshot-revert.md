# Snapshot and Revert

Algorithm for tracking and reverting file changes during agent execution.
Uses a **separate git repo** (not the user's) with the same work tree.

## Core Operations

### Capture state (before/after each LLM step)

```
track():
  1. git add .                    (stage all changes)
  2. hash = git write-tree        (create tree object, returns SHA)
  3. Return hash
```

Tree objects are lightweight -- just a reference, not a full commit.

### Compute what changed

```
patch(beforeHash):
  1. git add .
  2. git diff --no-ext-diff --name-only {beforeHash}
  3. Return { hash: beforeHash, files: [changed paths] }
```

Store this as a PatchPart in the message history. It's the audit trail.

### Revert files

```
revert(patches):
  seen = Set()

  for each patch in patches:
    for each file in patch.files:
      if seen has file → skip
      seen.add(file)

      try:
        git checkout {patch.hash} -- {file}
      catch:
        result = git ls-tree {patch.hash} -- {file}
        if empty:
          rm {file}              ← file was new, delete it
        else:
          leave alone            ← checkout failed for other reason
```

Three cases: modified (restore), created (delete), deleted (restore).
The `ls-tree` check distinguishes new files from existing ones.

### Full restoration (for unrevert)

```
restore(hash):
  git read-tree {hash}
  git checkout-index -a -f
```

## Integration with the Agent Loop

```
start-step:
  beforeHash = track()

[model generates text, calls tools, modifies files]

finish-step:
  afterHash = track()
  changed = patch(beforeHash)
  store PatchPart { hash: beforeHash, files: changed }
```

## End-to-End Revert Flow

```
User triggers revert on a message
  │
  ├─ Load messages, find the revert point
  ├─ Collect all PatchParts from that point onward
  ├─ Save current state: undoHash = track()
  ├─ Apply: revert(patches)
  ├─ Store undoHash so unrevert can restore
  └─ Delete reverted messages/parts from DB
```

Unrevert: `restore(undoHash)` to get back to pre-revert state.

## The Separate Git Repo

```
User's project:  /project/.git/          ← user's repo (untouched)
Snapshot repo:   ~/.data/snapshot/{id}/   ← harness's repo
Work tree:       /project/               ← shared by both
```

Both repos point at the same work tree. The snapshot repo captures state
independently of the user's commit history. Periodic `git gc --prune=7.days`
keeps disk usage bounded.
