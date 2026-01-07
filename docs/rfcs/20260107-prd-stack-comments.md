# PRD: Enhanced Stack Comments

**Status:** Draft  
**Author:** OpenCode  
**Date:** 2026-01-07  
**Scope:** Improvements to PR stack comment format and behavior

---

## Problem Statement

### Current Behavior

ryu adds stack navigation comments to each PR:

```
* #3 👈
* #2
* #1

---
This stack of pull requests is managed by jj-ryu.
```

### Issues

| Problem | Impact |
|---------|--------|
| **No PR titles** | Must click each link to understand what PRs are in the stack |
| **No base branch** | Unclear what the stack targets (main? develop?) |
| **Comments on single PRs** | Unnecessary noise when stack size = 1 |

### Graphite's Format

Graphite includes richer context:

```
* fix: take nullsFirst sort into account #19222 👈 (View in Graphite)
* main

This stack of pull requests is managed by Graphite.
```

Key differences:
1. **PR title** shown inline — no need to click
2. **Base branch** at bottom — shows stack target
3. Still shows comment for single PRs (we could deviate here)

---

## Goals

| Priority | Goal |
|----------|------|
| P0 | Include PR title in stack comment |
| P0 | Show base branch (trunk) at bottom of stack |
| P2 | Link to PR URL instead of just `#N` |

### Non-Goals (v1)

- External links (e.g., "View in ryu" — we're CLI-only)
- Custom comment templates
- Comment on base branch PR

---

## Design

### 1. Enhanced `StackItem` Structure

```rust
pub struct StackItem {
    pub bookmark_name: String,
    pub pr_url: String,
    pub pr_number: u64,
    pub pr_title: String,  // NEW
}
```

PR title fetched during submission (already have PR data from create/update).

### 2. New `StackCommentData` Fields

```rust
pub struct StackCommentData {
    pub version: u8,
    pub stack: Vec<StackItem>,
    pub base_branch: String,  // NEW: e.g., "main"
}
```

### 3. Updated Comment Format

```
* fix: add logout endpoint #3 👈
* feat: add session management #2
* feat: add auth #1
* `main`

---
This stack of pull requests is managed by [jj-ryu](https://github.com/dmmulroy/jj-ryu).
```

Format details:
- PR title + `#N` on same line
- Current PR marked with 👈 and **bold**
- Base branch at bottom in backticks (not a PR, visual distinction)
- Newest/leaf at top, oldest at bottom (current behavior)

### 4. Data Flow

```
execute_submission()
  └─> build_stack_comment_data(plan, bookmark_to_pr, trunk_name)
        └─> For each segment:
              - Get PR number, URL from bookmark_to_pr
              - Get PR title from PullRequest struct (already have it)
              - Set base_branch from workspace.trunk_name()
  └─> format_stack_comment(data, current_idx)
        └─> Skip if stack.len() == 1
        └─> Format with titles + base branch
```

---

## Implementation Plan

### Phase 1: Add PR Title to Stack Comments

1. Add `pr_title` field to `StackItem`
2. Update `build_stack_comment_data` to populate title from `PullRequest`
3. Update `format_stack_comment` to include title
4. Update tests

### Phase 2: Add Base Branch

1. Add `base_branch` field to `StackCommentData`
2. Pass trunk name through to comment builder
3. Append base branch line to comment format
4. Update tests

---

## Migration

### Comment Format Versioning

`StackCommentData.version` already exists. Bump to `1` for new format.

Old comments (v0) will be replaced on next `ryu submit` — no migration needed.

### Backward Compatibility

- New ryu can read old comments (for detection/replacement)
- Old ryu cannot parse new format (acceptable — users should update)

---

## Testing

| Test Case | Type |
|-----------|------|
| `format_stack_comment` includes title | Unit |
| `format_stack_comment` includes base branch | Unit |
| Single-PR stack skips comment | Unit |
| Multi-PR stack creates comment | Integration |
| Comment updated on re-submit | E2E |

---

## Decisions

1. **Truncate long titles?** — No, show full title (matches Graphite)
2. **Delete stale single-PR comments?** — No, leave existing comments
3. **Link format** — Use `#N` (GitHub auto-links, matches Graphite)
