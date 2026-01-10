//! Azure DevOps platform service implementation

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::types::{Platform, PlatformConfig, PrComment, PullRequest};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Azure DevOps service using reqwest
pub struct AzureDevOpsService {
    client: Client,
    token: String,
    host: String,
    config: PlatformConfig,
    organization: String,
    #[allow(dead_code)]
    project: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestResponse {
    pull_request_id: u64,
    #[serde(rename = "url")]
    _api_url: String,
    source_ref_name: String,
    target_ref_name: String,
    title: String,
    #[serde(default)]
    is_draft: bool,
    repository: Repository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    web_url: String,
}

impl PullRequestResponse {
    fn into_pull_request(self) -> PullRequest {
        // Strip refs/heads/ prefix from branch names
        let source_branch = self
            .source_ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(&self.source_ref_name)
            .to_string();
        let target_branch = self
            .target_ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(&self.target_ref_name)
            .to_string();

        let html_url = format!(
            "{}/pullrequest/{}",
            self.repository.web_url.trim_end_matches('/'),
            self.pull_request_id
        );

        PullRequest {
            number: self.pull_request_id,
            html_url,
            base_ref: target_branch,
            head_ref: source_branch,
            title: self.title,
            node_id: None,
            is_draft: self.is_draft,
        }
    }
}

#[derive(Deserialize)]
struct PullRequestListResponse {
    value: Vec<PullRequestResponse>,
}

#[derive(Deserialize)]
struct Thread {
    id: u64,
    comments: Vec<Comment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Comment {
    id: u64,
    content: String,
    #[serde(rename = "commentType")]
    type_id: u32, // 1 = text, 2 = system
}

#[derive(Deserialize)]
struct ThreadListResponse {
    value: Vec<Thread>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreatePrPayload {
    source_ref_name: String,
    target_ref_name: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_draft: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateThreadPayload {
    comments: Vec<CreateCommentPayload>,
    status: u32, // 1 = active
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateCommentPayload {
    parent_comment_id: u32, // 0 for root
    content: String,
    comment_type: u32, // 1 = text
}

/// Default request timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 30;

impl AzureDevOpsService {
    /// Create a new Azure DevOps service
    ///
    /// # Arguments
    /// * `token` - Personal Access Token
    /// * `organization` - Azure DevOps organization name
    /// * `project` - Project name
    /// * `repo` - Repository name
    /// * `host` - Optional host (defaults to dev.azure.com)
    pub fn new(
        token: String,
        organization: String,
        project: String,
        repo: String,
        host: Option<String>,
    ) -> Result<Self> {
        let host = host.unwrap_or_else(|| "dev.azure.com".to_string());

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::AzureDevOpsApi(format!("failed to create HTTP client: {e}")))?;

        let config_host = if host == "dev.azure.com" {
            None
        } else {
            Some(host.clone())
        };

        Ok(Self {
            client,
            token,
            host,
            config: PlatformConfig {
                platform: Platform::AzureDevOps,
                owner: format!("{organization}/{project}"),
                repo,
                host: config_host,
            },
            organization,
            project,
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!(
            "https://{}/{}/{}/_apis{}",
            self.host, self.organization, self.project, path
        )
    }

    fn auth_header(&self) -> String {
        let auth = format!(":{}", self.token);
        let encoded = base64::engine::general_purpose::STANDARD.encode(auth);
        format!("Basic {encoded}")
    }

    fn branch_ref(branch: &str) -> String {
        if branch.starts_with("refs/") {
            branch.to_string()
        } else {
            format!("refs/heads/{branch}")
        }
    }
}

#[async_trait]
impl PlatformService for AzureDevOpsService {
    async fn find_existing_pr(&self, head_branch: &str) -> Result<Option<PullRequest>> {
        debug!(head_branch, "finding existing PR");
        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests",
            urlencoding::encode(&self.config.repo)
        ));

        let source_ref = Self::branch_ref(head_branch);

        let response: PullRequestListResponse = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .query(&[
                ("searchCriteria.sourceRefName", source_ref.as_str()),
                ("searchCriteria.status", "active"),
                ("api-version", "7.1-preview"),
            ])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?
            .json()
            .await?;

        let result: Option<PullRequest> = response
            .value
            .into_iter()
            .next()
            .map(PullRequestResponse::into_pull_request);

        if let Some(ref pr) = result {
            debug!(pr_id = pr.number, "found existing PR");
        } else {
            debug!("no existing PR found");
        }
        Ok(result)
    }

    async fn create_pr_with_options(
        &self,
        head: &str,
        base: &str,
        title: &str,
        draft: bool,
    ) -> Result<PullRequest> {
        debug!(head, base, draft, "creating PR");
        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests",
            urlencoding::encode(&self.config.repo)
        ));

        let payload = CreatePrPayload {
            source_ref_name: Self::branch_ref(head),
            target_ref_name: Self::branch_ref(base),
            title: title.to_string(),
            is_draft: if draft { Some(true) } else { None },
        };

        let pr: PullRequestResponse = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .query(&[("api-version", "7.1-preview")])
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?
            .json()
            .await?;

        let result = pr.into_pull_request();
        debug!(pr_id = result.number, "created PR");
        Ok(result)
    }

