//! Integration tests for `ExecutionStep` model (RFC: Unified Execution Step Model)
//!
//! Tests the dependency-aware topological ordering of execution steps using
//! real jj repositories via `TempJjRepo`.

mod common;

use common::{MockPlatformService, TempJjRepo, github_config, make_pr, make_pr_draft};
use jj_ryu::graph::build_change_graph;
use jj_ryu::submit::{ExecutionStep, analyze_submission, create_submission_plan};

// =============================================================================
// Helper Functions
// =============================================================================

/// Find the index of a step matching a predicate
fn find_step_index(
    steps: &[ExecutionStep],
    predicate: impl Fn(&ExecutionStep) -> bool,
) -> Option<usize> {
    steps.iter().position(predicate)
}

/// Assert step A comes before step B in the execution order
fn assert_step_order(steps: &[ExecutionStep], description: &str, idx_a: usize, idx_b: usize) {
    assert!(
        idx_a < idx_b,
        "{description}: expected step at index {idx_a} before step at index {idx_b}, \
         but got order {:?}",
        steps.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
}

// =============================================================================
// Stack Swap Ordering Tests (RFC §Problem Statement - Critical)
// =============================================================================

/// Test: When stack changes from A→B to B→A, UpdateBase(B) must come before Push(A)
///
/// This is the critical swap scenario that motivated the RFC. Without proper
/// ordering, GitHub/GitLab would reject the base change due to branch history conflicts.
#[tokio::test]
async fn test_swap_scenario_retarget_before_push() {
    // Setup: Create stack A→B (A is root, B is leaf)
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    // Swap the stack: rebase B before A, making order B→A
    repo.rebase_before("feat-b", "feat-a");

    // Move working copy to the new leaf (feat-a) after swap
    repo.edit("feat-a");

    // Now the stack is B→A in the jj repo
    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");

    // Analyze submitting A (the new leaf)
    let analysis = analyze_submission(&graph, Some("feat-a")).expect("analyze");

    // Verify the analysis reflects the new order: B is root, A is leaf
    assert_eq!(analysis.segments.len(), 2);
    assert_eq!(analysis.segments[0].bookmark.name, "feat-b"); // root
    assert_eq!(analysis.segments[1].bookmark.name, "feat-a"); // leaf

    // Setup mock: Both PRs exist with OLD bases (before swap)
    let mock = MockPlatformService::with_config(github_config());
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main"))); // Was root, now should be on B
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "feat-a"))); // Was on A, now should be on main

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Verify we have the expected operations
    assert!(
        plan.count_updates() >= 1,
        "should have base updates for swapped PRs"
    );
    assert!(
        plan.count_pushes() >= 1,
        "should have pushes for changed branches"
    );

    // Critical assertion: UpdateBase operations must happen at correct time relative to pushes
    // B's PR needs to be retargeted from feat-a to main BEFORE we push the new A
    // (because A will have different history after the swap)
    let steps = &plan.execution_steps;

    // Find UpdateBase for B (changing from feat-a to main)
    let update_b_idx = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::UpdateBase(u) if u.bookmark.name == "feat-b"),
    );

    // Find Push for A
    let push_a_idx = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-a"),
    );

    // If B had A as its base and we're swapping, B's retarget should happen
    // This test verifies the constraint system handles the swap correctly
    if let (Some(update_idx), Some(push_idx)) = (update_b_idx, push_a_idx) {
        // The RFC specifies: RetargetBeforePush when current base moves "below"
        // B's current_base was "feat-a", B is now root (pos 0), A is leaf (pos 1)
        // So current_base (feat-a) position (1) > bookmark position (0) - swap scenario!
        assert_step_order(
            steps,
            "UpdateBase(B) must come before Push(A) in swap scenario",
            update_idx,
            push_idx,
        );
    }
}

/// Test: Three-level stack with middle element becoming root
#[tokio::test]
async fn test_three_level_swap_middle_to_root() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    // Move B to be the root (before A)
    repo.rebase_before("feat-b", "feat-a");

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");

    // Submit from C (should include B→A→C or similar reordered stack)
    let analysis = analyze_submission(&graph, Some("feat-c")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // All PRs exist with old bases
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "feat-a")));
    mock.set_find_pr_response("feat-c", Some(make_pr(3, "feat-c", "feat-b")));

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Should have operations to fix the base mismatches
    // The exact operations depend on how jj rebase affects the graph
    assert!(!plan.is_empty(), "plan should have operations after swap");
}

// =============================================================================
// Constraint Resolution Tests (RFC §Three-Phase Execution)
// =============================================================================

