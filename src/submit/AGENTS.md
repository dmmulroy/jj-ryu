# submit/

**Generated:** 2026-01-07

## OVERVIEW

Three-phase submission engine: analysis → plan → execute. Handles stack ordering, PR lifecycle, dependency-aware scheduling.

## FILES

| File | Purpose |
|------|---------|
| `analysis.rs` | Build `ChangeGraph`, identify bookmarks to submit |
| `plan.rs` | Create `SubmissionPlan` with typed constraints + topo sort |
| `execute.rs` | Execute plan: push, create PRs, update bases, stack comments |
| `progress.rs` | `ProgressCallback` trait for CLI feedback |
| `mod.rs` | Re-exports |

## EXECUTION STEP MODEL

**Why?** Stack swap scenarios require interleaved push/retarget. See `docs/rfcs/rfc-execution-step-model.md`.

### Core Types

```rust
enum ExecutionStep { Push, UpdateBase, CreatePr, PublishPr }
enum ExecutionConstraint {
    PushOrder { parent: PushRef, child: PushRef },       // Stack order
    PushBeforeRetarget { base: PushRef, pr: UpdateRef }, // Can't retarget to non-existent branch
    RetargetBeforePush { pr: UpdateRef, old_base: PushRef }, // SWAP: move off before pushing
    PushBeforeCreate { push: PushRef, create: CreateRef },
    CreateOrder { parent: CreateRef, child: CreateRef }, // Stack comment linking
}
```

### Typed Refs

`PushRef`, `UpdateRef`, `CreateRef` - prevent mixing constraint endpoints at compile time:
```rust
// COMPILE ERROR: expected PushRef, found UpdateRef
ExecutionConstraint::PushOrder { parent: UpdateRef("a".into()), ... }
```

### Scheduling Pipeline

```
collect_constraints() → build_execution_nodes() → resolve_constraints() → topo_sort_steps()
         ↓                        ↓                        ↓                    ↓
    Typed constraints       Nodes + Registry         Edges (adjacency)    Sorted steps
```

### Swap Detection

When `current_base` position > `bookmark` position in stack → `RetargetBeforePush` constraint emitted:
```rust
if current_pos > bookmark_pos {
    constraints.push(ExecutionConstraint::RetargetBeforePush { ... });
}
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Add new step type | `ExecutionStep` enum in `plan.rs`, add to `execute_step()` |
| Add constraint type | `ExecutionConstraint` enum, add typed ref if needed, impl `resolve()` |
| Change PR creation | `execute_create_pr()` in `execute.rs` |
| Change stack comments | `format_stack_comment()`, `COMMENT_DATA_PREFIX` |
| Debug scheduling | `RUST_LOG=jj_ryu::submit::plan=trace` |

## ANTI-PATTERNS

- Don't add edges without typed constraint - use `ExecutionConstraint` enum
- Don't modify execution order outside `build_execution_steps()` 
- Don't skip `resolve()` returning `None` - expected for already-synced bookmarks

## TESTING

Integration tests: `tests/execution_step_tests.rs`.

Key tests:
- `test_execution_steps_swap_order` - Validates swap constraint
- `test_swap_scenario_retarget_before_push` - Full integration with `TempJjRepo`
- `test_ten_level_stack_*` - Validates constraint scalability
