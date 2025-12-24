//! GitLab platform service implementation

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::types::{Platform, PlatformConfig, PrComment, PullRequest};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// GitLab service using reqwest
pub struct GitLabService {
    client: Client,
    token: String,
    host: String,
    config: PlatformConfig,
    project_path: String,
}

#[derive(Deserialize)]
struct MergeRequest {
    iid: u64,
    web_url: String,
    source_branch: String,
    target_branch: String,
    title: String,
    #[serde(default)]
    draft: bool,
}

#[derive(Deserialize)]
struct MrNote {
    id: u64,
    body: String,
    system: bool,
}

impl From<MergeRequest> for PullRequest {
    fn from(mr: MergeRequest) -> Self {
        Self {
            number: mr.iid,
            html_url: mr.web_url,
            base_ref: mr.target_branch,
            head_ref: mr.source_branch,
            title: mr.title,
            node_id: None, // GitLab doesn't use GraphQL node IDs
            is_draft: mr.draft,
        }
    }
}

#[derive(Serialize)]
struct CreateMrPayload {
    source_branch: String,
    target_branch: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft: Option<bool>,
}

/// Default request timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 30;

impl GitLabService {
    /// Create a new GitLab service
    pub fn new(token: String, owner: String, repo: String, host: Option<String>) -> Result<Self> {
        let host = host.unwrap_or_else(|| "gitlab.com".to_string());
        let project_path = format!("{owner}/{repo}");

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::GitLabApi(format!("failed to create HTTP client: {e}")))?;

        let config_host = if host == "gitlab.com" {
            None
        } else {
            Some(host.clone())
        };

        Ok(Self {
            client,
            token,
            host,
            config: PlatformConfig {
                platform: Platform::GitLab,
                owner,
                repo,
                host: config_host,
            },
            project_path,
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("https://{}/api/v4{}", self.host, path)
    }

    fn encoded_project(&self) -> String {
        urlencoding::encode(&self.project_path).into_owned()
    }
}

#[async_trait]
impl PlatformService for GitLabService {
    async fn find_existing_pr(&self, head_branch: &str) -> Result<Option<PullRequest>> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests",
            self.encoded_project()
        ));

        let mrs: Vec<MergeRequest> = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .query(&[("source_branch", head_branch), ("state", "opened")])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?
            .json()
            .await?;

        Ok(mrs.into_iter().next().map(Into::into))
    }

    async fn create_pr_with_options(
        &self,
        head: &str,
        base: &str,
        title: &str,
        draft: bool,
    ) -> Result<PullRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests",
            self.encoded_project()
        ));

        let payload = CreateMrPayload {
            source_branch: head.to_string(),
            target_branch: base.to_string(),
            title: title.to_string(),
            draft: if draft { Some(true) } else { None },
        };

        let mr: MergeRequest = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?
            .json()
            .await?;

        Ok(mr.into())
    }

    async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<PullRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            self.encoded_project(),
            pr_number
        ));

        let mr: MergeRequest = self
            .client
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({ "target_branch": new_base }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?
            .json()
            .await?;

        Ok(mr.into())
    }

    async fn publish_pr(&self, pr_number: u64) -> Result<PullRequest> {
        // GitLab: Use state_event to mark MR as ready
        // We need to remove the draft/WIP status
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            self.encoded_project(),
            pr_number
        ));

        // GitLab uses state_event: "ready" to mark as ready for review
        let mr: MergeRequest = self
            .client
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({ "state_event": "ready" }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?
            .json()
            .await?;

        Ok(mr.into())
    }

    async fn list_pr_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes",
            self.encoded_project(),
            pr_number
        ));

        let notes: Vec<MrNote> = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?
            .json()
            .await?;

        Ok(notes
            .into_iter()
            .filter(|n| !n.system)
            .map(|n| PrComment {
                id: n.id,
                body: n.body,
            })
            .collect())
    }

    async fn create_pr_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes",
            self.encoded_project(),
            pr_number
        ));

        self.client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?;

        Ok(())
    }

    async fn update_pr_comment(&self, pr_number: u64, comment_id: u64, body: &str) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes/{}",
            self.encoded_project(),
            pr_number,
            comment_id
        ));

        self.client
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GitLabApi(e.to_string()))?;

        Ok(())
    }

    fn config(&self) -> &PlatformConfig {
        &self.config
    }
}
