//! Regression tests for proxy routing in isolated processes.

use jj_ryu::auth::{
    AuthSource, GitHubAuthConfig, GitLabAuthConfig, test_github_auth, test_gitlab_auth,
};
use jj_ryu::platform::{GitHubService, GitLabService, PlatformService};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::time::timeout;

#[tokio::test]
#[ignore = "Invoked in a subprocess with isolated proxy environment variables"]
async fn proxy_child() {
    let mode = std::env::var("JJ_RYU_PROXY_TEST").expect("missing proxy test mode");
    let host = std::env::var("JJ_RYU_PROXY_HOST").expect("missing proxy test host");
    match mode.as_str() {
        "github-auth" | "github-auth-public" | "github-auth-public-host" => {
            let config = GitHubAuthConfig {
                token: "proxy-test-token".to_string(),
                source: AuthSource::EnvVar,
            };
            assert!(test_github_auth(&config).await.is_err());
        }
        "gitlab-auth" => {
            let config = GitLabAuthConfig {
                token: "proxy-test-token".to_string(),
                source: AuthSource::EnvVar,
                host,
            };
            assert!(test_gitlab_auth(&config).await.is_err());
        }
        "gitlab-service" => {
            let service = GitLabService::new(
                "proxy-test-token".to_string(),
                "owner".to_string(),
                "repo".to_string(),
                Some(host),
            )
            .unwrap();
            assert!(service.find_existing_pr("feature").await.is_err());
        }
        "github-service" => {
            let service = GitHubService::new(
                "proxy-test-token",
                "owner".to_string(),
                "repo".to_string(),
                Some(host),
            )
            .unwrap();
            assert!(service.find_existing_pr("feature").await.is_err());
        }
        _ => panic!("unknown proxy test mode: {mode}"),
    }
}

async fn check_proxy(variable: &str, mode: &str, bypass: bool) {
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let host = if bypass {
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        target.local_addr().unwrap().to_string()
    } else {
        "github.proxy-test.invalid".to_string()
    };
    // Isolate proxy variables without mutating the test runner's environment.
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "proxy_child", "--ignored", "--nocapture"]);
    command.env_remove("GH_HOST");
    if mode == "github-auth" {
        command.env("GH_HOST", format!("https://{host}/"));
    } else if mode == "github-auth-public-host" {
        command.env("GH_HOST", "https://github.com/");
    }
    for name in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        command.env_remove(name);
    }
    let mut child = command
        .env(variable, proxy_url)
        .env("NO_PROXY", if bypass { "127.0.0.1" } else { "" })
        .env("JJ_RYU_PROXY_TEST", mode)
        .env("JJ_RYU_PROXY_HOST", &host)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    if bypass {
        let status = timeout(Duration::from_secs(15), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert!(
            timeout(Duration::from_millis(100), proxy.accept())
                .await
                .is_err()
        );
    } else {
        let (mut stream, _) = timeout(Duration::from_secs(15), proxy.accept())
            .await
            .unwrap()
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let byte = timeout(Duration::from_secs(5), stream.read_u8())
                .await
                .unwrap()
                .unwrap();
            request.push(byte);
            assert!(request.len() < 8192);
        }
        let request = String::from_utf8(request).unwrap();
        let authority = if matches!(mode, "github-auth-public" | "github-auth-public-host") {
            "api.github.com"
        } else {
            &host
        };
        assert!(
            request.starts_with(&format!("CONNECT {authority}:443 HTTP/1.1\r\n")),
            "{request}"
        );
        assert!(!request.contains("proxy-test-token"));
        stream
            .write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        drop(stream);
        let output = timeout(Duration::from_secs(15), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[tokio::test]
async fn github_service_honors_https_proxy() {
    check_proxy("HTTPS_PROXY", "github-service", false).await;
}

#[tokio::test]
async fn github_auth_honors_https_proxy() {
    check_proxy("HTTPS_PROXY", "github-auth", false).await;
}

#[tokio::test]
async fn github_auth_defaults_to_public_github() {
    check_proxy("HTTPS_PROXY", "github-auth-public", false).await;
}

#[tokio::test]
async fn github_auth_honors_public_gh_host() {
    check_proxy("HTTPS_PROXY", "github-auth-public-host", false).await;
}

#[tokio::test]
async fn github_auth_honors_lowercase_https_proxy() {
    check_proxy("https_proxy", "github-auth", false).await;
}

#[tokio::test]
async fn github_auth_honors_no_proxy() {
    check_proxy("HTTPS_PROXY", "github-auth", true).await;
}

#[tokio::test]
async fn github_service_honors_lowercase_https_proxy() {
    check_proxy("https_proxy", "github-service", false).await;
}

#[tokio::test]
async fn github_service_honors_no_proxy() {
    check_proxy("HTTPS_PROXY", "github-service", true).await;
}

#[tokio::test]
async fn gitlab_service_honors_https_proxy() {
    check_proxy("HTTPS_PROXY", "gitlab-service", false).await;
}

#[tokio::test]
async fn gitlab_auth_honors_https_proxy() {
    check_proxy("HTTPS_PROXY", "gitlab-auth", false).await;
}

#[tokio::test]
async fn gitlab_service_honors_lowercase_https_proxy() {
    check_proxy("https_proxy", "gitlab-service", false).await;
}

#[tokio::test]
async fn gitlab_service_honors_no_proxy() {
    check_proxy("HTTPS_PROXY", "gitlab-service", true).await;
}