    async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<PullRequest> {
        debug!(pr_id = pr_number, new_base, "updating PR base");
        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests/{}",
            urlencoding::encode(&self.config.repo),
            pr_number
        ));

        let pr: PullRequestResponse = self
            .client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .query(&[("api-version", "7.1-preview")])
            .json(&serde_json::json!({ "targetRefName": Self::branch_ref(new_base) }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?
            .json()
            .await?;

        debug!(pr_id = pr_number, "updated PR base");
        Ok(pr.into_pull_request())
    }

    async fn publish_pr(&self, pr_number: u64) -> Result<PullRequest> {
        debug!(pr_id = pr_number, "publishing PR");
        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests/{}",
            urlencoding::encode(&self.config.repo),
            pr_number
        ));

        let pr: PullRequestResponse = self
            .client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .query(&[("api-version", "7.1-preview")])
            .json(&serde_json::json!({ "isDraft": false }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?
            .json()
            .await?;

        debug!(pr_id = pr_number, "published PR");
        Ok(pr.into_pull_request())
    }

    async fn list_pr_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        debug!(pr_id = pr_number, "listing PR comments");
        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests/{}/threads",
            urlencoding::encode(&self.config.repo),
            pr_number
        ));

        let response: ThreadListResponse = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .query(&[("api-version", "7.1-preview")])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?
            .json()
            .await?;

        // Flatten threads to comments, filtering out system comments
        let comments: Vec<PrComment> = response
            .value
            .into_iter()
            .flat_map(|thread| {
                thread.comments.into_iter().filter_map(|comment| {
                    // Only include text comments (type 1), not system comments (type 2)
                    if comment.type_id == 1 {
                        Some(PrComment {
                            id: comment.id,
                            body: comment.content,
                        })
                    } else {
                        None
                    }
                })
            })
            .collect();

        debug!(
            pr_id = pr_number,
            count = comments.len(),
            "listed PR comments"
        );
        Ok(comments)
    }

    async fn create_pr_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        debug!(pr_id = pr_number, "creating PR comment");
        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests/{}/threads",
            urlencoding::encode(&self.config.repo),
            pr_number
        ));

        let payload = CreateThreadPayload {
            comments: vec![CreateCommentPayload {
                parent_comment_id: 0,
                content: body.to_string(),
                comment_type: 1,
            }],
            status: 1, // active
        };

        self.client
            .post(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .query(&[("api-version", "7.1-preview")])
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?;

        debug!(pr_id = pr_number, "created PR comment");
        Ok(())
    }

    async fn update_pr_comment(&self, pr_number: u64, comment_id: u64, body: &str) -> Result<()> {
        debug!(pr_id = pr_number, comment_id, "updating PR comment");

        // Azure DevOps requires thread ID to update a comment
        // We need to find which thread contains this comment
        let threads_url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests/{}/threads",
            urlencoding::encode(&self.config.repo),
            pr_number
        ));

        let response: ThreadListResponse = self
            .client
            .get(&threads_url)
            .header("Authorization", self.auth_header())
            .query(&[("api-version", "7.1-preview")])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?
            .json()
            .await?;

        // Find the thread containing this comment
        let thread_id = response
            .value
            .iter()
            .find(|thread| thread.comments.iter().any(|c| c.id == comment_id))
            .map(|thread| thread.id)
            .ok_or_else(|| {
                Error::AzureDevOpsApi(format!("comment {comment_id} not found in any thread"))
            })?;

        let url = self.api_url(&format!(
            "/git/repositories/{}/pullrequests/{}/threads/{}/comments/{}",
            urlencoding::encode(&self.config.repo),
            pr_number,
            thread_id,
            comment_id
        ));

        self.client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .query(&[("api-version", "7.1-preview")])
            .json(&serde_json::json!({ "content": body }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::AzureDevOpsApi(e.to_string()))?;

        debug!(pr_id = pr_number, comment_id, "updated PR comment");
        Ok(())
    }

    fn config(&self) -> &PlatformConfig {
        &self.config
    }
}
