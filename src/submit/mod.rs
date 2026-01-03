//! Three-phase submission engine
//!
//! Handles the workflow of submitting stacked bookmarks as PRs/MRs:
//! 1. Analysis - understand what needs to be submitted
//! 2. Planning - determine what PRs to create/update
//! 3. Execution - perform the actual operations

mod analysis;
mod execute;
mod plan;
mod progress;

pub use analysis::{
    SubmissionAnalysis, analyze_submission, create_narrowed_segments, generate_pr_title,
    get_base_branch, select_bookmark_for_segment,
};
pub use execute::{
    STACK_COMMENT_THIS_PR, SubmissionResult, execute_submission, format_stack_comment,
};

// Exports for testing stack comment formatting (used by integration tests)
pub use execute::{
    COMMENT_DATA_POSTFIX, COMMENT_DATA_PREFIX, StackCommentData, StackItem,
    build_stack_comment_data,
};
pub use plan::{
    ExecutionConstraint, ExecutionStep, PrBaseUpdate, PrToCreate, SubmissionPlan,
    create_submission_plan,
};
pub use progress::{NoopProgress, Phase, ProgressCallback, PushStatus};
