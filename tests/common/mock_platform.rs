//! Mock platform service for testing
//!
//! These are test utilities - not all may be used in current tests but are
//! available for future test development.

#![allow(dead_code)]

use async_trait::async_trait;
use jj_ryu::error::{Error, Result};
use jj_ryu::platform::PlatformService;
use jj_ryu::types::{PlatformConfig, PrComment, PullRequest};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Call record for `create_pr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePrCall {
    pub head: String,
    pub base: String,
    pub title: String,
}

/// Call record for `update_pr_base`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateBaseCall {
    pub pr_number: u64,
    pub new_base: String,
}

/// Call record for `create_pr_comment`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCommentCall {
    pub pr_number: u64,
    pub body: String,
}

/// Simple mock platform service for testing
///
/// This manually implements `PlatformService` rather than using mockall,
/// because mockall has issues with methods returning references.
///
/// Features:
/// - Auto-incrementing PR numbers
/// - Call tracking for verification
/// - Configurable responses per branch
/// - Error injection for failure path testing
pub struct MockPlatformService {
    config: PlatformConfig,
    next_pr_number: AtomicU64,
    find_pr_responses: Mutex<HashMap<String, Option<PullRequest>>>,
    list_comments_responses: Mutex<HashMap<u64, Vec<PrComment>>>,
    // Call tracking
    find_pr_calls: Mutex<Vec<String>>,
    create_pr_calls: Mutex<Vec<CreatePrCall>>,
    update_base_calls: Mutex<Vec<UpdateBaseCall>>,
    create_comment_calls: Mutex<Vec<CreateCommentCall>>,
    list_comments_calls: Mutex<Vec<u64>>,
    // Error injection
    error_on_find_pr: Mutex<Option<String>>,
    error_on_create_pr: Mutex<Option<String>>,
    error_on_update_base: Mutex<Option<String>>,
}

impl MockPlatformService {
    /// Create a new mock with the given config
    pub fn with_config(config: PlatformConfig) -> Self {
        Self {
            config,
            next_pr_number: AtomicU64::new(1),
            find_pr_responses: Mutex::new(HashMap::new()),
            list_comments_responses: Mutex::new(HashMap::new()),
            find_pr_calls: Mutex::new(Vec::new()),
            create_pr_calls: Mutex::new(Vec::new()),
            update_base_calls: Mutex::new(Vec::new()),
            create_comment_calls: Mutex::new(Vec::new()),
            list_comments_calls: Mutex::new(Vec::new()),
            error_on_find_pr: Mutex::new(None),
            error_on_create_pr: Mutex::new(None),
            error_on_update_base: Mutex::new(None),
        }
    }

    // === Error injection methods ===

    /// Make `find_existing_pr` return an error
    pub fn fail_find_pr(&self, msg: &str) {
        *self.error_on_find_pr.lock().unwrap() = Some(msg.to_string());
    }

    /// Make `create_pr` return an error
    pub fn fail_create_pr(&self, msg: &str) {
        *self.error_on_create_pr.lock().unwrap() = Some(msg.to_string());
    }

    /// Make `update_pr_base` return an error
    pub fn fail_update_base(&self, msg: &str) {
        *self.error_on_update_base.lock().unwrap() = Some(msg.to_string());
    }

    /// Set the response for `find_existing_pr` for a specific branch
    pub fn set_find_pr_response(&self, branch: &str, pr: Option<PullRequest>) {
        self.find_pr_responses
            .lock()
            .unwrap()
            .insert(branch.to_string(), pr);
    }

    /// Set the response for `list_pr_comments` for a specific PR
    pub fn set_list_comments_response(&self, pr_number: u64, comments: Vec<PrComment>) {
        self.list_comments_responses
            .lock()
            .unwrap()
            .insert(pr_number, comments);
    }

    // === Call verification methods ===

    /// Get all branches that `find_existing_pr` was called with
    pub fn get_find_pr_calls(&self) -> Vec<String> {
        self.find_pr_calls.lock().unwrap().clone()
    }

    /// Get all `create_pr` calls
    pub fn get_create_pr_calls(&self) -> Vec<CreatePrCall> {
        self.create_pr_calls.lock().unwrap().clone()
    }

    /// Get all `update_pr_base` calls
    pub fn get_update_base_calls(&self) -> Vec<UpdateBaseCall> {
        self.update_base_calls.lock().unwrap().clone()
    }

    /// Get all `create_pr_comment` calls
    pub fn get_create_comment_calls(&self) -> Vec<CreateCommentCall> {
        self.create_comment_calls.lock().unwrap().clone()
    }

    /// Get all `list_pr_comments` calls
    pub fn get_list_comments_calls(&self) -> Vec<u64> {
        self.list_comments_calls.lock().unwrap().clone()
    }

    /// Assert that `create_pr` was called with specific head and base
    pub fn assert_create_pr_called(&self, head: &str, base: &str) {
        let calls = self.get_create_pr_calls();
        assert!(
            calls.iter().any(|c| c.head == head && c.base == base),
            "Expected create_pr({head}, {base}) but got: {calls:?}"
        );
    }

    /// Assert that `update_pr_base` was called with specific args
    pub fn assert_update_base_called(&self, pr_number: u64, new_base: &str) {
        let calls = self.get_update_base_calls();
        assert!(
            calls
                .iter()
                .any(|c| c.pr_number == pr_number && c.new_base == new_base),
            "Expected update_pr_base({pr_number}, {new_base}) but got: {calls:?}"
        );
    }

    /// Assert that `find_existing_pr` was called for each bookmark
    pub fn assert_find_pr_called_for(&self, branches: &[&str]) {
        let calls = self.get_find_pr_calls();
        for branch in branches {
            assert!(
                calls.contains(&branch.to_string()),
                "Expected find_existing_pr({branch}) but got: {calls:?}"
            );
        }
    }
}

#[async_trait]
impl PlatformService for MockPlatformService {
    async fn find_existing_pr(&self, head_branch: &str) -> Result<Option<PullRequest>> {
        self.find_pr_calls
            .lock()
            .unwrap()
            .push(head_branch.to_string());

        // Check for injected error
        if let Some(msg) = self.error_on_find_pr.lock().unwrap().as_ref() {
            return Err(Error::Platform(msg.clone()));
        }

        let responses = self.find_pr_responses.lock().unwrap();
        Ok(responses.get(head_branch).cloned().flatten())
    }

    async fn create_pr_with_options(
        &self,
        head: &str,
        base: &str,
        title: &str,
        draft: bool,
    ) -> Result<PullRequest> {
        self.create_pr_calls.lock().unwrap().push(CreatePrCall {
            head: head.to_string(),
            base: base.to_string(),
            title: title.to_string(),
        });

        // Check for injected error
        if let Some(msg) = self.error_on_create_pr.lock().unwrap().as_ref() {
            return Err(Error::Platform(msg.clone()));
        }

        let number = self.next_pr_number.fetch_add(1, Ordering::SeqCst);
        let pr = PullRequest {
            number,
            html_url: format!("https://github.com/test/repo/pull/{number}"),
            base_ref: base.to_string(),
            head_ref: head.to_string(),
            title: title.to_string(),
            node_id: Some(format!("PR_node_{number}")),
            is_draft: draft,
        };
        Ok(pr)
    }

    async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<PullRequest> {
        self.update_base_calls.lock().unwrap().push(UpdateBaseCall {
            pr_number,
            new_base: new_base.to_string(),
        });

        // Check for injected error
        if let Some(msg) = self.error_on_update_base.lock().unwrap().as_ref() {
            return Err(Error::Platform(msg.clone()));
        }

        Ok(PullRequest {
            number: pr_number,
            html_url: format!("https://github.com/test/repo/pull/{pr_number}"),
            base_ref: new_base.to_string(),
            head_ref: "updated".to_string(),
            title: "Updated PR".to_string(),
            node_id: Some(format!("PR_node_{pr_number}")),
            is_draft: false,
        })
    }

    async fn list_pr_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        self.list_comments_calls.lock().unwrap().push(pr_number);
        let responses = self.list_comments_responses.lock().unwrap();
        Ok(responses.get(&pr_number).cloned().unwrap_or_default())
    }

    async fn create_pr_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        self.create_comment_calls
            .lock()
            .unwrap()
            .push(CreateCommentCall {
                pr_number,
                body: body.to_string(),
            });
        Ok(())
    }

    async fn update_pr_comment(
        &self,
        _pr_number: u64,
        _comment_id: u64,
        _body: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn publish_pr(&self, pr_number: u64) -> Result<PullRequest> {
        Ok(PullRequest {
            number: pr_number,
            html_url: format!("https://github.com/test/repo/pull/{pr_number}"),
            base_ref: "main".to_string(),
            head_ref: "published".to_string(),
            title: "Published PR".to_string(),
            node_id: Some(format!("PR_node_{pr_number}")),
            is_draft: false, // After publishing, is_draft is false
        })
    }

    fn config(&self) -> &PlatformConfig {
        &self.config
    }
}
