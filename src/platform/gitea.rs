//! Gitea platform service implementation

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::types::{Platform, PlatformConfig, PrComment, PullRequest};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Gitea service using reqwest
pub struct GiteaService {
    client: Client,
    token: String,
    base_url: String,
    config: PlatformConfig,
}

#[derive(Deserialize)]
struct GiteaBranchRef {
    #[serde(rename = "ref")]
    reference: String,
}

#[derive(Deserialize)]
struct GiteaHeadRef {
    #[serde(rename = "ref")]
    reference: String,
    #[allow(dead_code)]
    label: Option<String>,
}

#[derive(Deserialize)]
struct GiteaPullRequest {
    number: u64,
    html_url: String,
    title: String,
    #[serde(default)]
    draft: bool,
    base: GiteaBranchRef,
    head: GiteaHeadRef,
}

#[derive(Deserialize)]
struct GiteaIssueComment {
    id: u64,
    body: String,
}

#[derive(Serialize)]
struct CreatePrPayload<'a> {
    title: String,
    head: &'a str,
    base: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft: Option<bool>,
}

impl From<GiteaPullRequest> for PullRequest {
    fn from(pr: GiteaPullRequest) -> Self {
        Self {
            number: pr.number,
            html_url: pr.html_url,
            base_ref: pr.base.reference,
            head_ref: pr.head.reference,
            title: pr.title,
            node_id: None,
            is_draft: pr.draft,
        }
    }
}

const DEFAULT_TIMEOUT_SECS: u64 = 30;

impl GiteaService {
    /// Create a new Gitea service
    pub fn new(token: String, owner: String, repo: String, host: Option<String>) -> Result<Self> {
        let raw_host = host.unwrap_or_else(|| "gitea.com".to_string());
        let base_url = if raw_host.starts_with("http://") || raw_host.starts_with("https://") {
            raw_host.clone()
        } else {
            format!("https://{raw_host}")
        };

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::GiteaApi(format!("failed to create HTTP client: {e}")))?;

        let config_host = if raw_host == "gitea.com" {
            None
        } else if raw_host.starts_with("http://") || raw_host.starts_with("https://") {
            Some(
                raw_host
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_string(),
            )
        } else {
            Some(raw_host.clone())
        };

        Ok(Self {
            client,
            token,
            base_url,
            config: PlatformConfig {
                platform: Platform::Gitea,
                owner,
                repo,
                host: config_host,
            },
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    fn auth_header(&self) -> String {
        format!("token {}", self.token)
    }

    fn pulls_path(&self) -> String {
        format!("/repos/{}/{}/pulls", self.config.owner, self.config.repo)
    }

    fn comments_path(&self, pr_number: u64) -> String {
        format!(
            "/repos/{}/{}/issues/{pr_number}/comments",
            self.config.owner, self.config.repo
        )
    }

    fn strip_wip_prefix(title: &str) -> String {
        title
            .trim_start_matches("WIP: ")
            .trim_start_matches("[WIP] ")
            .to_string()
    }
}

#[async_trait]
impl PlatformService for GiteaService {
    async fn find_existing_pr(&self, head_branch: &str) -> Result<Option<PullRequest>> {
        debug!(head_branch, "finding existing Gitea PR");
        let url = self.api_url(&self.pulls_path());

        let prs: Vec<GiteaPullRequest> = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .query(&[("state", "open")])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?
            .json()
            .await?;

        Ok(prs
            .into_iter()
            .find(|pr| pr.head.reference == head_branch)
            .map(Into::into))
    }

    async fn create_pr_with_options(
        &self,
        head: &str,
        base: &str,
        title: &str,
        draft: bool,
    ) -> Result<PullRequest> {
        debug!(head, base, draft, "creating Gitea PR");
        let url = self.api_url(&self.pulls_path());
        let payload = CreatePrPayload {
            title: title.to_string(),
            head,
            base,
            draft: if draft { Some(true) } else { None },
        };

        let pr: GiteaPullRequest = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?
            .json()
            .await?;

        Ok(pr.into())
    }

    async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<PullRequest> {
        debug!(pr_number, new_base, "updating Gitea PR base");
        let url = self.api_url(&format!("{}/{}", self.pulls_path(), pr_number));

        let pr: GiteaPullRequest = self
            .client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({ "base": new_base }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?
            .json()
            .await?;

        Ok(pr.into())
    }

    async fn publish_pr(&self, pr_number: u64) -> Result<PullRequest> {
        debug!(pr_number, "publishing Gitea PR");
        let url = self.api_url(&format!("{}/{}", self.pulls_path(), pr_number));

        let current: GiteaPullRequest = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?
            .json()
            .await?;

        let published_title = Self::strip_wip_prefix(&current.title);
        if published_title == current.title {
            return Ok(current.into());
        }

        let pr: GiteaPullRequest = self
            .client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({ "title": published_title }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?
            .json()
            .await?;

        Ok(pr.into())
    }

    async fn list_pr_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        debug!(pr_number, "listing Gitea PR comments");
        let url = self.api_url(&self.comments_path(pr_number));

        let comments: Vec<GiteaIssueComment> = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?
            .json()
            .await?;

        Ok(comments
            .into_iter()
            .map(|comment| PrComment {
                id: comment.id,
                body: comment.body,
            })
            .collect())
    }

    async fn create_pr_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        debug!(pr_number, "creating Gitea PR comment");
        let url = self.api_url(&self.comments_path(pr_number));

        self.client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?;

        Ok(())
    }

    async fn update_pr_comment(&self, _pr_number: u64, comment_id: u64, body: &str) -> Result<()> {
        debug!(comment_id, "updating Gitea PR comment");
        let url = self.api_url(&format!(
            "/repos/{}/{}/issues/comments/{}",
            self.config.owner, self.config.repo, comment_id
        ));

        self.client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| Error::GiteaApi(e.to_string()))?;

        Ok(())
    }

    fn config(&self) -> &PlatformConfig {
        &self.config
    }
}
