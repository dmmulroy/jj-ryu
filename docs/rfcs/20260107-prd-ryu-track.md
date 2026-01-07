# PRD: `ryu track` — Explicit Bookmark Tracking

**Status:** Draft  
**Author:** OpenCode  
**Date:** 2026-01-07  
**Scope:** New commands `ryu track`, `ryu untrack`; modifications to `ryu submit`, `ryu sync`, `ryu` (analyze)

---

## Problem Statement

### Current Behavior (Implicit Discovery)

ryu discovers the stack implicitly via jj's `trunk()..@` revset:
- Every bookmark between trunk and working copy is a candidate for submission
- No persistence across runs — each invocation re-discovers the stack
- No association between bookmarks and their PRs stored locally

### Pain Points

| Problem | Impact |
|---------|--------|
| **Multiple independent stacks** | Can't have 2+ unrelated feature stacks without switching working copy |
| **Partial stack submission** | `-i/--select` works but doesn't persist — must re-select each time |
| **PR association** | Every `ryu submit` must query platform API to find existing PRs |
| **Multi-remote scenarios** | No way to associate different bookmarks with different remotes |
| **Accidental submission** | Easy to submit a WIP bookmark you didn't intend to |

### Why Explicit Tracking?

Graphite's model demonstrates that explicit tracking provides:
1. **Intent clarity** — User explicitly declares "this is my stack"
2. **State persistence** — Remember selections across sessions
3. **Faster operations** — Local PR# cache avoids API calls for status display
4. **Multi-stack support** — Track different bookmarks independently

---

## Goals

