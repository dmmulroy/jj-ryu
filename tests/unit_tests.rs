//! Unit tests for jj-ryu modules

mod common;

mod analysis_test {
    use crate::common::{make_linear_stack, make_multi_bookmark_segment};
    use jj_ryu::error::Error;
    use jj_ryu::submit::{
        analyze_submission, generate_pr_title, get_base_branch, select_bookmark_for_segment,
    };

    #[test]
    fn test_analyze_middle_of_stack() {
        // Stack: a -> b -> c, target b
        // Should return [a, b] not [a, b, c]
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let result = analyze_submission(&graph, Some("feat-b")).unwrap();

        assert_eq!(result.target_bookmark, "feat-b");
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[0].bookmark.name, "feat-a");
        assert_eq!(result.segments[1].bookmark.name, "feat-b");
    }

    #[test]
    fn test_analyze_root_of_stack() {
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let result = analyze_submission(&graph, Some("feat-a")).unwrap();

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.segments[0].bookmark.name, "feat-a");
    }

    #[test]
    fn test_analyze_leaf_of_stack() {
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let result = analyze_submission(&graph, Some("feat-c")).unwrap();

        assert_eq!(result.segments.len(), 3);
        assert_eq!(result.segments[2].bookmark.name, "feat-c");
    }

    #[test]
    fn test_get_base_branch_three_level_stack() {
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let analysis = analyze_submission(&graph, Some("feat-c")).unwrap();

        assert_eq!(
            get_base_branch("feat-a", &analysis.segments, "main").unwrap(),
            "main"
        );
        assert_eq!(
            get_base_branch("feat-b", &analysis.segments, "main").unwrap(),
            "feat-a"
        );
        assert_eq!(
            get_base_branch("feat-c", &analysis.segments, "main").unwrap(),
            "feat-b"
        );
    }

    #[test]
    fn test_generate_pr_title_uses_root_commit_description() {
        // Fixture creates description "Commit for {name}"
        let graph = make_linear_stack(&["feat-a"]);
        let analysis = analyze_submission(&graph, Some("feat-a")).unwrap();

        let title = generate_pr_title("feat-a", &analysis.segments).unwrap();
        // Should use the actual commit description, not just the bookmark name
        assert_eq!(title, "Commit for feat-a");
    }

    #[test]
    fn test_analyze_nonexistent_bookmark_error_type() {
        let graph = make_linear_stack(&["feat-a", "feat-b"]);
        let result = analyze_submission(&graph, Some("nonexistent"));

        // Verify we get the correct error type with the bookmark name
        match result {
            Err(Error::BookmarkNotFound(name)) => assert_eq!(name, "nonexistent"),
            other => panic!("Expected BookmarkNotFound error, got: {other:?}"),
        }
    }

    // === Multi-bookmark tests ===

    #[test]
    fn test_analyze_multi_bookmark_segment_selects_target() {
        // Two bookmarks pointing to the same commit
        let graph = make_multi_bookmark_segment(&["feat-a", "feat-b"]);
        let analysis = analyze_submission(&graph, Some("feat-b")).unwrap();

        // Should select the target bookmark
        assert_eq!(analysis.segments.len(), 1);
        assert_eq!(analysis.segments[0].bookmark.name, "feat-b");
    }

    #[test]
    fn test_select_bookmark_prefers_shorter_name() {
        let graph = make_multi_bookmark_segment(&["feature-auth", "auth"]);
        // Don't specify target - should prefer shorter name
        let segment = &graph.stack.as_ref().unwrap().segments[0];
        let selected = select_bookmark_for_segment(segment, None);
        assert_eq!(selected.name, "auth");
    }

    #[test]
    fn test_select_bookmark_filters_temporary() {
        let graph = make_multi_bookmark_segment(&["wip-feature", "feature"]);
        let segment = &graph.stack.as_ref().unwrap().segments[0];
        let selected = select_bookmark_for_segment(segment, None);
        // Should filter out "wip-feature" and select "feature"
        assert_eq!(selected.name, "feature");
    }

    #[test]
    fn test_select_bookmark_filters_temp_suffix() {
        let graph = make_multi_bookmark_segment(&["auth-old", "auth"]);
        let segment = &graph.stack.as_ref().unwrap().segments[0];
        let selected = select_bookmark_for_segment(segment, None);
        assert_eq!(selected.name, "auth");
    }

    #[test]
    fn test_select_bookmark_all_temporary_uses_shortest() {
        // When all bookmarks are temporary, still picks shortest
        let graph = make_multi_bookmark_segment(&["wip-auth-feature", "tmp-auth"]);
        let segment = &graph.stack.as_ref().unwrap().segments[0];
        let selected = select_bookmark_for_segment(segment, None);
        // Falls back to shortest
        assert_eq!(selected.name, "tmp-auth");
    }

    #[test]
    fn test_select_bookmark_alphabetical_tiebreaker() {
        // Use equal-length names to test alphabetical tiebreaker
        let graph = make_multi_bookmark_segment(&["bbb", "aaa"]);
        let segment = &graph.stack.as_ref().unwrap().segments[0];
        let selected = select_bookmark_for_segment(segment, None);
        // Same length (3 chars each), alphabetically first wins
        assert_eq!(selected.name, "aaa");
    }

    #[test]
    fn test_select_bookmark_single_returns_it() {
        let graph = make_linear_stack(&["solo"]);
        let segment = &graph.stack.as_ref().unwrap().segments[0];
        let selected = select_bookmark_for_segment(segment, None);
        assert_eq!(selected.name, "solo");
    }

    // === Deep stack test ===

    #[test]
    fn test_analyze_10_level_deep_stack() {
        let names: Vec<String> = (0..10).map(|i| format!("feat-{i}")).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let graph = make_linear_stack(&name_refs);
        let analysis = analyze_submission(&graph, Some("feat-9")).unwrap();

        assert_eq!(analysis.segments.len(), 10);
        assert_eq!(analysis.segments[0].bookmark.name, "feat-0");
        assert_eq!(analysis.segments[9].bookmark.name, "feat-9");
    }
}

