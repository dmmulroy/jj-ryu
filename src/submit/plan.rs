//! Phase 2: Submission planning
//!
//! Determines what operations need to be performed to submit a stack.

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::submit::SubmissionAnalysis;
use crate::submit::analysis::{generate_pr_title, get_base_branch};
use crate::types::{Bookmark, NarrowedBookmarkSegment, PullRequest};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Information about a PR that needs to be created
#[derive(Debug, Clone)]
pub struct PrToCreate {
    /// Bookmark for this PR
    pub bookmark: Bookmark,
    /// Base branch (previous bookmark or default branch)
    pub base_branch: String,
    /// Generated PR title
    pub title: String,
    /// Whether to create as draft
    pub draft: bool,
}

/// Information about a PR that needs its base updated
#[derive(Debug, Clone)]
pub struct PrBaseUpdate {
    /// Bookmark for this PR
    pub bookmark: Bookmark,
    /// Current base branch
    pub current_base: String,
    /// Expected base branch
    pub expected_base: String,
    /// Existing PR
    pub pr: PullRequest,
}

/// Ordered execution step for a submission plan
#[derive(Debug, Clone)]
pub enum ExecutionStep {
    /// Push bookmark to remote
    Push(Bookmark),
    /// Update PR base branch
    UpdateBase(PrBaseUpdate),
    /// Create a new PR
    CreatePr(PrToCreate),
    /// Publish a draft PR
    PublishPr(PullRequest),
}

impl ExecutionStep {
    /// Get the bookmark name for this step
    pub fn bookmark_name(&self) -> &str {
        match self {
            Self::Push(bm) => &bm.name,
            Self::UpdateBase(update) => &update.bookmark.name,
            Self::CreatePr(create) => &create.bookmark.name,
            Self::PublishPr(pr) => &pr.head_ref,
        }
    }
}

impl std::fmt::Display for ExecutionStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Push(bm) => write!(f, "push {}", bm.name),
            Self::UpdateBase(update) => write!(
                f,
                "update {} (PR #{}) {} → {}",
                update.bookmark.name, update.pr.number, update.current_base, update.expected_base
            ),
            Self::CreatePr(create) => {
                write!(f, "create PR {} → {} ({})", create.bookmark.name, create.base_branch, create.title)?;
                if create.draft {
                    write!(f, " [draft]")?;
                }
                Ok(())
            }
            Self::PublishPr(pr) => write!(f, "publish PR #{} ({})", pr.number, pr.head_ref),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Typed constraint system for dependency-aware scheduling
// ═══════════════════════════════════════════════════════════════════════════

/// Typed reference to a Push operation by bookmark name.
/// Distinct from [`UpdateRef`]/[`CreateRef`] to prevent mixing constraint endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PushRef(pub String);

/// Typed reference to an `UpdateBase` operation by bookmark name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UpdateRef(pub String);

/// Typed reference to a `CreatePr` operation by bookmark name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreateRef(pub String);

/// Dependency constraint between execution operations.
///
/// Each variant encodes a semantic relationship between operations.
/// Invalid pairings (e.g., `CreatePr` → `Push`) are unrepresentable at the type level.
///
/// Constraints may reference operations that don't exist in the current plan
/// (e.g., a bookmark that's already synced has no `Push` node). Resolution
/// returns `None` for such constraints, which is expected behavior.
#[derive(Debug, Clone)]
pub enum ExecutionConstraint {
    /// Push parent branch before child branch.
    /// Ensures commits are pushed in stack order (ancestors before descendants).
    PushOrder {
        /// Parent bookmark (pushed first)
        parent: PushRef,
        /// Child bookmark (pushed second)
        child: PushRef,
    },

    /// Push new base branch before retargeting PR to it.
    /// Can't retarget a PR to a branch that doesn't exist on remote yet.
    PushBeforeRetarget {
        /// Base branch to push
        base: PushRef,
        /// PR to retarget
        pr: UpdateRef,
    },

    /// Retarget PR before pushing its old base (swap scenario).
    /// When stack order changes and a PR's current base moves "below" it,
    /// must retarget first to avoid platform rejection.
    RetargetBeforePush {
        /// PR to retarget first
        pr: UpdateRef,
        /// Old base to push after
        old_base: PushRef,
    },

