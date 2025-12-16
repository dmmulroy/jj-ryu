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
    analyze_submission, create_narrowed_segments, generate_pr_title, get_base_branch,
    select_bookmark_for_segment, SubmissionAnalysis,
};
pub use execute::{
    execute_submission, format_stack_comment, SubmissionResult, STACK_COMMENT_THIS_PR,
};

// Exports for testing stack comment formatting (used by integration tests)
pub use execute::{
    build_stack_comment_data, StackCommentData, StackItem, COMMENT_DATA_PREFIX,
    COMMENT_DATA_POSTFIX,
};
pub use plan::{create_submission_plan, PrBaseUpdate, PrToCreate, SubmissionPlan};
pub use progress::{NoopProgress, Phase, ProgressCallback, PushStatus};