| Priority | Goal |
|----------|------|
| P0 | Explicit bookmark tracking with `ryu track` / `ryu untrack` |
| P0 | Tracked bookmarks persist across sessions |
| P0 | `ryu submit` respects tracking (only submit tracked bookmarks) |
| P1 | Local PR association cache (bookmark → PR#/URL) |
| P1 | `ryu` visualization distinguishes tracked vs untracked |
| P2 | Multi-remote tracking (bookmark → remote association) |

### Non-Goals (v1)

- `ryu status` (dedicated command for tracked state) — use `ryu` (analyze)
- `ryu reorder` (reorder tracked stack) — use jj directly
- Named stacks (grouping bookmarks into named collections)
- Syncing tracking metadata across machines

---

## Design

### 1. Tracking Granularity: Bookmark-centric

Track individual bookmarks. Stack relationships are derived from jj's commit graph at runtime — no need to store parent-child metadata (jj already knows this).

**Why not stack-centric?**
- jj's graph already encodes relationships
- Named stacks add complexity without clear benefit
- Bookmarks can be freely rearranged in jj; tracking follows the bookmark, not position

### 2. Metadata Storage: `.jj/repo/ryu/`

Store tracking metadata inside jj's repo directory:

```
.jj/
└── repo/
    └── ryu/
        ├── tracked.toml       # Tracked bookmarks
        └── pr_cache.toml      # PR associations (optional cache)
```

**Why `.jj/repo/`?**
- Workspace-specific (not global)
- Follows jj conventions (alongside other repo state)
- Not committed to git (lives inside `.jj/`)
- Clean separation from jj internals (own subdirectory)

### 3. Data Model

#### `tracked.toml`

```toml
# ryu tracking metadata
# Auto-generated — manual edits may be overwritten

version = 1

[[bookmarks]]
name = "feat-auth"
change_id = "abc123"          # For rename detection
remote = "origin"             # Optional, defaults to auto-detect
tracked_at = 2026-01-07T10:30:00Z

[[bookmarks]]
name = "feat-auth-tests"
change_id = "def456"
remote = "origin"
tracked_at = 2026-01-07T10:31:00Z

[[bookmarks]]
name = "unrelated-fix"
change_id = "ghi789"
remote = "upstream"           # Different remote
tracked_at = 2026-01-07T11:00:00Z
```

#### `pr_cache.toml`

```toml
# PR association cache — regenerated from platform API on submit
# Safe to delete; will be rebuilt on next submit

version = 1

[[prs]]
bookmark = "feat-auth"
number = 123
url = "https://github.com/owner/repo/pull/123"
remote = "origin"
updated_at = 2026-01-07T10:35:00Z

[[prs]]
bookmark = "feat-auth-tests"
number = 124
url = "https://github.com/owner/repo/pull/124"
remote = "origin"
updated_at = 2026-01-07T10:35:00Z
```

### 4. CLI Interface

#### `ryu track [bookmark...]`

Track one or more bookmarks for submission.

```
USAGE:
    ryu track [OPTIONS] [BOOKMARK]...

ARGS:
    [BOOKMARK]...    Bookmarks to track (interactive selection if omitted)

OPTIONS:
    -a, --all        Track all bookmarks in trunk()..@
    -r, --remote     Associate with specific remote (default: auto-detect)
    -f, --force      Re-track already-tracked bookmarks (update remote)
    -h, --help       Print help
```

**Behavior:**
- No args → Interactive multi-select from untracked bookmarks in `trunk()..@`
- With args → Track specified bookmarks (error if not in graph)
- `--all` → Track everything in `trunk()..@`
- Already tracked → Skip (or update with `--force`)

**Output:**
```
$ ryu track feat-auth feat-auth-tests
Tracked 2 bookmarks:
  ✓ feat-auth
  ✓ feat-auth-tests

$ ryu track
? Select bookmarks to track: (space to select, enter to confirm)
  [ ] feat-db-migration
  [x] feat-auth
  [x] feat-auth-tests
  [ ] wip-experiments

Tracked 2 bookmarks:
  ✓ feat-auth
  ✓ feat-auth-tests
```

#### `ryu untrack [bookmark...]`

Stop tracking bookmarks.

```
USAGE:
    ryu untrack [OPTIONS] [BOOKMARK]...

ARGS:
    [BOOKMARK]...    Bookmarks to untrack (interactive selection if omitted)

OPTIONS:
    -a, --all        Untrack all tracked bookmarks
    -h, --help       Print help
```

**Behavior:**
- No args → Interactive multi-select from tracked bookmarks
- With args → Untrack specified bookmarks
- `--all` → Untrack everything
- Untracking does NOT delete branches or close PRs — only removes from tracking

**Output:**
```
$ ryu untrack feat-auth-tests
Untracked 1 bookmark:
  ✓ feat-auth-tests

Note: PR #124 remains open. Close manually if needed.
```

### 5. Modified Commands

#### `ryu` (analyze/visualize)

Update visualization to show tracking status:

```
$ ryu

Stack: 3 bookmarks (2 tracked)

  ┌─ feat-auth-tests          ✓ #124  ← tracked, has PR
  ├─ feat-auth                ↑ #123  ← tracked, needs push
  └─ feat-db-migration        ·       ← untracked

  (use 'ryu track' to track untracked bookmarks)

@ Working copy at feat-auth-tests
```

**Legend:**
- `✓` — Tracked, synced with remote
- `↑` — Tracked, needs push
- `·` — Untracked (dimmed)
- `#123` — Associated PR number (from cache)

#### `ryu submit`

Change default behavior based on tracking state:

| Tracking State | `ryu submit` Behavior |
|----------------|----------------------|
| No bookmarks tracked | Error: "No bookmarks tracked. Run `ryu track` first" |
| Some bookmarks tracked | Submit only tracked bookmarks |
| `--all` flag | Submit all in `trunk()..@` (ignore tracking) |

**New flags:**
```
OPTIONS:
    --all                Submit all bookmarks in trunk()..@ (ignore tracking)
    --include-untracked  Also submit untracked bookmarks in selection
```

**Error on no tracking:**
```
$ ryu submit
Error: No bookmarks tracked.

Run 'ryu track' to select bookmarks, or 'ryu track --all' to track everything in trunk()..@
```

#### `ryu sync`

Same tracking-aware behavior as `submit`.

### 6. No Implicit Fallback

Explicit tracking is **required**. No legacy/implicit mode:

1. **No tracking file or empty** → Error: "No bookmarks tracked. Run `ryu track` first"
2. **Tracking file has bookmarks** → Use tracked bookmarks only

First-time UX:
```
$ ryu submit
Error: No bookmarks tracked.

Run 'ryu track' to select bookmarks, or 'ryu track --all' to track everything in trunk()..@
```

### 7. Bookmark Rename Detection

Tracking stores `change_id` alongside bookmark name. On load:

1. For each tracked entry, check if bookmark name still points to stored `change_id`
2. If mismatch, search for bookmark pointing to that `change_id`
3. If found → Auto-update tracking with new name, log info message
4. If not found (bookmark deleted) → Mark as stale, warn user

```
$ ryu submit
Info: Tracked bookmark 'feat-auth' was renamed to 'feature/auth'. Updated tracking.
```

### 8. PR Cache Management

**On `ryu submit`:**
1. After successful PR creation/update, write to `pr_cache.toml`
2. Cache includes: bookmark name, PR#, URL, remote, timestamp

**On `ryu` (analyze):**
1. Read `pr_cache.toml` for display (no API call)
2. Show PR# next to tracked bookmarks
3. Cache miss → show `?` or omit PR#

**Cache invalidation:**
- Manual delete of `pr_cache.toml` → Rebuilt on next submit
- Stale entries (bookmark deleted) → Cleaned on next submit
- No TTL — cache is source of truth until next submit

### 9. Scope Flag Interaction

Scope flags (`--upto`, `--only`, `--stack`) filter **within** the tracked set:

| Command | Behavior |
|---------|----------|
| `ryu submit` | Submit all tracked bookmarks |
| `ryu submit --upto feat-a` | Submit tracked bookmarks up to feat-a |
| `ryu submit --only` | Submit only the target tracked bookmark |
| `ryu submit -i` | Interactive select from all bookmarks, pre-select tracked |

### 10. Interactive Select Behavior

When using `-i/--select`:
- Show all bookmarks in `trunk()..@`
- Pre-select tracked bookmarks
- Selection does NOT modify tracking (one-time override)
- To persist selection, use `ryu track` separately

---

## User Stories

### US1: Track specific bookmarks
```
As a developer with multiple WIP bookmarks
I want to track only the bookmarks I intend to submit
So that I don't accidentally create PRs for experimental work
```

**Acceptance:**
- `ryu track feat-a feat-b` adds both to tracked.toml
- `ryu submit` only submits feat-a and feat-b
- Untracked bookmarks in `trunk()..@` are ignored

### US2: Interactive bookmark selection
```
As a developer with many bookmarks
I want to interactively select which to track
So that I can easily manage my stack
```

**Acceptance:**
- `ryu track` (no args) shows multi-select prompt
- Only untracked bookmarks in `trunk()..@` shown
- Selected bookmarks added to tracked.toml

### US3: See tracking status at a glance
```
As a developer
I want to see which bookmarks are tracked vs untracked
So that I know what will be submitted
```

**Acceptance:**
- `ryu` (analyze) shows tracking status indicator
- Tracked bookmarks show PR# from cache
- Untracked bookmarks visually distinguished (dimmed)

### US4: Untrack bookmarks
```
As a developer
I want to stop tracking a bookmark without deleting it
So that I can exclude it from submission while keeping the branch
```

**Acceptance:**
- `ryu untrack feat-a` removes from tracked.toml
- PR remains open (note shown to user)
- Bookmark remains in jj graph

### US5: Bookmark rename handling
```
As a developer who renames bookmarks
I want tracking to follow the bookmark
So that I don't have to re-track after renaming
```

**Acceptance:**
- Rename bookmark via jj → tracking auto-updates on next ryu command
- Info message shown about the rename
- PR association preserved

---

## Implementation Plan

### Phase 1: Tracking Infrastructure
- [ ] Create `src/tracking/` module
- [ ] Implement `TrackedBookmark` struct
- [ ] Implement `tracked.toml` read/write (serde)
- [ ] Implement `pr_cache.toml` read/write (serde)
- [ ] Add `change_id` lookup to `JjWorkspace`
- [ ] Unit tests for serialization

### Phase 2: `ryu track` Command
- [ ] Add `Track` variant to `Commands` enum
- [ ] Implement `run_track()` in cli module
- [ ] Interactive selection (dialoguer)
- [ ] `--all`, `--remote`, `--force` flags
- [ ] Integration tests

### Phase 3: `ryu untrack` Command
- [ ] Add `Untrack` variant to `Commands` enum
- [ ] Implement `run_untrack()` in cli module
- [ ] Interactive selection for untrack
- [ ] `--all` flag
- [ ] Integration tests

### Phase 4: Update Visualization
- [ ] Modify `run_analyze()` to load tracking state
- [ ] Add tracking indicators to output
- [ ] Show PR# from cache
- [ ] Dim untracked bookmarks
- [ ] Add hint about `ryu track`

### Phase 5: Update Submit/Sync
- [ ] Load tracking state at start
- [ ] Filter bookmarks based on tracking
- [ ] Add `--all`, `--include-untracked` flags
- [ ] Update PR cache after submit
- [ ] Prompt flow for empty tracking
- [ ] Integration tests

### Phase 6: Rename Detection
- [ ] Store `change_id` on track
- [ ] Implement rename detection on load
- [ ] Auto-update tracking on rename
- [ ] Stale entry cleanup
- [ ] Integration tests

---

## Testing Strategy

### Unit Tests
- `tracked.toml` serialization/deserialization
- `pr_cache.toml` serialization/deserialization
- Tracking state queries (is_tracked, get_tracked, etc.)
- Rename detection logic

### Integration Tests
- `ryu track` creates tracking file
- `ryu track` with existing file appends
- `ryu track --all` tracks everything
- `ryu untrack` removes entries
- `ryu untrack --all` clears tracking
- `ryu submit` respects tracking
- `ryu submit` errors when no tracking
- `ryu submit --all` ignores tracking
- `ryu submit -i` pre-selects tracked
- PR cache population on submit
- Bookmark rename → tracking auto-update

### E2E Tests
- Full workflow: track → submit → untrack
- PR cache accuracy after submit
- Multi-remote scenarios
- Rename detection with real jj operations

---

## Decisions Log

| Question | Decision | Rationale |
|----------|----------|-----------|
| Scope flag interaction | Filter within tracked | Keeps tracking as source of truth; `--all` for override |
| Bookmark rename handling | Auto-detect via change_id | Better UX; jj users frequently rename bookmarks |
| Tracking scope | Workspace-specific | Matches jj's workspace model |
| `-i/--select` behavior | Show all, pre-select tracked | More flexible; one-time override without modifying tracking |