    /// Push branch before creating PR for it.
    /// Branch must exist on remote before PR creation.
    PushBeforeCreate {
        /// Branch to push
        push: PushRef,
        /// PR to create
        create: CreateRef,
    },

    /// Create parent PR before child PR.
    /// Parent PR must exist so stack comments can reference its number/URL.
    CreateOrder {
        /// Parent PR (created first)
        parent: CreateRef,
        /// Child PR (created second)
        child: CreateRef,
    },
}

impl std::fmt::Display for ExecutionConstraint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PushOrder { parent, child } => {
                write!(f, "Push({}) → Push({})", parent.0, child.0)
            }
            Self::PushBeforeRetarget { base, pr } => {
                write!(f, "Push({}) → UpdateBase({})", base.0, pr.0)
            }
            Self::RetargetBeforePush { pr, old_base } => {
                write!(f, "UpdateBase({}) → Push({})", pr.0, old_base.0)
            }
            Self::PushBeforeCreate { push, create } => {
                write!(f, "Push({}) → CreatePr({})", push.0, create.0)
            }
            Self::CreateOrder { parent, child } => {
                write!(f, "CreatePr({}) → CreatePr({})", parent.0, child.0)
            }
        }
    }
}

/// Opaque node index, only obtainable via [`NodeRegistry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NodeIdx(usize);

/// Registry mapping typed refs to node indices.
/// Built during node creation, consumed during constraint resolution.
#[derive(Debug, Default)]
struct NodeRegistry {
    push: HashMap<String, NodeIdx>,
    update: HashMap<String, NodeIdx>,
    create: HashMap<String, NodeIdx>,
    publish: HashMap<String, NodeIdx>,
}

impl NodeRegistry {
    fn register_push(&mut self, name: &str, idx: usize) {
        self.push.insert(name.to_string(), NodeIdx(idx));
    }

    fn register_update(&mut self, name: &str, idx: usize) {
        self.update.insert(name.to_string(), NodeIdx(idx));
    }

    fn register_create(&mut self, name: &str, idx: usize) {
        self.create.insert(name.to_string(), NodeIdx(idx));
    }

    fn register_publish(&mut self, name: &str, idx: usize) {
        self.publish.insert(name.to_string(), NodeIdx(idx));
    }

    fn len(&self) -> usize {
        self.push.len() + self.update.len() + self.create.len() + self.publish.len()
    }
}

impl ExecutionConstraint {
    /// Resolve constraint to concrete `(from, to)` indices.
    ///
    /// Returns `None` if either endpoint doesn't exist in the registry.
    /// This is expected when an operation isn't needed (e.g., already-synced bookmark).
    fn resolve(&self, registry: &NodeRegistry) -> Option<(usize, usize)> {
        match self {
            Self::PushOrder { parent, child } => {
                let from = registry.push.get(&parent.0)?;
                let to = registry.push.get(&child.0)?;
                Some((from.0, to.0))
            }
            Self::PushBeforeRetarget { base, pr } => {
                let from = registry.push.get(&base.0)?;
                let to = registry.update.get(&pr.0)?;
                Some((from.0, to.0))
            }
            Self::RetargetBeforePush { pr, old_base } => {
                let from = registry.update.get(&pr.0)?;
                let to = registry.push.get(&old_base.0)?;
                Some((from.0, to.0))
            }
            Self::PushBeforeCreate { push, create } => {
                let from = registry.push.get(&push.0)?;
                let to = registry.create.get(&create.0)?;
                Some((from.0, to.0))
            }
            Self::CreateOrder { parent, child } => {
                let from = registry.create.get(&parent.0)?;
                let to = registry.create.get(&child.0)?;
                Some((from.0, to.0))
            }
        }
    }
}

/// Internal node for dependency-aware scheduling
#[derive(Debug, Clone)]
struct ExecutionNode {
    step: ExecutionStep,
    order: usize,
}

