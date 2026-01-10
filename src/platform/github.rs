//! GitHub platform service implementation

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::types::{Platform, PlatformConfig, PrComment, PullRequest};
use async_trait::async_trait;
use octocrab::Octocrab;
use serde::Deserialize;
use tracing::debug;

// GraphQL response types for publish_pr mutation

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkReadyForReviewData {
    mark_pull_request_ready_for_review: MarkReadyPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkReadyPayload {
    pull_request: GraphQlPullRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlPullRequest {
    number: u64,
    url: String,
    base_ref_name: String,
    head_ref_name: String,
    title: String,
    id: String,
    is_draft: bool,
}

impl From<GraphQlPullRequest> for PullRequest {
    fn from(pr: GraphQlPullRequest) -> Self {
        Self {
            number: pr.number,
            html_url: pr.url,
            base_ref: pr.base_ref_name,
            head_ref: pr.head_ref_name,
            title: pr.title,
            node_id: Some(pr.id),
            is_draft: pr.is_draft,
        }
    }
}

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
        debug!(head_branch, "finding existing PR");
        let head = format!("{}:{}", &self.config.owner, head_branch);

        let prs = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .list()
            .head(head)
            .state(octocrab::params::State::Open)
            .send()
            .await?;

        let result = prs.items.first().map(pr_from_octocrab);
        if let Some(ref pr) = result {
            debug!(pr_number = pr.number, "found existing PR");
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
        let pr = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .create(title, head, base)
            .draft(draft)
            .send()
            .await?;

        let result = pr_from_octocrab(&pr);
        debug!(pr_number = result.number, "created PR");
        Ok(result)
    }

    async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<PullRequest> {
        debug!(pr_number, new_base, "updating PR base");
        let pr = self
            .client
            .pulls(&self.config.owner, &self.config.repo)
            .update(pr_number)
            .base(new_base)
            .send()
            .await?;

        debug!(pr_number, "updated PR base");
        Ok(pr_from_octocrab(&pr))
    }

    async fn publish_pr(&self, pr_number: u64) -> Result<PullRequest> {
        debug!(pr_number, "publishing PR");
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
        let response: GraphQlResponse<MarkReadyForReviewData> = self
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
        if let Some(errors) = response.errors
            && !errors.is_empty()
        {
            let messages: Vec<_> = errors.into_iter().map(|e| e.message).collect();
            return Err(Error::GitHubApi(format!(
                "GraphQL error: {}",
                messages.join(", ")
            )));
        }

        // Extract typed response
        let data = response
            .data
            .ok_or_else(|| Error::GitHubApi("No data in GraphQL response".to_string()))?;

        debug!(pr_number, "published PR");
        Ok(data.mark_pull_request_ready_for_review.pull_request.into())
    }

    async fn list_pr_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        debug!(pr_number, "listing PR comments");
        let comments = self
            .client
            .issues(&self.config.owner, &self.config.repo)
            .list_comments(pr_number)
            .send()
            .await?;

        let result: Vec<PrComment> = comments
            .items
            .into_iter()
            .map(|c| PrComment {
                id: c.id.0,
                body: c.body.unwrap_or_default(),
            })
            .collect();
        debug!(pr_number, count = result.len(), "listed PR comments");
        Ok(result)
    }

    async fn create_pr_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        debug!(pr_number, "creating PR comment");
        self.client
            .issues(&self.config.owner, &self.config.repo)
            .create_comment(pr_number, body)
            .await?;
        debug!(pr_number, "created PR comment");
        Ok(())
    }

    async fn update_pr_comment(&self, _pr_number: u64, comment_id: u64, body: &str) -> Result<()> {
        debug!(comment_id, "updating PR comment");
        self.client
            .issues(&self.config.owner, &self.config.repo)
            .update_comment(octocrab::models::CommentId(comment_id), body)
            .await?;
        debug!(comment_id, "updated PR comment");
        Ok(())
    }

    fn config(&self) -> &PlatformConfig {
        &self.config
    }
}
