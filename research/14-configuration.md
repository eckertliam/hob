# Configuration Loading

How the harness resolves configuration from multiple sources.

## Config Precedence (lowest to highest)

```
1. Remote org config        (fetched from .well-known endpoints)
2. Global config            (~/.config/agent/config.json)
3. Custom config            ($AGENT_CONFIG environment variable)
4. Project config           (agent.json walked up from directory)
5. Project .agent/ dir      (.agent/config.json + subdirectories)
6. Inline config            ($AGENT_CONFIG_CONTENT env var)
7. Account/org config       (fetched from account API)
8. Managed config           (admin-controlled, enterprise override)
```

## Merge Rules

- Objects: deep merge (nested fields merged recursively)
- `plugin` arrays: concatenated (not replaced)
- `instructions` arrays: concatenated
- Everything else: later value wins

## Project Discovery

```
1. Walk up directory tree looking for .git
2. If found:
   - Get worktree root (git rev-parse --show-toplevel)
   - Generate project ID from hash of first root commit
   - Cache ID for future lookups
3. If not found:
   - Use global project ID
   - No VCS features available
```

## Database Setup (if using SQLite)

Key pragmas for concurrent access:

```
journal_mode = WAL          ← concurrent reads while writing
synchronous = NORMAL        ← balance safety and speed
busy_timeout = 5000         ← wait 5s on lock before failing
cache_size = -64000         ← 64MB in-memory cache
foreign_keys = ON           ← enforce referential integrity
```

WAL mode is essential -- without it, the database locks during the agent
loop and the UI can't read messages.