/// Submission plan
#[derive(Debug, Clone)]
pub struct SubmissionPlan {
    /// Segments to submit (used for stack comment generation)
    pub segments: Vec<NarrowedBookmarkSegment>,
    /// Dependency constraints between operations (for debugging/dry-run display)
    pub constraints: Vec<ExecutionConstraint>,
    /// Ordered execution steps
    pub execution_steps: Vec<ExecutionStep>,
    /// Existing PRs by bookmark name
    pub existing_prs: HashMap<String, PullRequest>,
    /// Remote name to push to
    pub remote: String,
    /// Default branch name (main/master)
    pub default_branch: String,
}

impl SubmissionPlan {
    /// Check if there's nothing to do
    pub fn is_empty(&self) -> bool {
        self.execution_steps.is_empty()
    }

    /// Count push steps
    pub fn count_pushes(&self) -> usize {
        self.execution_steps
            .iter()
            .filter(|s| matches!(s, ExecutionStep::Push(_)))
            .count()
    }

    /// Count create PR steps
    pub fn count_creates(&self) -> usize {
        self.execution_steps
            .iter()
            .filter(|s| matches!(s, ExecutionStep::CreatePr(_)))
            .count()
    }

    /// Count update base steps
    pub fn count_updates(&self) -> usize {
        self.execution_steps
            .iter()
            .filter(|s| matches!(s, ExecutionStep::UpdateBase(_)))
            .count()
    }

    /// Count publish steps
    pub fn count_publishes(&self) -> usize {
        self.execution_steps
            .iter()
            .filter(|s| matches!(s, ExecutionStep::PublishPr(_)))
            .count()
    }
}

/// Create a submission plan
///
/// This determines what operations need to be performed:
/// - Which bookmarks need pushing
/// - Which PRs need to be created
/// - Which PR bases need updating
pub async fn create_submission_plan(
    analysis: &SubmissionAnalysis,
    platform: &dyn PlatformService,
    remote: &str,
    default_branch: &str,
) -> Result<SubmissionPlan> {
    let segments = &analysis.segments;
    let bookmarks: Vec<&Bookmark> = segments.iter().map(|s| &s.bookmark).collect();

    // Check for existing PRs
    let mut existing_prs = HashMap::new();
    for bookmark in &bookmarks {
        if let Some(pr) = platform.find_existing_pr(&bookmark.name).await? {
            existing_prs.insert(bookmark.name.clone(), pr);
        }
    }

    // Collect raw operations (unordered)
    let mut bookmarks_needing_push = Vec::new();
    let mut prs_to_create = Vec::new();
    let mut prs_to_update_base = Vec::new();

    for bookmark in &bookmarks {
        // Check if needs push
        if !bookmark.has_remote || !bookmark.is_synced {
            bookmarks_needing_push.push((*bookmark).clone());
        }

        // Check if needs PR creation
        if let Some(pr) = existing_prs.get(&bookmark.name) {
            // PR exists - check if base needs updating
            let expected_base = get_base_branch(&bookmark.name, segments, default_branch)?;

            if pr.base_ref != expected_base {
                prs_to_update_base.push(PrBaseUpdate {
                    bookmark: (*bookmark).clone(),
                    current_base: pr.base_ref.clone(),
                    expected_base,
                    pr: pr.clone(),
                });
            }
        } else {
            // PR doesn't exist - needs creation
            let base_branch = get_base_branch(&bookmark.name, segments, default_branch)?;
            let title = generate_pr_title(&bookmark.name, segments)?;

            prs_to_create.push(PrToCreate {
                bookmark: (*bookmark).clone(),
                base_branch,
                title,
                draft: false,
            });
        }
    }

    // Build ordered execution steps
    let (constraints, execution_steps) = build_execution_steps(
        segments,
        &bookmarks_needing_push,
        &prs_to_update_base,
        &prs_to_create,
        &[], // prs_to_publish populated by CLI layer via apply_plan_options
    )?;

    Ok(SubmissionPlan {
        segments: segments.clone(),
        constraints,
        execution_steps,
        existing_prs,
        remote: remote.to_string(),
        default_branch: default_branch.to_string(),
    })
}

