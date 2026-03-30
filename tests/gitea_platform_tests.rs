//! Gitea platform service tests.

use jj_ryu::platform::{GiteaService, PlatformService};
use mockito::{Matcher, Server};
use serde_json::json;

fn make_service(server: &Server) -> GiteaService {
    GiteaService::new(
        "test-token".to_string(),
        "org".to_string(),
        "repo".to_string(),
        Some(server.url()),
    )
    .expect("create gitea service")
}

fn pr_response(number: u64, title: &str, head: &str, base: &str, draft: bool) -> serde_json::Value {
    json!({
        "number": number,
        "html_url": format!("https://gitea.example.local/org/repo/pulls/{number}"),
        "title": title,
        "draft": draft,
        "head": {
            "ref": head,
            "label": format!("org:{head}")
        },
        "base": {
            "ref": base
        }
    })
}

#[tokio::test]
async fn test_find_existing_pr_filters_open_pulls_by_head_ref() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/v1/repos/org/repo/pulls")
        .match_header("authorization", "token test-token")
        .match_query(Matcher::UrlEncoded("state".into(), "open".into()))
        .with_status(200)
        .with_body(
            json!([
                pr_response(11, "Other", "other-branch", "main", false),
                pr_response(12, "Feature", "feature-branch", "main", false)
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let service = make_service(&server);
    let pr = service
        .find_existing_pr("feature-branch")
        .await
        .expect("find existing pr")
        .expect("matching pr");

    assert_eq!(pr.number, 12);
    assert_eq!(pr.head_ref, "feature-branch");
    assert_eq!(pr.base_ref, "main");
}

#[tokio::test]
async fn test_create_pr_with_options_posts_gitea_payload() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("POST", "/api/v1/repos/org/repo/pulls")
        .match_header("authorization", "token test-token")
        .match_body(Matcher::Json(json!({
            "title": "Add feature",
            "head": "feature-branch",
            "base": "main",
            "draft": true
        })))
        .with_status(200)
        .with_body(pr_response(21, "Add feature", "feature-branch", "main", true).to_string())
        .create_async()
        .await;

    let service = make_service(&server);
    let pr = service
        .create_pr_with_options("feature-branch", "main", "Add feature", true)
        .await
        .expect("create pr");

    assert_eq!(pr.number, 21);
    assert!(pr.is_draft);
}

#[tokio::test]
async fn test_update_pr_base_patches_pull_request() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("PATCH", "/api/v1/repos/org/repo/pulls/21")
        .match_header("authorization", "token test-token")
        .match_body(Matcher::Json(json!({
            "base": "feat-a"
        })))
        .with_status(200)
        .with_body(pr_response(21, "Add feature", "feature-branch", "feat-a", false).to_string())
        .create_async()
        .await;

    let service = make_service(&server);
    let pr = service
        .update_pr_base(21, "feat-a")
        .await
        .expect("update pr base");

    assert_eq!(pr.base_ref, "feat-a");
}

#[tokio::test]
async fn test_list_pr_comments_uses_issue_comments_endpoint() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/v1/repos/org/repo/issues/21/comments")
        .match_header("authorization", "token test-token")
        .with_status(200)
        .with_body(json!([{ "id": 7, "body": "stack comment" }]).to_string())
        .create_async()
        .await;

    let service = make_service(&server);
    let comments = service.list_pr_comments(21).await.expect("list comments");

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, 7);
    assert_eq!(comments[0].body, "stack comment");
}

#[tokio::test]
async fn test_create_pr_comment_posts_to_issue_comments_endpoint() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("POST", "/api/v1/repos/org/repo/issues/21/comments")
        .match_header("authorization", "token test-token")
        .match_body(Matcher::Json(json!({
            "body": "hello from jj-ryu"
        })))
        .with_status(201)
        .create_async()
        .await;

    let service = make_service(&server);
    service
        .create_pr_comment(21, "hello from jj-ryu")
        .await
        .expect("create comment");
}

#[tokio::test]
async fn test_update_pr_comment_patches_issue_comment() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("PATCH", "/api/v1/repos/org/repo/issues/comments/7")
        .match_header("authorization", "token test-token")
        .match_body(Matcher::Json(json!({
            "body": "updated stack comment"
        })))
        .with_status(200)
        .create_async()
        .await;

    let service = make_service(&server);
    service
        .update_pr_comment(21, 7, "updated stack comment")
        .await
        .expect("update comment");
}

#[tokio::test]
async fn test_publish_pr_removes_wip_prefix_from_title() {
    let mut server = Server::new_async().await;
    let _get_mock = server
        .mock("GET", "/api/v1/repos/org/repo/pulls/17")
        .match_header("authorization", "token test-token")
        .with_status(200)
        .with_body(pr_response(17, "WIP: Implement auth", "feat-auth", "main", false).to_string())
        .create_async()
        .await;
    let _patch_mock = server
        .mock("PATCH", "/api/v1/repos/org/repo/pulls/17")
        .match_header("authorization", "token test-token")
        .match_body(Matcher::Json(json!({
            "title": "Implement auth"
        })))
        .with_status(200)
        .with_body(pr_response(17, "Implement auth", "feat-auth", "main", false).to_string())
        .create_async()
        .await;

    let service = make_service(&server);
    let pr = service.publish_pr(17).await.expect("publish pr");

    assert_eq!(pr.title, "Implement auth");
}
