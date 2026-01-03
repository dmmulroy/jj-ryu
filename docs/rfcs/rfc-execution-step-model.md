# RFC: Unified Execution Step Model for Submission Plans

**Status:** Approved
**Author:** OpenCode
**Reviewer:** dmmulroy
**Date:** 2026-01-03  
**Scope:** `src/submit/`, `src/cli/`, `tests/`

---

## Summary

Refactor the submission planning system from a collection-based model (separate vectors for pushes, creates, updates, publishes) to a unified `ExecutionStep` enum with dependency-aware topological ordering. This enables correct handling of stack reordering scenarios and simplifies the execution and display logic throughout the codebase.

**Amendment (2026-01-03):** Add typed `ExecutionConstraint` enum to make invalid constraint pairings unrepresentable at compile time.

---

## Motivation

### Problem Statement

The current `SubmissionPlan` structure maintains separate collections:

```rust
// BEFORE
pub struct SubmissionPlan {
    pub bookmarks_needing_push: Vec<Bookmark>,
    pub prs_to_create: Vec<PrToCreate>,
    pub prs_to_update_base: Vec<PrBaseUpdate>,
    pub prs_to_publish: Vec<PullRequest>,
    // ...
}
```

This design has several deficiencies:

1. **Implicit Ordering**: The execution order is hardcoded in `execute_submission()` as "push all, then update all, then create all, then publish all". This is incorrect for stack swap scenarios.

2. **Stack Swap Bug**: When a user reorders commits (e.g., swapping A and B in a stack), the naive "push then retarget" order fails:
   - If B was based on A and is now the root, we must retarget B's PR *before* pushing A
   - Otherwise GitHub/GitLab rejects the base change due to branch history conflicts

3. **Display Logic Duplication**: `submit.rs`, `sync.rs`, and `execute.rs` all have similar `if !vec.is_empty() { println!(...) }` blocks that are tedious to maintain.

4. **Testing Difficulty**: Verifying execution order requires mocking the entire execution flow rather than testing the scheduler in isolation.

5. **Stringly-Typed Constraints**: Edge construction uses raw `usize` indices and string-based HashMap lookups, making it easy to mix up index types or constraint endpoints without compile-time detection.

### Goals

- Correct handling of stack reordering scenarios
- Single source of truth for execution order
- Simplified display and dry-run logic via `Display` implementations
- Isolated, unit-testable scheduler
- **Type-safe constraint representation** — invalid constraint pairings rejected at compile time
- Maintain backward compatibility for all existing workflows

---

## Design

### Core Abstraction: `ExecutionStep`

```rust
pub enum ExecutionStep {
    Push(Bookmark),
    UpdateBase(PrBaseUpdate),
    CreatePr(PrToCreate),
    PublishPr(PullRequest),
}
```

Each variant represents an atomic operation. The `SubmissionPlan` now holds an ordered `Vec<ExecutionStep>`:

```rust
// AFTER
pub struct SubmissionPlan {
    pub segments: Vec<NarrowedBookmarkSegment>,
    pub constraints: Vec<ExecutionConstraint>,  // NEW: for debugging/display
    pub execution_steps: Vec<ExecutionStep>,
    pub existing_prs: HashMap<String, PullRequest>,
    pub remote: String,
    pub default_branch: String,
}
```

### Typed Constraint System

Dependencies between operations are expressed as a typed enum:

```rust
/// Typed reference to a Push operation by bookmark name.
struct PushRef(String);

/// Typed reference to an UpdateBase operation by bookmark name.
struct UpdateRef(String);

/// Typed reference to a CreatePr operation by bookmark name.
struct CreateRef(String);

/// Dependency constraint between execution operations.
pub enum ExecutionConstraint {
    /// Push parent branch before child branch (stack order).
    PushOrder { parent: PushRef, child: PushRef },

    /// Push new base branch before retargeting PR to it.
    PushBeforeRetarget { base: PushRef, pr: UpdateRef },

    /// Retarget PR before pushing its old base (swap scenario).
    RetargetBeforePush { pr: UpdateRef, old_base: PushRef },

    /// Push branch before creating PR for it.
    PushBeforeCreate { push: PushRef, create: CreateRef },

    /// Create parent PR before child PR (stack comment linking).
    CreateOrder { parent: CreateRef, child: CreateRef },
}
```

**Why typed refs?** Each constraint variant accepts only the appropriate ref types:

```rust
// COMPILE ERROR: expected PushRef, found UpdateRef
ExecutionConstraint::PushOrder {
    parent: UpdateRef("a".into()),  // ✗
    child: PushRef("b".into()),
}

// CORRECT
ExecutionConstraint::PushOrder {
    parent: PushRef("a".into()),    // ✓
    child: PushRef("b".into()),
}
```

