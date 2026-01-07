# jj-ryu

**Generated:** 2026-01-07 | **Commit:** 633a52f

## OVERVIEW

Stacked PRs CLI for Jujutsu (jj). Rust binary `ryu` + library `jj_ryu`. GitHub & GitLab support via platform abstraction.

## STRUCTURE

```
src/
├── main.rs         # CLI entry (clap), owns `mod cli`
├── lib.rs          # Library crate, public API
├── cli/            # CLI-only (not exported from lib)
├── submit/         # 3-phase engine: analysis → plan → execute (see AGENTS.md)
├── platform/       # PlatformService trait + GitHub/GitLab impls
├── graph/          # ChangeGraph builder from jj workspace
├── repo/           # JjWorkspace wrapper, revset resolution, trunk() alias
├── auth/           # Token retrieval (gh/glab CLI integration)
├── tracking/       # PR cache, bookmark-to-PR mapping persistence
├── types.rs        # Core domain types (Bookmark, PullRequest, etc.)
└── error.rs        # thiserror Error enum
tests/
├── unit_tests.rs         # Pure logic, no I/O
├── integration_tests.rs  # Real jj workspace, mocked platform
├── execution_step_tests.rs  # Execution ordering tests (topological sort)
├── e2e_tests.rs          # Real GitHub API (#[ignore])
└── common/               # TempJjRepo, MockPlatformService, fixtures
docs/rfcs/                # Design docs (rfc-execution-step-model.md)
npm/                      # Cross-platform binary distribution
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add CLI flag | `src/main.rs` | clap derives, then wire to `src/cli/` |
| New platform | `src/platform/` | Impl `PlatformService` trait |
| PR creation logic | `src/submit/execute.rs` | Stack comments, base updates |
| Graph traversal | `src/graph/builder.rs` | jj revsets, adjacency building |
| Auth flow | `src/auth/{github,gitlab}.rs` | Shells to gh/glab CLI |
| Execution ordering | `src/submit/plan.rs` | Typed constraints, topo sort |
| Swap scenarios | `tests/execution_step_tests.rs` | `test_swap_scenario_*` |
| Integration test | `tests/integration_tests.rs` | Uses `TempJjRepo` + `MockPlatformService` |
| E2E test | `tests/e2e_tests.rs` | Real API, needs `JJ_RYU_E2E_TESTS=1` |

## ARCHITECTURE

**Dual crate pattern**: Binary `ryu` (main.rs + cli/) uses library `jj_ryu` (lib.rs). CLI concerns stay out of library.

**Three-phase submission** (see `src/submit/AGENTS.md`):
1. `analysis.rs` - Build `ChangeGraph`, find bookmarks to submit
2. `plan.rs` - Determine `SubmissionPlan` with typed constraints + topo sort
3. `execute.rs` - Push branches, create/update PRs, manage stack comments

**Execution Step Model** (RFC: `docs/rfcs/rfc-execution-step-model.md`):
- `ExecutionStep` enum: `Push`, `UpdateBase`, `CreatePr`, `PublishPr`
- `ExecutionConstraint` enum: typed dependencies (compile-time invalid pairing rejection)
- Kahn's algorithm for topological sort
- Handles stack swap scenarios correctly

**Platform abstraction**: `PlatformService` trait → `GitHubService`, `GitLabService`. Factory in `platform/factory.rs`.

## CONVENTIONS

- **Error handling**: `thiserror` for `Error` enum, `anyhow` only at main() boundary
- **Async**: tokio runtime, `#[async_trait]` for platform trait
- **TLS**: rustls everywhere (no native-tls) for cross-platform builds
- **Edition 2024 / MSRV 1.85** - bleeding edge
- **No inline tests**: All tests in `tests/` directory, no `mod tests { }` blocks

## ANTI-PATTERNS

- `unsafe_code = "deny"` - no unsafe allowed
- No `.unwrap()` in production code (limited exceptions for post-validation)
- Don't add features to lib.rs unless interface-agnostic
- Don't use mockall - hand-rolled `MockPlatformService` for method return type compatibility

## LINTS

Clippy: `all`, `pedantic`, `nursery` at warn. Allowed exceptions:
- `module_name_repetitions`
- `missing_errors_doc`, `missing_panics_doc`
- `must_use_candidate`

Local suppression: `#[allow(clippy::too_many_lines)]` acceptable for complex functions.

## COMMANDS

```bash
cargo build              # Debug build
cargo build --release    # Release (RUSTFLAGS="-C strip=symbols" in CI)
cargo test --lib         # Unit tests
cargo test --test '*'    # Integration tests
cargo test --doc         # Doc tests
cargo clippy -- -D warnings  # Lint (warnings = errors in CI)
```

E2E tests (main branch CI only):
```bash
JJ_RYU_E2E_TESTS=1 cargo test --test e2e_tests -- --ignored
```

## TESTING

- **Unit**: `tests/unit_tests.rs` - synthetic `ChangeGraph`, no filesystem
- **Integration**: `tests/integration_tests.rs` - real jj workspace via `TempJjRepo`, mocked platform via `MockPlatformService`
- **Execution ordering**: `tests/execution_step_tests.rs` - constraint resolution, swap scenarios
- **E2E**: `tests/e2e_tests.rs` - real GitHub API, `#[ignore]`, requires tokens

**Key test helpers**:
- `TempJjRepo::new()` - Creates temp dir + `jj git init`
- `TempJjRepo::build_stack(&[...])` - Creates commit chain with bookmarks
- `TempJjRepo::rebase_before(rev, before)` - Swap commits for reorder tests
- `MockPlatformService` - Response injection, call tracking, error injection

**Why no mockall**: Method return type compatibility issues. Hand-rolled `MockPlatformService` uses `Mutex<HashMap>` for response injection and call tracking.

## RELEASE

Manual dispatch only. Builds 8 platform targets:
- darwin: arm64, x64
- linux: x64, arm64, x64-musl, arm64-musl
- windows: x64, arm64

Publishes to: crates.io, npm (platform packages), GitHub releases, Homebrew tap.
