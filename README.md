# review-mcp

[![CI](https://github.com/Fuabioo/review-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/Fuabioo/review-mcp/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/Fuabioo/review-mcp/graph/badge.svg)](https://codecov.io/gh/Fuabioo/review-mcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Deterministic review workflow MCP server backed by SQLite. Replaces token-burning LLM operations (tmp file management, UUID generation, round detection, permission prompts) with fast, atomic, deterministic tool calls.

**1.5 MB binary. No async runtime. No `/tmp`. Persistent audit trail.**

## Install

### Homebrew (production)

```bash
brew tap Fuabioo/tap
brew install review-mcp
```

### From source (development)

```bash
git clone https://github.com/Fuabioo/review-mcp.git
cd review-mcp
just install   # builds release + copies to ~/.local/bin/review-mcp-dev
```

## Configure Claude Code

### Production (homebrew)

```bash
claude mcp add -s user review-mcp -- review-mcp serve
```

### Development

```bash
claude mcp add -s user review-mcp -- review-mcp-dev serve
```

### Verify

```bash
claude mcp get review-mcp
```

## Uninstall

### Homebrew

```bash
claude mcp remove review-mcp
brew uninstall review-mcp
```

### Development

```bash
claude mcp remove review-mcp
just uninstall
```

## Tools

| Tool | Description |
|------|-------------|
| `session_create` | Create a new review session (UUID + round 1 auto-created) |
| `session_get` | Get session by ID or find active session by target path |
| `round_start` | Begin next round (auto-detects number) |
| `review_write` | Write review content to slot (regular/harsh/grounded) — atomic |
| `review_read` | Read review content (defaults to latest round) |
| `round_status` | Check which reviews exist + round outcome |
| `round_set_outcome` | Mark round approved/rejected/conditional |
| `session_signal` | Cross-session signaling (addressed/acknowledged/needs_revision) |
| `session_signals` | Read all signals for a session |
| `session_list` | List sessions with filters (limit/offset/type/status) |

## CLI

```bash
# Version
review-mcp --version

# List all review sessions
review-mcp audit

# Filter by type and limit
review-mcp audit --type code --limit 5

# Show session detail (supports partial UUID match)
review-mcp audit a1b2c3d4

# Prune sessions older than 1 year (default)
review-mcp prune

# Prune sessions older than 90 days
review-mcp prune --days 90

# Preview what would be pruned
review-mcp prune --dry-run
```

### Audit output

```
SESSION ID                            TARGET                      TYPE  ROUNDS  STATUS  CREATED
------------------------------------------------------------------------------------------------------------------------
4b05c644-ef7b-4857-8c6a-ef59d6b3a20b /home/user/project/PLAN.md  plan  3       active  2026-04-07T12:00:00Z

$ review-mcp audit 4b05c644
Session: 4b05c644-ef7b-4857-8c6a-ef59d6b3a20b
Target:  /home/user/project/PLAN.md
Type:    plan
Status:  active
Created: 2026-04-07T12:00:00Z

Rounds:
  Round 1 [rejected] - 2026-04-07T12:00:00Z
    regular:  CODE_REVIEW_regular_r1.md (4523 bytes)
    harsh:    CODE_REVIEW_harsh_r1.md (6102 bytes)
    grounded: GROUNDED_REVIEW_r1.md (3891 bytes)
  Round 2 [pending] - 2026-04-07T13:00:00Z
    regular:  (missing)
    harsh:    (missing)
    grounded: (missing)

Signals:
  [2026-04-07T12:30:00Z] worker-regular: addressed
  [2026-04-07T12:35:00Z] orchestrator: acknowledged
```

## Storage

All data lives in `~/.local/share/review-mcp/`:

```
~/.local/share/review-mcp/
├── reviews.db                              # SQLite (WAL mode)
└── sessions/
    └── <uuid>/
        └── round_<N>/
            ├── CODE_REVIEW_regular_r<N>.md
            ├── CODE_REVIEW_harsh_r<N>.md
            └── GROUNDED_REVIEW_r<N>.md
```

## Design

- **No `/tmp`** — no permission prompts, persistent across reboots
- **SQLite WAL mode** — concurrent reads from multiple Claude sessions
- **UNIQUE constraints** — first writer wins, duplicates get a clean error
- **Atomic file writes** — write to temp, fsync, rename (no partial reads)
- **No async runtime** — pure `std::io`, MCP stdio is sequential
- **Enum-based errors** — no `unwrap()` in production code

## License

MIT