### Three-Phase Execution Planning

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐    ┌──────────────┐
│ collect_        │───▶│ build_execution │───▶│ resolve_        │───▶│ topo_sort_   │
│ constraints()   │    │ _nodes()        │    │ constraints()   │    │ steps()      │
└─────────────────┘    └─────────────────┘    └─────────────────┘    └──────────────┘
  Declarative           Nodes + Registry       Edges (Vec<Vec<       Sorted steps
  constraints           (NodeRegistry)         usize>>)
```

1. **collect_constraints()**: Build typed `ExecutionConstraint` values declaratively, without indices
2. **build_execution_nodes()**: Create nodes and populate `NodeRegistry` mapping bookmark names to indices
3. **resolve_constraints()**: Convert constraints to `(from, to)` edges via registry lookup
4. **topo_sort_steps()**: Kahn's algorithm produces final execution order

### Node Registry

```rust
#[derive(Debug, Clone, Copy)]
struct NodeIdx(usize);

#[derive(Debug, Default)]
struct NodeRegistry {
    push: HashMap<String, NodeIdx>,
    update: HashMap<String, NodeIdx>,
    create: HashMap<String, NodeIdx>,
    publish: HashMap<String, NodeIdx>,
}

impl ExecutionConstraint {
    /// Resolve to (from, to) indices, or None if endpoint missing.
    fn resolve(&self, registry: &NodeRegistry) -> Option<(usize, usize)> {
        match self {
            Self::PushOrder { parent, child } => {
                let from = registry.push.get(&parent.0)?;
                let to = registry.push.get(&child.0)?;
                Some((from.0, to.0))
            }
            // ... other variants
        }
    }
}
```

**Why `Option`?** Constraints may reference operations that don't exist in the plan (e.g., an already-synced bookmark has no `Push` node). Silent skipping is semantically correct here.

### Topological Sort (Kahn's Algorithm)

We use Kahn's algorithm to produce a valid execution order from the dependency DAG.

**Algorithm overview:**
1. Compute in-degree (incoming edge count) for each node
2. Initialize a ready queue with all nodes having in-degree=0
3. Loop: pop node from queue → emit to output → decrement in-degree of all successors → enqueue any successor that reaches in-degree=0
4. If output length < node count, a cycle exists (some nodes never became ready)

**Why Kahn's algorithm:**
- Naturally handles DAG constraints (dependencies satisfied before dependents)
- Cycle detection is free (unreachable nodes indicate cycles)
- Deterministic ordering via min-heap keyed by `(insertion_order, node_idx)` as tiebreaker when multiple nodes are ready simultaneously

```rust
fn topo_sort_steps(nodes: &[ExecutionNode], edges: &[Vec<usize>]) -> Result<Vec<ExecutionStep>>
```

### Execution Loop

The executor becomes a simple dispatch loop:

```rust
for step in &plan.execution_steps {
    let outcome = execute_step(step, workspace, platform, remote, progress).await;
    match outcome {
        StepOutcome::Success(pr) => { /* track PR */ }
        StepOutcome::FatalError(msg) => { result.fail(msg); return Ok(result); }
        StepOutcome::SoftError(msg) => { result.soft_fail(msg); }
    }
}
```

### Display Implementation

Each enum gains a `Display` impl:

```rust
impl Display for ExecutionStep {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Push(bm) => write!(f, "push {}", bm.name),
            Self::CreatePr(c) => write!(f, "create PR {} -> {} ({})", ...),
            // ...
        }
    }
}