mod detection_test {
    use jj_ryu::error::Error;
    use jj_ryu::platform::{detect_platform, parse_repo_info};
    use jj_ryu::types::Platform;

    #[test]
    fn test_github_ssh_without_git_extension() {
        let config = parse_repo_info("git@github.com:owner/repo").unwrap();
        assert_eq!(config.platform, Platform::GitHub);
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_github_https_without_git_extension() {
        let config = parse_repo_info("https://github.com/owner/repo").unwrap();
        assert_eq!(config.platform, Platform::GitHub);
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_gitlab_deeply_nested_groups() {
        let config = parse_repo_info("https://gitlab.com/a/b/c/d/repo.git").unwrap();
        assert_eq!(config.platform, Platform::GitLab);
        assert_eq!(config.owner, "a/b/c/d");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_gitlab_ssh_nested_groups() {
        let config = parse_repo_info("git@gitlab.com:group/subgroup/repo.git").unwrap();
        assert_eq!(config.platform, Platform::GitLab);
        assert_eq!(config.owner, "group/subgroup");
        assert_eq!(config.repo, "repo");
    }

    // Note: GitHub Enterprise and GitLab self-hosted detection tests
    // are skipped here because they require modifying env vars, which
    // is unsafe in Rust 2024 edition and the project forbids unsafe code.
    // These are tested inline in src/platform/detection.rs

    #[test]
    fn test_unknown_platform_returns_none() {
        let platform = detect_platform("https://bitbucket.org/owner/repo.git");
        assert_eq!(platform, None);
    }

    #[test]
    fn test_parse_unknown_platform_returns_error() {
        let result = parse_repo_info("https://bitbucket.org/owner/repo.git");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_url_returns_no_supported_remotes() {
        // Invalid URLs that can't be parsed return NoSupportedRemotes
        let result = parse_repo_info("not-a-valid-url");
        match result {
            Err(Error::NoSupportedRemotes) => {} // Expected
            other => panic!("Expected NoSupportedRemotes error, got: {other:?}"),
        }
    }

    #[test]
    fn test_github_url_with_trailing_slash() {
        // Trailing slashes are stripped before parsing
        let config = parse_repo_info("https://github.com/owner/repo/").unwrap();
        assert_eq!(config.platform, Platform::GitHub);
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_github_url_with_multiple_trailing_slashes() {
        let config = parse_repo_info("https://github.com/owner/repo///").unwrap();
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_gitlab_single_level_group() {
        let config = parse_repo_info("https://gitlab.com/owner/repo.git").unwrap();
        assert_eq!(config.platform, Platform::GitLab);
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_github_with_git_extension() {
        let config = parse_repo_info("git@github.com:owner/repo.git").unwrap();
        assert_eq!(config.platform, Platform::GitHub);
        assert_eq!(config.repo, "repo"); // .git should be stripped
    }
}

mod plan_test {
    use crate::common::{MockPlatformService, github_config, make_linear_stack, make_pr};
    use jj_ryu::submit::{ExecutionStep, analyze_submission, create_submission_plan};

    #[tokio::test]
    async fn test_plan_new_stack_no_existing_prs() {
        let graph = make_linear_stack(&["feat-a", "feat-b"]);
        let analysis = analyze_submission(&graph, Some("feat-b")).unwrap();

        // Mock returns None for all find_existing_pr calls (default behavior)
        let mock = MockPlatformService::with_config(github_config());

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        assert_eq!(plan.count_creates(), 2);
        assert_eq!(plan.count_updates(), 0);

        // Find CreatePr steps and verify them
        let creates: Vec<_> = plan
            .execution_steps
            .iter()
            .filter_map(|s| match s {
                ExecutionStep::CreatePr(c) => Some(c),
                _ => None,
            })
            .collect();

        // First PR should target main
        assert_eq!(creates[0].bookmark.name, "feat-a");
        assert_eq!(creates[0].base_branch, "main");

        // Second PR should target first bookmark
        assert_eq!(creates[1].bookmark.name, "feat-b");
        assert_eq!(creates[1].base_branch, "feat-a");
    }

    #[tokio::test]
    async fn test_plan_update_existing_pr_base() {
        let graph = make_linear_stack(&["feat-a", "feat-b"]);
        let analysis = analyze_submission(&graph, Some("feat-b")).unwrap();

        let mock = MockPlatformService::with_config(github_config());
        // feat-a: no existing PR (default)
        // feat-b: existing PR with wrong base (main instead of feat-a)
        mock.set_find_pr_response("feat-b", Some(make_pr(123, "feat-b", "main")));

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        assert_eq!(plan.count_creates(), 1);
        assert_eq!(plan.count_updates(), 1);

        let update = plan
            .execution_steps
            .iter()
            .find_map(|s| match s {
                ExecutionStep::UpdateBase(u) => Some(u),
                _ => None,
            })
            .expect("should have update step");

        assert_eq!(update.bookmark.name, "feat-b");
        assert_eq!(update.current_base, "main");
        assert_eq!(update.expected_base, "feat-a");
    }

    #[tokio::test]
    async fn test_plan_all_prs_exist_correct_base() {
        let graph = make_linear_stack(&["feat-a", "feat-b"]);
        let analysis = analyze_submission(&graph, Some("feat-b")).unwrap();

        let mock = MockPlatformService::with_config(github_config());
        // Both PRs exist with correct bases
        mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
        mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "feat-a")));

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        // Nothing to create or update (only pushes if needed)
        assert_eq!(plan.count_creates(), 0);
        assert_eq!(plan.count_updates(), 0);

        // But we should have existing PRs tracked
        assert_eq!(plan.existing_prs.len(), 2);
    }

    #[tokio::test]
    async fn test_plan_synced_bookmark_not_in_push_list() {
        let mut graph = make_linear_stack(&["feat-a"]);
        // Mark bookmark as synced
        if let Some(bm) = graph.bookmarks.get_mut("feat-a") {
            bm.has_remote = true;
            bm.is_synced = true;
        }
        // Also update in stacks
        if let Some(segment) = graph.stack.as_mut().and_then(|s| s.segments.get_mut(0)) {
            if let Some(bm) = segment.bookmarks.get_mut(0) {
                bm.has_remote = true;
                bm.is_synced = true;
            }
        }

        let analysis = analyze_submission(&graph, Some("feat-a")).unwrap();
        let mock = MockPlatformService::with_config(github_config());

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        // Synced bookmark should not be in push list
        assert_eq!(plan.count_pushes(), 0);
    }

    #[tokio::test]
    async fn test_plan_unsynced_bookmark_in_push_list() {
        let graph = make_linear_stack(&["feat-a"]);
        // Default bookmarks from fixtures are not synced
        let analysis = analyze_submission(&graph, Some("feat-a")).unwrap();
        let mock = MockPlatformService::with_config(github_config());

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        assert_eq!(plan.count_pushes(), 1);

        let push = plan
            .execution_steps
            .iter()
            .find_map(|s| match s {
                ExecutionStep::Push(b) => Some(b),
                _ => None,
            })
            .expect("should have push step");

        assert_eq!(push.name, "feat-a");
    }

    // === Mock verification tests ===

    #[tokio::test]
    async fn test_plan_queries_platform_for_each_bookmark() {
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let analysis = analyze_submission(&graph, Some("feat-c")).unwrap();
        let mock = MockPlatformService::with_config(github_config());

        let _ = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        // Verify find_existing_pr was called for each bookmark
        mock.assert_find_pr_called_for(&["feat-a", "feat-b", "feat-c"]);
    }

    #[tokio::test]
    async fn test_plan_has_remote_true_but_not_synced_needs_push() {
        let mut graph = make_linear_stack(&["feat-a"]);
        // has_remote=true but is_synced=false (e.g., local changes after push)
        if let Some(bm) = graph.bookmarks.get_mut("feat-a") {
            bm.has_remote = true;
            bm.is_synced = false;
        }
        if let Some(segment) = graph.stack.as_mut().and_then(|s| s.segments.get_mut(0)) {
            if let Some(bm) = segment.bookmarks.get_mut(0) {
                bm.has_remote = true;
                bm.is_synced = false;
            }
        }

        let analysis = analyze_submission(&graph, Some("feat-a")).unwrap();
        let mock = MockPlatformService::with_config(github_config());

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        // Should still need push because is_synced=false
        assert_eq!(plan.count_pushes(), 1);
    }

    #[tokio::test]
    async fn test_plan_multiple_base_updates_needed() {
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let analysis = analyze_submission(&graph, Some("feat-c")).unwrap();

        let mock = MockPlatformService::with_config(github_config());
        // All PRs exist but with wrong bases (all pointing to main)
        mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
        mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "main"))); // Should be feat-a
        mock.set_find_pr_response("feat-c", Some(make_pr(3, "feat-c", "main"))); // Should be feat-b

        let plan = create_submission_plan(&analysis, &mock, "origin", "main")
            .await
            .unwrap();

        // feat-a is correct (base=main), feat-b and feat-c need updates
        assert_eq!(plan.count_creates(), 0);
        assert_eq!(plan.count_updates(), 2);

        let updates: Vec<_> = plan
            .execution_steps
            .iter()
            .filter_map(|s| match s {
                ExecutionStep::UpdateBase(u) => Some(u),
                _ => None,
            })
            .collect();

        assert_eq!(updates[0].bookmark.name, "feat-b");
        assert_eq!(updates[0].expected_base, "feat-a");
        assert_eq!(updates[1].bookmark.name, "feat-c");
        assert_eq!(updates[1].expected_base, "feat-b");
    }

    // === Error handling tests ===

    #[tokio::test]
    async fn test_plan_handles_find_pr_error() {
        let graph = make_linear_stack(&["feat-a", "feat-b"]);
        let analysis = analyze_submission(&graph, Some("feat-b")).unwrap();

        let mock = MockPlatformService::with_config(github_config());
        mock.fail_find_pr("rate limited");

        let result = create_submission_plan(&analysis, &mock, "origin", "main").await;

        assert!(result.is_err(), "Expected error when find_pr fails");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("rate limited"),
            "Error should contain original message: {err}"
        );
    }

