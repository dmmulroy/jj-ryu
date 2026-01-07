//! jj-ryu - Stacked PRs for Jujutsu
//!
//! This library provides the core functionality for managing stacked pull requests
//! when using Jujutsu (jj) as your version control system. It supports both GitHub
//! and GitLab.
//!
//! # Architecture
//!
//! The library is designed to be interface-agnostic, allowing it to be used from:
//! - CLI applications
//! - TUI applications
//! - Web servers / REST APIs
//! - WebSocket servers
//!
//! All I/O is async and state is passed explicitly (no globals).

pub mod auth;
pub mod error;
pub mod graph;
pub mod platform;
pub mod repo;
pub mod submit;
pub mod tracking;
pub mod types;

pub use error::{Error, Result};
pub use types::*;
