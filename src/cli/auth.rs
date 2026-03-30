//! Auth command - test and manage authentication

use crate::cli::style::{Stylize, check, spinner_style};
use anstream::println;
use indicatif::ProgressBar;
use jj_ryu::auth::{
    get_gitea_auth, get_github_auth, get_gitlab_auth, test_gitea_auth, test_github_auth,
    test_gitlab_auth,
};
use jj_ryu::error::Result;
use jj_ryu::types::Platform;
use std::time::Duration;

/// Run the auth test command
pub async fn run_auth_test(platform: Platform) -> Result<()> {
    match platform {
        Platform::GitHub => {
            let spinner = ProgressBar::new_spinner();
            spinner.set_style(spinner_style());
            spinner.set_message("Testing GitHub authentication...");
            spinner.enable_steady_tick(Duration::from_millis(80));

            let config = get_github_auth().await?;
            let username = test_github_auth(&config).await?;

            spinner.finish_and_clear();
            println!("{} Authenticated as: {}", check(), username.accent());
            println!("  {} {:?}", "Token source:".muted(), config.source);
        }
        Platform::GitLab => {
            let spinner = ProgressBar::new_spinner();
            spinner.set_style(spinner_style());
            spinner.set_message("Testing GitLab authentication...");
            spinner.enable_steady_tick(Duration::from_millis(80));

            let config = get_gitlab_auth(None).await?;
            let username = test_gitlab_auth(&config).await?;

            spinner.finish_and_clear();
            println!("{} Authenticated as: {}", check(), username.accent());
            println!("  {} {:?}", "Token source:".muted(), config.source);
            println!("  {} {}", "Host:".muted(), config.host);
        }
        Platform::Gitea => {
            let spinner = ProgressBar::new_spinner();
            spinner.set_style(spinner_style());
            spinner.set_message("Testing Gitea authentication...");
            spinner.enable_steady_tick(Duration::from_millis(80));

            let config = get_gitea_auth(None).await?;
            let username = test_gitea_auth(&config).await?;

            spinner.finish_and_clear();
            println!("{} Authenticated as: {}", check(), username.accent());
            println!("  {} {:?}", "Token source:".muted(), config.source);
            println!("  {} {}", "Host:".muted(), config.host);
        }
    }
    Ok(())
}

/// Run the auth setup command (show instructions)
pub fn run_auth_setup(platform: Platform) {
    match platform {
        Platform::GitHub => {
            println!("{}", "GitHub Authentication Setup".emphasis());
            println!();
            println!("{}", "Option 1: GitHub CLI (recommended)".emphasis());
            println!("  Install: {}", "https://cli.github.com/".accent());
            println!("  Run: {}", "gh auth login".accent());
            println!();
            println!("{}", "Option 2: Environment variable".emphasis());
            println!(
                "  Set {} or {}",
                "GITHUB_TOKEN".accent(),
                "GH_TOKEN".accent()
            );
            println!();
            println!("{}", "For GitHub Enterprise:".muted());
            println!("  {}", "Set GH_HOST to your instance hostname".muted());
        }
        Platform::GitLab => {
            println!("{}", "GitLab Authentication Setup".emphasis());
            println!();
            println!("{}", "Option 1: GitLab CLI (glab)".emphasis());
            println!(
                "  Install: {}",
                "https://gitlab.com/gitlab-org/cli".accent()
            );
            println!("  Run: {}", "glab auth login".accent());
            println!();
            println!("{}", "Option 2: Environment variable".emphasis());
            println!(
                "  Set {} or {}",
                "GITLAB_TOKEN".accent(),
                "GL_TOKEN".accent()
            );
            println!();
            println!("{}", "For self-hosted GitLab:".muted());
            println!("  {}", "Set GITLAB_HOST to your instance hostname".muted());
        }
        Platform::Gitea => {
            println!("{}", "Gitea Authentication Setup".emphasis());
            println!();
            println!("{}", "Option 1: tea CLI".emphasis());
            println!("  Install: {}", "https://gitea.com/gitea/tea".accent());
            println!("  Run: {}", "tea login add".accent());
            println!(
                "  {}",
                "ryu can read the host from your default tea login".muted()
            );
            println!();
            println!("{}", "Option 2: Environment variable".emphasis());
            println!("  Set one of:");
            println!("    {}", "GITEA_TOKEN".accent());
            println!("    {}", "GITEA_ACCESS_TOKEN".accent());
            println!("    {}", "GITEA_KEY".accent());
            println!("    {}", "GT_TOKEN".accent());
            println!();
            println!("{}", "Gitea requires your instance hostname.".muted());
            println!(
                "  {}",
                "Set GITEA_HOST, or configure a default tea login".muted()
            );
        }
    }
}

/// Wrapper for auth commands
pub async fn run_auth(platform: Platform, action: &str) -> Result<()> {
    match action {
        "test" => run_auth_test(platform).await,
        "setup" => {
            run_auth_setup(platform);
            Ok(())
        }
        _ => {
            println!(
                "{}",
                format!("Unknown action: {action}. Use 'test' or 'setup'.").muted()
            );
            Ok(())
        }
    }
}
