//! Auth command - test and manage authentication

use crate::cli::style::{check, spinner_style, Stylize};
use anstream::println;
use indicatif::ProgressBar;
use jj_ryu::auth::{
    get_azure_devops_auth, get_github_auth, get_gitlab_auth, test_azure_devops_auth,
    test_github_auth, test_gitlab_auth,
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
        Platform::AzureDevOps => {
            let spinner = ProgressBar::new_spinner();
            spinner.set_style(spinner_style());
            spinner.set_message("Testing Azure DevOps authentication...");
            spinner.enable_steady_tick(Duration::from_millis(80));

            let config = get_azure_devops_auth(None).await?;
            let username = test_azure_devops_auth(&config).await?;

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
        Platform::AzureDevOps => {
            println!("{}", "Azure DevOps Authentication Setup".emphasis());
            println!();
            println!("{}", "Recommended: Personal Access Token (PAT)".emphasis());
            println!();
            println!("{}", "Step 1: Create a PAT".muted());
            println!("  1. Go to: {}", "https://dev.azure.com/{your-org}/_usersSettings/tokens".accent());
            println!("  2. Click 'New Token'");
            println!("  3. Set name: {}", "jj-ryu".accent());
            println!("  4. Select scopes:");
            println!("     - {} (Read & Write)", "Code".emphasis());
            println!("     - {} (Read & Write)", "Pull Requests".emphasis());
            println!("  5. Click 'Create' and copy the token");
            println!();
            println!("{}", "Step 2: Set environment variables".muted());
            println!("  export {}=<your-token>", "AZURE_DEVOPS_PAT".accent());
            println!("  export {}=<your-org>  # Optional but recommended", "AZURE_DEVOPS_ORGANIZATION".accent());
            println!();
            println!("{}", "Example:".muted());
            println!("  export AZURE_DEVOPS_PAT=abc123...");
            println!("  export AZURE_DEVOPS_ORGANIZATION=MyCompany");
            println!();
            println!("{}", "Alternative environment variables:".muted());
            println!("  {} - Personal Access Token (recommended)", "AZURE_DEVOPS_PAT".muted());
            println!("  {} - Alternative name", "AZURE_DEVOPS_TOKEN".muted());
            println!("  {} - For custom validation", "AZURE_DEVOPS_ORGANIZATION".muted());
            println!();
            println!("{}", "Note: Azure CLI (az devops) is supported but not required".muted());
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
