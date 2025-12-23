//! GitHub platform service implementation

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::types::{Platform, PlatformConfig, PrComment, PullRequest};
use async_trait::async_trait;
use octocrab::Octocrab;

/// GitHub service using octocrab
pub struct GitHubService {
    client: Octocrab,
    config: PlatformConfig,
}

impl GitHubService {
    /// Create a new GitHub service
    pub fn new(token: &str, owner: String, repo: String, host: Option<String>) -> Result<Self> {
        let mut builder = Octocrab::builder().personal_token(token.to_string());

        if let Some(ref h) = host {
            let base_url = format!("https://{h}/api/v3");
            builder = builder
                .base_uri(&base_url)
                .map_err(|e| Error::GitHubApi(e.to_string()))?;
        }

        let client = builder
            .build()
            .map_err(|e| Error::GitHubApi(e.to_string()))?;

        Ok(Self {
            client,
            config: PlatformConfig {
                platform: Platform::GitHub,
                owner,
                repo,
                host,
            },
        })
    }
}

/// Helper to convert octocrab PR to our `PullRequest` type
fn pr_from_octocrab(pr: &octocrab::models::pulls::PullRequest) -> PullRequest {
    PullRequest {
        number: pr.number,
        html_url: pr
            .html_url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        base_ref: pr.base.ref_field.clone(),
        head_ref: pr.head.ref_field.clone(),
        title: pr.title.as_deref().unwrap_or_default().to_string(),
        node_id: pr.node_id.clone(),
        is_draft: pr.draft.unwrap_or(false),
    }
}

#[async_trait]
impl PlatformService for GitHubService {
    async fn find_existing_pr(&self, head_branch: &str) -> Result<Option<PullRequest>> {
        let head = format!("{}:{}", &self.config.owner, head_branch);

        let prs = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .list()
            .head(head)
            .state(octocrab::params::State::Open)
            .send()
            .await?;

        Ok(prs.items.first().map(pr_from_octocrab))
    }

    async fn create_pr_with_options(
        &self,
        head: &str,
        base: &str,
        title: &str,
        draft: bool,
    ) -> Result<PullRequest> {
        let pr = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .create(title, head, base)
            .draft(draft)
            .send()
            .await?;

        Ok(pr_from_octocrab(&pr))
    }

    async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<PullRequest> {
        let pr = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .update(pr_number)
            .base(new_base)
            .send()
            .await?;

        Ok(pr_from_octocrab(&pr))
    }

    async fn publish_pr(&self, pr_number: u64) -> Result<PullRequest> {
        // Fetch PR to get node_id for GraphQL mutation
        let pr = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .get(pr_number)
            .await?;

        let node_id = pr.node_id.as_ref().ok_or_else(|| {
            Error::GitHubApi("PR missing node_id for GraphQL mutation".to_string())
        })?;

        // Execute GraphQL mutation to mark PR as ready for review
        let response: serde_json::Value = self
            .client
            .graphql(&serde_json::json!({
                "query": r"
                    mutation MarkPullRequestReadyForReview($pullRequestId: ID!) {
                        markPullRequestReadyForReview(input: { pullRequestId: $pullRequestId }) {
                            pullRequest {
                                number
                                url
                                baseRefName
                                headRefName
                                title
                                id
                                isDraft
                            }
                        }
                    }
                ",
                "variables": {
                    "pullRequestId": node_id
                }
            }))
            .await
            .map_err(|e| Error::GitHubApi(format!("GraphQL mutation failed: {e}")))?;

        // Check for GraphQL errors
        if let Some(errors) = response.get("errors") {
            if errors.is_array() && !errors.as_array().unwrap().is_empty() {
                return Err(Error::GitHubApi(format!("GraphQL error: {errors}")));
            }
        }

        // Parse response
        let pr_data = response
            .get("data")
            .and_then(|d| d.get("markPullRequestReadyForReview"))
            .and_then(|m| m.get("pullRequest"))
            .ok_or_else(|| Error::GitHubApi("Invalid GraphQL response structure".to_string()))?;

        Ok(PullRequest {
            number: pr_data["number"].as_u64().unwrap_or(pr_number),
            html_url: pr_data["url"].as_str().unwrap_or_default().to_string(),
            base_ref: pr_data["baseRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            head_ref: pr_data["headRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            title: pr_data["title"].as_str().unwrap_or_default().to_string(),
            node_id: pr_data["id"].as_str().map(String::from),
            is_draft: pr_data["isDraft"].as_bool().unwrap_or(false),
        })
    }

    async fn list_pr_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        let comments = self
            .client
            .issues(&self.config.owner, &self.config.repo)
            .list_comments(pr_number)
            .send()
            .await?;

        Ok(comments
            .items
            .into_iter()
            .map(|c| PrComment {
                id: c.id.0,
                body: c.body.unwrap_or_default(),
            })
            .collect())
    }

    async fn create_pr_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        self.client
            .issues(&self.config.owner, &self.config.repo)
            .create_comment(pr_number, body)
            .await?;
        Ok(())
    }

    async fn update_pr_comment(&self, _pr_number: u64, comment_id: u64, body: &str) -> Result<()> {
        self.client
            .issues(&self.config.owner, &self.config.repo)
            .update_comment(octocrab::models::CommentId(comment_id), body)
            .await?;
        Ok(())
    }

    fn config(&self) -> &PlatformConfig {
        &self.config
    }
}
