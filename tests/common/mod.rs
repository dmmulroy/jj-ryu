//! Common test utilities for jj-ryu tests

pub mod fixtures;
pub mod mock_platform;
pub mod temp_repo;

// Re-exports for convenience - not all test binaries use all exports
#[allow(unused_imports)]
pub use fixtures::*;
#[allow(unused_imports)]
pub use mock_platform::MockPlatformService;
#[allow(unused_imports)]
pub use temp_repo::TempJjRepo;