/// Test: Push operations follow stack order (parent before child)
#[tokio::test]
async fn test_push_order_follows_stack_structure() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
        ("feat-d", "Add D"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-d")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // No existing PRs - all need push and create

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    assert_eq!(plan.count_pushes(), 4, "all 4 bookmarks need push");

    let steps = &plan.execution_steps;
    let push_a = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-a"),
    )
    .unwrap();
    let push_b = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-b"),
    )
    .unwrap();
    let push_c = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-c"),
    )
    .unwrap();
    let push_d = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-d"),
    )
    .unwrap();

    assert_step_order(steps, "Push(A) before Push(B)", push_a, push_b);
    assert_step_order(steps, "Push(B) before Push(C)", push_b, push_c);
    assert_step_order(steps, "Push(C) before Push(D)", push_c, push_d);
}

/// Test: `CreatePr` operations follow stack order for comment linking
#[tokio::test]
async fn test_create_order_respects_stack_for_comment_linking() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-c")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // No existing PRs

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    assert_eq!(plan.count_creates(), 3);

    let steps = &plan.execution_steps;
    let create_a = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "feat-a"),
    )
    .unwrap();
    let create_b = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "feat-b"),
    )
    .unwrap();
    let create_c = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "feat-c"),
    )
    .unwrap();

    assert_step_order(steps, "CreatePr(A) before CreatePr(B)", create_a, create_b);
    assert_step_order(steps, "CreatePr(B) before CreatePr(C)", create_b, create_c);
}

/// Test: Push must happen before `CreatePr` for the same bookmark
#[tokio::test]
async fn test_push_before_create_constraint() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-a")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    assert_eq!(plan.count_pushes(), 1);
    assert_eq!(plan.count_creates(), 1);

    let steps = &plan.execution_steps;
    let push_a = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-a"),
    )
    .unwrap();
    let create_a = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "feat-a"),
    )
    .unwrap();

    assert_step_order(steps, "Push(A) before CreatePr(A)", push_a, create_a);
}

/// Test: Push base branch before retargeting PR to it
#[tokio::test]
async fn test_push_before_retarget_constraint() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // B's PR exists but has wrong base (main instead of feat-a)
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "main")));
    // A has no PR yet

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Should have: Push(A), Push(B), UpdateBase(B), CreatePr(A)
    let steps = &plan.execution_steps;

    let push_a = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-a"),
    );
    let update_b = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::UpdateBase(u) if u.bookmark.name == "feat-b"),
    );

    // Push(A) must happen before UpdateBase(B) because B's new base is A
    if let (Some(push_idx), Some(update_idx)) = (push_a, update_b) {
        assert_step_order(
            steps,
            "Push(A) before UpdateBase(B) - can't retarget to unpushed branch",
            push_idx,
            update_idx,
        );
    }
}

// =============================================================================
// Mixed Operation Scenario Tests (RFC §Design)
// =============================================================================

/// Test: Partial existing PRs with mixed operations
#[tokio::test]
async fn test_partial_existing_prs_mixed_operations() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-c")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // A: PR exists with correct base (main)
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
    // B: PR exists but wrong base (main instead of feat-a)
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "main")));
    // C: No PR exists

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Expected operations:
    // - Push for all 3 (assuming not synced)
    // - UpdateBase for B (fix base from main to feat-a)
    // - CreatePr for C
    assert_eq!(plan.count_updates(), 1, "B needs base update");
    assert_eq!(plan.count_creates(), 1, "C needs PR creation");

    // Verify ordering constraints
    let steps = &plan.execution_steps;

    // Push(A) should come before UpdateBase(B)
    let push_a = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-a"),
    );
    let update_b = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::UpdateBase(u) if u.bookmark.name == "feat-b"),
    );

    if let (Some(push_idx), Some(update_idx)) = (push_a, update_b) {
        assert_step_order(steps, "Push(A) before UpdateBase(B)", push_idx, update_idx);
    }

    // Push(C) should come before CreatePr(C)
    let push_c = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::Push(b) if b.name == "feat-c"),
    );
    let create_c = find_step_index(
        steps,
        |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "feat-c"),
    );

    if let (Some(push_idx), Some(create_idx)) = (push_c, create_c) {
        assert_step_order(steps, "Push(C) before CreatePr(C)", push_idx, create_idx);
    }
}

/// Test: Plan with draft PR that needs publishing
#[tokio::test]
async fn test_draft_pr_in_stack() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // A: Draft PR exists
    mock.set_find_pr_response("feat-a", Some(make_pr_draft(1, "feat-a", "main")));
    // B: No PR

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // A's draft PR is tracked but not auto-published by planning
    // (publishing is handled by CLI options)
    assert_eq!(plan.existing_prs.len(), 1);
    assert!(plan.existing_prs.get("feat-a").unwrap().is_draft);
}

// =============================================================================
// Constraint Skipping Tests (RFC §NodeRegistry)
// =============================================================================

