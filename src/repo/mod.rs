//! jj-lib workspace wrapper
//!
//! Provides a high-level interface to jj repository operations.

mod workspace;

pub use workspace::{select_remote, JjWorkspace};