/// Build dependency-ordered execution steps.
///
/// Returns both the constraints (for debugging/display) and the sorted execution steps.
fn build_execution_steps(
    segments: &[NarrowedBookmarkSegment],
    bookmarks_needing_push: &[Bookmark],
    prs_to_update_base: &[PrBaseUpdate],
    prs_to_create: &[PrToCreate],
    prs_to_publish: &[PullRequest],
) -> Result<(Vec<ExecutionConstraint>, Vec<ExecutionStep>)> {
    let stack_index = build_stack_index(segments);

    // Phase 1: Collect semantic constraints (declarative, no indices)
    let constraints = collect_constraints(segments, prs_to_update_base, prs_to_create, &stack_index);

    tracing::debug!(
        constraint_count = constraints.len(),
        "Collected execution constraints"
    );

    // Phase 2: Build nodes and registry
    let (nodes, registry) = build_execution_nodes(
        segments,
        bookmarks_needing_push,
        prs_to_update_base,
        prs_to_create,
        prs_to_publish,
    );

    // Phase 3: Resolve constraints to edges
    let edges = resolve_constraints(&constraints, &registry);

    // Phase 4: Topological sort
    let steps = topo_sort_steps(&nodes, &edges)?;

    Ok((constraints, steps))
}

/// Map bookmark name to stack position for relative ordering
fn build_stack_index(segments: &[NarrowedBookmarkSegment]) -> HashMap<String, usize> {
    segments
        .iter()
        .enumerate()
        .map(|(idx, seg)| (seg.bookmark.name.clone(), idx))
        .collect()
}

/// Collect all dependency constraints declaratively.
///
/// This phase creates typed constraints without resolving them to indices.
/// Constraints may reference operations that won't exist in the final plan
/// (e.g., already-synced bookmarks have no Push node); resolution handles this.
fn collect_constraints(
    segments: &[NarrowedBookmarkSegment],
    prs_to_update_base: &[PrBaseUpdate],
    prs_to_create: &[PrToCreate],
    stack_index: &HashMap<String, usize>,
) -> Vec<ExecutionConstraint> {
    let mut constraints = Vec::new();

    // Constraint: Push(parent) → Push(child) for stack order
    for window in segments.windows(2) {
        constraints.push(ExecutionConstraint::PushOrder {
            parent: PushRef(window[0].bookmark.name.clone()),
            child: PushRef(window[1].bookmark.name.clone()),
        });
    }

    // Constraint: Push(expected_base) → UpdateBase(PR)
    for update in prs_to_update_base {
        constraints.push(ExecutionConstraint::PushBeforeRetarget {
            base: PushRef(update.expected_base.clone()),
            pr: UpdateRef(update.bookmark.name.clone()),
        });
    }

    // Constraint: UpdateBase(PR) → Push(current_base) when swapping
    for update in prs_to_update_base {
        if update.expected_base != update.current_base {
            let current_pos = stack_index.get(&update.current_base);
            let bookmark_pos = stack_index.get(&update.bookmark.name);
            if let (Some(&current_pos), Some(&bookmark_pos)) = (current_pos, bookmark_pos) {
                if current_pos > bookmark_pos {
                    // Current base is now below this bookmark - swap scenario
                    constraints.push(ExecutionConstraint::RetargetBeforePush {
                        pr: UpdateRef(update.bookmark.name.clone()),
                        old_base: PushRef(update.current_base.clone()),
                    });
                }
            }
        }
    }

    // Constraint: Push(bookmark) → CreatePr(bookmark)
    for create in prs_to_create {
        constraints.push(ExecutionConstraint::PushBeforeCreate {
            push: PushRef(create.bookmark.name.clone()),
            create: CreateRef(create.bookmark.name.clone()),
        });
    }

    // Constraint: CreatePr(parent) → CreatePr(child)
    for window in segments.windows(2) {
        constraints.push(ExecutionConstraint::CreateOrder {
            parent: CreateRef(window[0].bookmark.name.clone()),
            child: CreateRef(window[1].bookmark.name.clone()),
        });
    }

    constraints
}