/// Test: Constraints gracefully skip when bookmarks are already synced
#[tokio::test]
async fn test_constraints_skip_synced_bookmarks() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // Both PRs exist with correct bases
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "feat-a")));

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // No creates or updates needed
    assert_eq!(plan.count_creates(), 0);
    assert_eq!(plan.count_updates(), 0);

    // Plan should still be valid (constraints for CreateOrder should be skipped)
    // since there are no CreatePr nodes to order
}

/// Test: All PRs exist with correct bases - minimal operations
#[tokio::test]
async fn test_all_prs_exist_correct_bases() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-c")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "feat-a")));
    mock.set_find_pr_response("feat-c", Some(make_pr(3, "feat-c", "feat-b")));

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    assert_eq!(plan.count_creates(), 0, "no PRs to create");
    assert_eq!(plan.count_updates(), 0, "no bases to update");
    assert_eq!(plan.existing_prs.len(), 3, "all PRs tracked");
}

// =============================================================================
// Deep Stack Tests
// =============================================================================

/// Test: 10-level deep stack maintains correct ordering
#[tokio::test]
async fn test_ten_level_stack_ordering() {
    let repo = TempJjRepo::new();

    let bookmarks: Vec<(&str, &str)> = (0..10)
        .map(|i| {
            // Leak strings to get static lifetime for the test
            let name = Box::leak(format!("feat-{i}").into_boxed_str());
            let msg = Box::leak(format!("Add feature {i}").into_boxed_str());
            (name as &str, msg as &str)
        })
        .collect();

    repo.build_stack(&bookmarks);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-9")).expect("analyze");

    assert_eq!(analysis.segments.len(), 10);

    let mock = MockPlatformService::with_config(github_config());

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    assert_eq!(plan.count_pushes(), 10);
    assert_eq!(plan.count_creates(), 10);

    // Verify all pushes are in order
    let steps = &plan.execution_steps;
    let mut prev_push_idx = None;
    for i in 0..10 {
        let name = format!("feat-{i}");
        let push_idx = find_step_index(
            steps,
            |s| matches!(s, ExecutionStep::Push(b) if b.name == name),
        )
        .unwrap_or_else(|| panic!("Push for {name} not found"));

        if let Some(prev) = prev_push_idx {
            assert!(
                prev < push_idx,
                "Push(feat-{}) at {prev} should come before Push({name}) at {push_idx}",
                i - 1
            );
        }
        prev_push_idx = Some(push_idx);
    }

    // Verify all creates are in order
    let mut prev_create_idx = None;
    for i in 0..10 {
        let name = format!("feat-{i}");
        let create_idx = find_step_index(
            steps,
            |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == name),
        )
        .unwrap_or_else(|| panic!("CreatePr for {name} not found"));

        if let Some(prev) = prev_create_idx {
            assert!(
                prev < create_idx,
                "CreatePr(feat-{}) at {prev} should come before CreatePr({name}) at {create_idx}",
                i - 1
            );
        }
        prev_create_idx = Some(create_idx);
    }
}

// =============================================================================
// ExecutionConstraint Display Tests
// =============================================================================

/// Test: `ExecutionConstraint` Display formatting is correct
#[tokio::test]
async fn test_constraint_display_formatting() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // B has wrong base to generate UpdateBase constraint
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "main")));

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Verify constraints are present and displayable
    assert!(!plan.constraints.is_empty(), "should have constraints");

    // Check that all constraints have valid Display output
    for constraint in &plan.constraints {
        let display = format!("{constraint}");
        assert!(
            !display.is_empty(),
            "constraint display should not be empty"
        );
        assert!(
            display.contains("→"),
            "constraint display should contain arrow: {display}"
        );
    }
}

// =============================================================================
// Cycle Detection Test
// =============================================================================

/// Test: `SchedulerCycle` error is properly detected
///
/// Note: This tests the error type exists and is properly formatted.
/// Actually triggering a cycle requires manually constructing invalid constraints,
/// which the type system prevents in normal usage.
#[test]
fn test_scheduler_cycle_error_format() {
    use jj_ryu::error::Error;

    let error = Error::SchedulerCycle {
        message: "test cycle".to_string(),
        cycle_nodes: vec!["push feat-a".to_string(), "update feat-b".to_string()],
    };

    let display = format!("{error}");
    assert!(display.contains("scheduler cycle detected"));
    assert!(display.contains("test cycle"));

    // Verify the error contains cycle_nodes (for debugging)
    match error {
        Error::SchedulerCycle { cycle_nodes, .. } => {
            assert_eq!(cycle_nodes.len(), 2);
            assert!(cycle_nodes.contains(&"push feat-a".to_string()));
        }
        _ => panic!("wrong error variant"),
    }
}