    #[tokio::test]
    async fn test_plan_error_is_platform_type() {
        use jj_ryu::error::Error;

        let graph = make_linear_stack(&["feat-a"]);
        let analysis = analyze_submission(&graph, Some("feat-a")).unwrap();

        let mock = MockPlatformService::with_config(github_config());
        mock.fail_find_pr("API unavailable");

        let result = create_submission_plan(&analysis, &mock, "origin", "main").await;

        match result {
            Err(Error::Platform(msg)) => {
                assert_eq!(msg, "API unavailable");
            }
            other => panic!("Expected Platform error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_plan_fails_fast_on_first_error() {
        let graph = make_linear_stack(&["feat-a", "feat-b", "feat-c"]);
        let analysis = analyze_submission(&graph, Some("feat-c")).unwrap();

        let mock = MockPlatformService::with_config(github_config());
        mock.fail_find_pr("connection failed");

        let result = create_submission_plan(&analysis, &mock, "origin", "main").await;

        assert!(result.is_err());
        // Should have attempted at least one call before failing
        let calls = mock.get_find_pr_calls();
        assert!(!calls.is_empty(), "Should have made at least one API call");
        // But should not have completed all calls (fail fast)
        assert!(
            calls.len() <= 3,
            "Should fail fast, not retry all bookmarks"
        );
    }
}

mod stack_comment_test {
    use jj_ryu::submit::{
        COMMENT_DATA_PREFIX, STACK_COMMENT_THIS_PR, StackCommentData, StackItem, SubmissionPlan,
        build_stack_comment_data, format_stack_comment,
    };
    use jj_ryu::types::{Bookmark, NarrowedBookmarkSegment, PullRequest};
    use std::collections::HashMap;

    fn make_bookmark(name: &str) -> Bookmark {
        Bookmark {
            name: name.to_string(),
            commit_id: format!("{name}_commit"),
            change_id: format!("{name}_change"),
            has_remote: false,
            is_synced: false,
        }
    }

    fn make_pr(number: u64, bookmark: &str) -> PullRequest {
        PullRequest {
            number,
            html_url: format!("https://github.com/test/test/pull/{number}"),
            base_ref: "main".to_string(),
            head_ref: bookmark.to_string(),
            title: format!("PR for {bookmark}"),
            node_id: Some(format!("PR_node_{number}")),
            is_draft: false,
        }
    }

    fn make_stack_item(name: &str, number: u64) -> StackItem {
        StackItem {
            bookmark_name: name.to_string(),
            pr_url: format!("https://github.com/test/test/pull/{number}"),
            pr_number: number,
            pr_title: format!("feat: {name}"),
        }
    }

    #[test]
    fn test_build_stack_comment_data_single_pr() {
        let plan = SubmissionPlan {
            segments: vec![NarrowedBookmarkSegment {
                bookmark: make_bookmark("feat-a"),
                changes: vec![],
            }],
            constraints: vec![],
            execution_steps: vec![],
            existing_prs: HashMap::new(),
            remote: "origin".to_string(),
            default_branch: "main".to_string(),
        };

        let mut bookmark_to_pr = HashMap::new();
        bookmark_to_pr.insert("feat-a".to_string(), make_pr(1, "feat-a"));

        let data = build_stack_comment_data(&plan, &bookmark_to_pr);

        assert_eq!(data.version, 1);
        assert_eq!(data.base_branch, "main");
        assert_eq!(data.stack.len(), 1);
        assert_eq!(data.stack[0].bookmark_name, "feat-a");
        assert_eq!(data.stack[0].pr_number, 1);
    }

    #[test]
    fn test_build_stack_comment_data_three_pr_stack() {
        let plan = SubmissionPlan {
            segments: vec![
                NarrowedBookmarkSegment {
                    bookmark: make_bookmark("feat-a"),
                    changes: vec![],
                },
                NarrowedBookmarkSegment {
                    bookmark: make_bookmark("feat-b"),
                    changes: vec![],
                },
                NarrowedBookmarkSegment {
                    bookmark: make_bookmark("feat-c"),
                    changes: vec![],
                },
            ],
            constraints: vec![],
            execution_steps: vec![],
            existing_prs: HashMap::new(),
            remote: "origin".to_string(),
            default_branch: "main".to_string(),
        };

        let mut bookmark_to_pr = HashMap::new();
        bookmark_to_pr.insert("feat-a".to_string(), make_pr(1, "feat-a"));
        bookmark_to_pr.insert("feat-b".to_string(), make_pr(2, "feat-b"));
        bookmark_to_pr.insert("feat-c".to_string(), make_pr(3, "feat-c"));

        let data = build_stack_comment_data(&plan, &bookmark_to_pr);

        assert_eq!(data.stack.len(), 3);
        assert_eq!(data.stack[0].pr_number, 1);
        assert_eq!(data.stack[1].pr_number, 2);
        assert_eq!(data.stack[2].pr_number, 3);
    }

    #[test]
    fn test_format_body_marks_current_pr() {
        let data = StackCommentData {
            version: 1,
            stack: vec![make_stack_item("feat-a", 1), make_stack_item("feat-b", 2)],
            base_branch: "main".to_string(),
        };

        // Format for second PR (index 1)
        let body = format_stack_comment(&data, 1).unwrap();

        // PR #2 should have the marker
        assert!(
            body.contains(&format!("#{} {STACK_COMMENT_THIS_PR}", 2)),
            "body should mark PR #2 as current: {body}"
        );

        // PR #1 should NOT have the marker
        assert!(
            !body.contains(&format!("#{} {STACK_COMMENT_THIS_PR}", 1)),
            "body should NOT mark PR #1 as current: {body}"
        );
    }

    #[test]
    fn test_format_body_reverse_order() {
        let data = StackCommentData {
            version: 1,
            stack: vec![
                make_stack_item("feat-a", 1),
                make_stack_item("feat-b", 2),
                make_stack_item("feat-c", 3),
            ],
            base_branch: "main".to_string(),
        };

        let body = format_stack_comment(&data, 0).unwrap();

        // Find positions of each PR in the body
        let pos_1 = body.find("#1").expect("should contain #1");
        let pos_2 = body.find("#2").expect("should contain #2");
        let pos_3 = body.find("#3").expect("should contain #3");

        // Reverse order means #3 (leaf) comes first, #1 (root) comes last
        assert!(pos_3 < pos_2, "PR #3 should appear before #2");
        assert!(pos_2 < pos_1, "PR #2 should appear before #1");
    }

    #[test]
    fn test_format_body_contains_marker() {
        let data = StackCommentData {
            version: 1,
            stack: vec![make_stack_item("feat-a", 1)],
            base_branch: "main".to_string(),
        };

        let body = format_stack_comment(&data, 0).unwrap();

        assert!(
            body.contains(COMMENT_DATA_PREFIX),
            "body should contain data prefix"
        );
    }

    #[test]
    fn test_format_body_contains_base_branch() {
        let data = StackCommentData {
            version: 1,
            stack: vec![make_stack_item("feat-a", 1)],
            base_branch: "develop".to_string(),
        };

        let body = format_stack_comment(&data, 0).unwrap();

        assert!(
            body.contains("`develop`"),
            "body should contain base branch: {body}"
        );
    }

    #[test]
    fn test_format_body_contains_pr_title() {
        let data = StackCommentData {
            version: 1,
            stack: vec![make_stack_item("feat-a", 1)],
            base_branch: "main".to_string(),
        };

        let body = format_stack_comment(&data, 0).unwrap();

        assert!(
            body.contains("feat: feat-a"),
            "body should contain PR title: {body}"
        );
    }
}

mod sync_test {
    use jj_ryu::error::Error;
    use jj_ryu::repo::select_remote;
    use jj_ryu::types::GitRemote;

    fn make_remote(name: &str) -> GitRemote {
        GitRemote {
            name: name.to_string(),
            url: format!("https://github.com/test/{name}.git"),
        }
    }

    #[test]
    fn test_select_remote_single_remote() {
        let remotes = vec![make_remote("upstream")];
        let result = select_remote(&remotes, None).unwrap();
        assert_eq!(result, "upstream");
    }

    #[test]
    fn test_select_remote_prefers_origin() {
        let remotes = vec![
            make_remote("upstream"),
            make_remote("origin"),
            make_remote("fork"),
        ];
        let result = select_remote(&remotes, None).unwrap();
        assert_eq!(result, "origin");
    }

    #[test]
    fn test_select_remote_no_origin_uses_first() {
        let remotes = vec![make_remote("upstream"), make_remote("fork")];
        let result = select_remote(&remotes, None).unwrap();
        assert_eq!(result, "upstream");
    }

    #[test]
    fn test_select_remote_specified_exists() {
        let remotes = vec![make_remote("origin"), make_remote("fork")];
        let result = select_remote(&remotes, Some("fork")).unwrap();
        assert_eq!(result, "fork");
    }

    #[test]
    fn test_select_remote_specified_not_found() {
        let remotes = vec![make_remote("origin")];
        let result = select_remote(&remotes, Some("nonexistent"));
        match result {
            Err(Error::RemoteNotFound(name)) => assert_eq!(name, "nonexistent"),
            other => panic!("Expected RemoteNotFound error, got: {other:?}"),
        }
    }

    #[test]
    fn test_select_remote_none_available() {
        let remotes: Vec<GitRemote> = vec![];
        let result = select_remote(&remotes, None);
        match result {
            Err(Error::NoSupportedRemotes) => {}
            other => panic!("Expected NoSupportedRemotes error, got: {other:?}"),
        }
    }
}