/// Build execution nodes for all operations
fn build_execution_nodes(
    segments: &[NarrowedBookmarkSegment],
    bookmarks_needing_push: &[Bookmark],
    prs_to_update_base: &[PrBaseUpdate],
    prs_to_create: &[PrToCreate],
    prs_to_publish: &[PullRequest],
) -> (Vec<ExecutionNode>, NodeRegistry) {
    let mut nodes = Vec::new();
    let mut order = 0usize;
    let mut registry = NodeRegistry::default();

    // Build push set for O(1) lookup
    let push_set: HashSet<_> = bookmarks_needing_push.iter().map(|b| &b.name).collect();

    // Add push nodes in stack order
    for seg in segments {
        if push_set.contains(&seg.bookmark.name) {
            let bookmark = bookmarks_needing_push
                .iter()
                .find(|b| b.name == seg.bookmark.name)
                .unwrap()
                .clone();
            registry.register_push(&seg.bookmark.name, nodes.len());
            nodes.push(ExecutionNode {
                step: ExecutionStep::Push(bookmark),
                order,
            });
            order += 1;
        }
    }

    // Add any pushes not in segments (shouldn't happen, but be safe)
    for bookmark in bookmarks_needing_push {
        if !registry.push.contains_key(&bookmark.name) {
            registry.register_push(&bookmark.name, nodes.len());
            nodes.push(ExecutionNode {
                step: ExecutionStep::Push(bookmark.clone()),
                order,
            });
            order += 1;
        }
    }

    // Add update base nodes
    for update in prs_to_update_base {
        registry.register_update(&update.bookmark.name, nodes.len());
        nodes.push(ExecutionNode {
            step: ExecutionStep::UpdateBase(update.clone()),
            order,
        });
        order += 1;
    }

    // Add create PR nodes (in stack order for proper base dependencies)
    let create_set: HashSet<_> = prs_to_create.iter().map(|c| &c.bookmark.name).collect();
    for seg in segments {
        if create_set.contains(&seg.bookmark.name) {
            let create = prs_to_create
                .iter()
                .find(|c| c.bookmark.name == seg.bookmark.name)
                .unwrap()
                .clone();
            registry.register_create(&seg.bookmark.name, nodes.len());
            nodes.push(ExecutionNode {
                step: ExecutionStep::CreatePr(create),
                order,
            });
            order += 1;
        }
    }

    // Add publish nodes
    for pr in prs_to_publish {
        registry.register_publish(&pr.head_ref, nodes.len());
        nodes.push(ExecutionNode {
            step: ExecutionStep::PublishPr(pr.clone()),
            order,
        });
        order += 1;
    }

    (nodes, registry)
}

/// Resolve constraints to adjacency list edges.
///
/// Constraints that reference non-existent nodes are silently skipped.
/// This is expected: e.g., a Push constraint for an already-synced bookmark.
fn resolve_constraints(constraints: &[ExecutionConstraint], registry: &NodeRegistry) -> Vec<Vec<usize>> {
    let mut edges = vec![Vec::new(); registry.len()];

    for constraint in constraints {
        if let Some((from, to)) = constraint.resolve(registry) {
            if !edges[from].contains(&to) {
                edges[from].push(to);
                tracing::trace!(%constraint, from, to, "Resolved constraint to edge");
            }
        } else {
            tracing::trace!(%constraint, "Constraint skipped (endpoint not in plan)");
        }
    }

    edges
}