impl Display for ExecutionConstraint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PushOrder { parent, child } => {
                write!(f, "Push({}) → Push({})", parent.0, child.0)
            }
            // ...
        }
    }
}
```

CLI display code collapses from ~80 lines to:

```rust
for step in &plan.execution_steps {
    println!("    -> {step}");
}
```

---

## Type Safety Analysis

### What Invalid States Are Now Unrepresentable

| Invalid State | How Prevented |
|---------------|---------------|
| `PushBeforeCreate { push: UpdateRef(...), ... }` | Compile error: expected `PushRef` |
| `CreateOrder { parent: PushRef(...), ... }` | Compile error: expected `CreateRef` |
| Anonymous `add_edge(3, 7)` without semantic meaning | Must use `ExecutionConstraint` enum |
| Missing match arm when adding new constraint type | Exhaustive match in `resolve()` |

### What Is NOT Prevented (Acceptable)

| Risk | Mitigation |
|------|------------|
| Typos in bookmark names | Runtime: constraint resolves to `None`, edge skipped |
| Cycles in constraint graph | Runtime: Kahn's algorithm detects, returns `Error::SchedulerCycle` |
| Forgetting a constraint | Code review, tests verify ordering |

---

## Changes Summary

| File | Lines Changed | Description |
|------|---------------|-------------|
| `src/submit/plan.rs` | +600 | `ExecutionStep`, `ExecutionConstraint`, typed refs, topo sort |
| `src/submit/execute.rs` | +200 | `StepOutcome`, step executors, simplified orchestrator |
| `src/submit/progress.rs` | +15 | `Phase::Executing` replaces 4 separate phases |
| `src/submit/mod.rs` | +5 | Export `ExecutionConstraint` |
| `src/cli/submit.rs` | -50 | Simplified plan preview and option application |
| `src/cli/sync.rs` | -20 | Simplified plan display |
| `src/cli/progress.rs` | -20 | Use `Display` impls |
| `tests/*.rs` | +40 | Adapt to `execution_steps` API, add constraint field |
| `tests/execution_step_tests.rs` | +500 | **NEW**: Integration tests for step ordering |
| `tests/e2e_tests.rs` | +120 | **NEW**: E2E tests for swap/mixed operations |
| `tests/common/temp_repo.rs` | +50 | `TempJjRepo` helpers: `rebase_before`, `move_bookmark`, etc. |
| `tests/common/fixtures.rs` | +15 | `make_pr_draft` helper |

**Total:** ~+1500 lines net, including ~150 lines of type definitions and ~700 lines of tests.

---

## Trade-offs and Alternatives

### Alternative: Keep Separate Collections + Sort Key

Add a `sort_key: usize` to each operation, sort before execution.

**Rejected**: Doesn't express inter-operation dependencies, only total ordering. The swap case requires conditional ordering that a simple key cannot express.

### Alternative: Multi-Phase with Explicit Swap Detection

Keep current structure, add a "swap detection" pre-pass that reorders operations.

**Rejected**: Fragile heuristics, doesn't generalize to complex multi-swap scenarios.

### Alternative: Typed Node Indices Instead of Typed Constraints

Use `PushIdx`, `CreateIdx` newtypes for indices rather than constraint-level typing.

**Rejected**: Prevents mixing index types but doesn't document *why* edges exist. The `ExecutionConstraint` enum is self-documenting and allows storing constraints for debugging.

### Alternative: Builder Pattern for Graph Construction

Use a builder that returns typed handles from `add_push()`, `add_create()`, etc.

**Rejected**: Introduces cross-builder bug risk (handles from builder A used with builder B). Declarative constraint collection is cleaner.

### Trade-off: Memory Overhead

`ExecutionStep` and `ExecutionConstraint` enums with cloned data. For typical stacks (2-10 PRs), this is negligible (~10KB). Acceptable.

### Trade-off: Complexity

The typed constraint system adds ~100 lines of type definitions. This is offset by:
- Compile-time rejection of invalid constraint pairings
- Self-documenting constraint vocabulary (enum variants)
- Debug logging of constraints before resolution

---

## Testing Strategy

### Unit Tests (`src/submit/plan.rs`)

| Test | Validates |
|------|-----------|
| `test_execution_steps_simple_push_order` | Pushes follow stack order |
| `test_execution_steps_push_before_create` | Push precedes PR creation |
| `test_execution_steps_create_order_follows_stack` | Creates follow stack order |
| `test_execution_steps_swap_order` | **Critical**: retarget before push in swap case |

### Integration Tests (`tests/execution_step_tests.rs`)

Tests using real jj repositories via `TempJjRepo` to validate ordering with actual workspace state:

| Test | Validates |
|------|-----------|
| `test_swap_scenario_retarget_before_push` | **Critical**: A→B to B→A swap ordering |
| `test_three_level_swap_middle_to_root` | 3-level stack reordering |
| `test_push_order_follows_stack_structure` | 4-deep stack push ordering |
| `test_create_order_respects_stack_for_comment_linking` | `CreatePr` follows stack for parent refs |
| `test_push_before_create_constraint` | `Push(X)` before `CreatePr(X)` |
| `test_push_before_retarget_constraint` | `Push(base)` before `UpdateBase(pr)` |
| `test_partial_existing_prs_mixed_operations` | Mixed Push/Update/Create ordering |
| `test_draft_pr_in_stack` | Draft PR handling |
| `test_constraints_skip_synced_bookmarks` | Graceful constraint skipping |
| `test_all_prs_exist_correct_bases` | Minimal operations scenario |
| `test_ten_level_stack_ordering` | 10-level stress test |
| `test_constraint_display_formatting` | `ExecutionConstraint::Display` impl |
| `test_scheduler_cycle_error_format` | `Error::SchedulerCycle` structure |

### E2E Tests (`tests/e2e_tests.rs`)

Real GitHub API tests validating execution ordering against live platform (require `JJ_RYU_E2E_TESTS=1`):

| Test | Validates |
|------|-----------|
| `test_stack_swap_reorder` | Swap works against real GitHub API |
| `test_mixed_operations_ordering` | Mixed ops with real API |
| `test_insert_middle_of_stack` | Insert commit + base update |

### Test Infrastructure

`TempJjRepo` helpers for stack manipulation:
- `rebase_before(rev, before)` — Swap commits via `jj rebase -r REV --before TARGET`
- `move_bookmark(name, to_rev)` — Move bookmark to different revision
- `change_id(bookmark)` — Get change ID for a bookmark

Fixture helpers:
- `make_pr_draft(number, head, base)` — Create draft PR fixture

---

## Migration & Compatibility

### API Changes (Library Crate)

| Before | After |
|--------|-------|
| `plan.prs_to_create` | `plan.execution_steps.iter().filter_map(...)` |
| `plan.bookmarks_needing_push` | `plan.count_pushes()` or filter |
| N/A | `plan.constraints` (new field for debugging) |

The old fields are **removed**, not deprecated. Library consumers must update.

### CLI Behavior

Unchanged. Users see the same output, just generated via `Display` rather than hardcoded strings.

### CLI Flag Semantics: `--draft` and `--publish`

`PublishPr` steps are added post-planning by `apply_plan_options()` rather than during constraint resolution. This is safe because:

1. `--publish` only affects PRs in `existing_prs` (from previous runs)
2. Publishing has no ordering dependencies with other operations
3. PRs created in the current run are not in `existing_prs`

**Flag precedence**: When both `--draft` and `--publish` are specified, `--publish` takes precedence and `--draft` is ignored. This ensures predictable behavior:

| Flags | New PRs | Existing Draft PRs |
|-------|---------|-------------------|
| (none) | normal | unchanged |
| `--draft` | draft | unchanged |
| `--publish` | normal | published |
| `--draft --publish` | normal | published |

### Progress Phases

```rust
// BEFORE
Phase::Pushing, Phase::CreatingPrs, Phase::UpdatingPrs, Phase::PublishingPrs

// AFTER  
Phase::Executing  // single phase for all operations
```

---

## Debug Logging

The constraint system adds structured debug logging:

```rust
tracing::debug!(constraint_count = constraints.len(), "Collected execution constraints");

// Per-constraint resolution (trace level)
tracing::trace!(%constraint, from, to, "Resolved constraint to edge");
tracing::trace!(%constraint, "Constraint skipped (endpoint not in plan)");
```

Enable with `RUST_LOG=jj_ryu::submit::plan=debug` for constraint counts, or `=trace` for per-constraint details.

---

## Security Considerations

None. This is a pure refactoring of internal scheduling logic. No new external inputs, no authentication changes, no new network calls.

---

## Questions & Decisions

1. **Parallelization**: Sequential execution only. Parallel API calls add complexity and rate-limit concerns; current perf is acceptable.

2. **Undo/Rollback**: No rollback on failure. PRs are idempotent; users can `ryu submit` again after fixing issues.

3. **Cycle Detection UX**: Dedicated `Error::SchedulerCycle` variant with:
   - Message explaining this is a bug, not user error
   - List of cycle node descriptions for debugging
   - `tracing::error!` log with cycle details
   
   ```rust
   Error::SchedulerCycle {
       message: String,
       cycle_nodes: Vec<String>,  // e.g., ["push feature-a", "update feature-b"]
   }
   ```

4. **Store constraints in plan?** **Yes** — enables debugging and potential dry-run display of dependencies.

5. **More granular `NodeIdx` types?** **No** — `PushIdx`, `CreateIdx` etc. adds boilerplate without significant benefit since resolution already type-checks via registry maps.

---

## Conclusion

This RFC proposes replacing the implicit, collection-based execution model with an explicit, dependency-aware `ExecutionStep` model, enhanced with a typed `ExecutionConstraint` system. The key benefits are:

1. **Correctness**: Stack swap scenarios work correctly
2. **Type Safety**: Invalid constraint pairings rejected at compile time
3. **Simplicity**: Display and execution logic reduced by ~150 lines
4. **Testability**: Scheduler can be unit-tested in isolation
5. **Debuggability**: Constraints stored in plan, available for logging
6. **Maintainability**: Single source of truth for operation ordering; self-documenting constraint vocabulary

The implementation is complete and all existing tests pass after adaptation.

**Test coverage added (2026-01-03):**
- 13 integration tests in `tests/execution_step_tests.rs` using real jj workspaces
- 3 E2E tests in `tests/e2e_tests.rs` validating against real GitHub API
- `TempJjRepo` helpers for stack reordering (`rebase_before`, `move_bookmark`)
- `make_pr_draft` fixture helper