/// Topologically sort nodes respecting dependencies
fn topo_sort_steps(nodes: &[ExecutionNode], edges: &[Vec<usize>]) -> Result<Vec<ExecutionStep>> {
    // Kahn's algorithm with heap for stable ordering
    let mut indegree = vec![0usize; nodes.len()];
    for edge_list in edges {
        for &to in edge_list {
            indegree[to] += 1;
        }
    }

    // Use min-heap by (order, idx) for deterministic output
    let mut ready = BinaryHeap::new();
    for (idx, node) in nodes.iter().enumerate() {
        if indegree[idx] == 0 {
            ready.push(Reverse((node.order, idx)));
        }
    }

    let mut sorted = Vec::with_capacity(nodes.len());
    while let Some(Reverse((_order, idx))) = ready.pop() {
        sorted.push(idx);
        for &to in &edges[idx] {
            indegree[to] -= 1;
            if indegree[to] == 0 {
                ready.push(Reverse((nodes[to].order, to)));
            }
        }
    }

    if sorted.len() != nodes.len() {
        // Collect nodes stuck in the cycle (indegree > 0 means couldn't be scheduled)
        let cycle_nodes: Vec<String> = nodes
            .iter()
            .enumerate()
            .filter(|(idx, _)| indegree[*idx] > 0)
            .map(|(_, node)| format!("{}", node.step))
            .collect();

        tracing::error!(
            cycle_nodes = ?cycle_nodes,
            "Scheduler cycle detected - this is a bug in jj-ryu"
        );

        return Err(Error::SchedulerCycle {
            message: "Dependency cycle in execution plan - this is a bug in jj-ryu, please report it".to_string(),
            cycle_nodes,
        });
    }

    Ok(sorted
        .into_iter()
        .map(|idx| nodes[idx].step.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bookmark(name: &str, has_remote: bool, is_synced: bool) -> Bookmark {
        Bookmark {
            name: name.to_string(),
            commit_id: format!("{name}_commit"),
            change_id: format!("{name}_change"),
            has_remote,
            is_synced,
        }
    }

    fn make_segment(name: &str) -> NarrowedBookmarkSegment {
        NarrowedBookmarkSegment {
            bookmark: make_bookmark(name, false, false),
            changes: vec![],
        }
    }

    fn make_pr(number: u64, bookmark: &str, base: &str) -> PullRequest {
        PullRequest {
            number,
            html_url: format!("https://github.com/test/test/pull/{number}"),
            base_ref: base.to_string(),
            head_ref: bookmark.to_string(),
            title: format!("PR for {bookmark}"),
            node_id: Some(format!("PR_node_{number}")),
            is_draft: false,
        }
    }

    fn make_update(
        bookmark: &Bookmark,
        current_base: &str,
        expected_base: &str,
        pr_number: u64,
    ) -> PrBaseUpdate {
        PrBaseUpdate {
            bookmark: bookmark.clone(),
            current_base: current_base.to_string(),
            expected_base: expected_base.to_string(),
            pr: make_pr(pr_number, &bookmark.name, current_base),
        }
    }

    fn make_create(bookmark: &Bookmark, base_branch: &str) -> PrToCreate {
        PrToCreate {
            bookmark: bookmark.clone(),
            base_branch: base_branch.to_string(),
            title: format!("Add {}", bookmark.name),
            draft: false,
        }
    }

    fn find_step_index(steps: &[ExecutionStep], predicate: impl Fn(&ExecutionStep) -> bool) -> Option<usize> {
        steps.iter().position(predicate)
    }

    #[test]
    fn test_bookmark_needs_push() {
        let bm1 = make_bookmark("feat-a", false, false);
        assert!(!bm1.has_remote || !bm1.is_synced);

        let bm2 = make_bookmark("feat-b", true, false);
        assert!(!bm2.has_remote || !bm2.is_synced);

        let bm3 = make_bookmark("feat-c", true, true);
        assert!(bm3.has_remote && bm3.is_synced);
    }

    #[test]
    fn test_pr_to_create_structure() {
        let pr_create = PrToCreate {
            bookmark: make_bookmark("feat-a", false, false),
            base_branch: "main".to_string(),
            title: "Add feature A".to_string(),
            draft: false,
        };

        assert_eq!(pr_create.bookmark.name, "feat-a");
        assert_eq!(pr_create.base_branch, "main");
        assert_eq!(pr_create.title, "Add feature A");
        assert!(!pr_create.draft);
    }

    #[test]
    fn test_execution_steps_simple_push_order() {
        let segments = vec![make_segment("a"), make_segment("b")];
        let pushes = vec![
            make_bookmark("a", false, false),
            make_bookmark("b", false, false),
        ];

        let (_constraints, steps) = build_execution_steps(&segments, &pushes, &[], &[], &[]).unwrap();

        let push_a = find_step_index(&steps, |s| matches!(s, ExecutionStep::Push(b) if b.name == "a"));
        let push_b = find_step_index(&steps, |s| matches!(s, ExecutionStep::Push(b) if b.name == "b"));

        assert!(push_a.unwrap() < push_b.unwrap(), "pushes should follow stack order");
    }

    #[test]
    fn test_execution_steps_push_before_create() {
        let bm_a = make_bookmark("a", false, false);
        let segments = vec![make_segment("a")];
        let pushes = vec![bm_a.clone()];
        let creates = vec![make_create(&bm_a, "main")];

        let (_constraints, steps) = build_execution_steps(&segments, &pushes, &[], &creates, &[]).unwrap();

        let push_a = find_step_index(&steps, |s| matches!(s, ExecutionStep::Push(b) if b.name == "a")).unwrap();
        let create_a = find_step_index(&steps, |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "a")).unwrap();

        assert!(push_a < create_a, "push must happen before create");
    }

    #[test]
    fn test_execution_steps_create_order_follows_stack() {
        let bm_a = make_bookmark("a", false, false);
        let bm_b = make_bookmark("b", false, false);
        let segments = vec![make_segment("a"), make_segment("b")];
        let pushes = vec![bm_a.clone(), bm_b.clone()];
        let creates = vec![make_create(&bm_a, "main"), make_create(&bm_b, "a")];

        let (_constraints, steps) = build_execution_steps(&segments, &pushes, &[], &creates, &[]).unwrap();

        let create_a = find_step_index(&steps, |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "a")).unwrap();
        let create_b = find_step_index(&steps, |s| matches!(s, ExecutionStep::CreatePr(c) if c.bookmark.name == "b")).unwrap();

        assert!(create_a < create_b, "creates should follow stack order");
    }

    #[test]
    fn test_execution_steps_swap_order() {
        // Scenario: Stack was A -> B, now B -> A (swapped)
        let bm_a = make_bookmark("a", false, false);
        let bm_b = make_bookmark("b", false, false);

        // New stack order: B is root, A is leaf
        let segments = vec![make_segment("b"), make_segment("a")];
        let pushes = vec![bm_a.clone(), bm_b.clone()];
        let updates = vec![
            make_update(&bm_b, "a", "main", 2), // B was on A, now on main
            make_update(&bm_a, "main", "b", 1), // A was on main, now on B
        ];

        let (_constraints, steps) = build_execution_steps(&segments, &pushes, &updates, &[], &[]).unwrap();

        let retarget_b = find_step_index(&steps, |s| matches!(s, ExecutionStep::UpdateBase(u) if u.bookmark.name == "b")).unwrap();
        let push_a = find_step_index(&steps, |s| matches!(s, ExecutionStep::Push(b) if b.name == "a")).unwrap();
        let push_b = find_step_index(&steps, |s| matches!(s, ExecutionStep::Push(b) if b.name == "b")).unwrap();

        assert!(retarget_b < push_a, "b must move off a before pushing a");
        assert!(push_b < push_a, "push order should follow new stack (b before a)");
    }

    #[test]
    fn test_plan_is_empty() {
        let plan = SubmissionPlan {
            segments: vec![],
            constraints: vec![],
            execution_steps: vec![],
            existing_prs: HashMap::new(),
            remote: "origin".to_string(),
            default_branch: "main".to_string(),
        };

        assert!(plan.is_empty());
        assert_eq!(plan.count_pushes(), 0);
        assert_eq!(plan.count_creates(), 0);
    }

    #[test]
    fn test_plan_counts() {
        let bm = make_bookmark("a", false, false);
        let plan = SubmissionPlan {
            segments: vec![make_segment("a")],
            constraints: vec![],
            execution_steps: vec![
                ExecutionStep::Push(bm.clone()),
                ExecutionStep::CreatePr(make_create(&bm, "main")),
            ],
            existing_prs: HashMap::new(),
            remote: "origin".to_string(),
            default_branch: "main".to_string(),
        };

        assert!(!plan.is_empty());
        assert_eq!(plan.count_pushes(), 1);
        assert_eq!(plan.count_creates(), 1);
        assert_eq!(plan.count_updates(), 0);
        assert_eq!(plan.count_publishes(), 0);
    }
}
