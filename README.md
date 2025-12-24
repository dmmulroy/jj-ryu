# jj-ryu

<img width="366" height="366" alt="image" src="https://github.com/user-attachments/assets/1691edfc-3b65-4f8d-b959-71ff21ff23e5" />

Stacked PRs for [Jujutsu](https://jj-vcs.github.io/jj/latest/). Push bookmark stacks to GitHub and GitLab as chained pull requests.

## What it does

```
       [feat-c]
    @  mzpwwxkq a1b2c3d4 Add logout       -->   PR #3: feat-c -> feat-b
    |
       [feat-b]
    o  yskvutnz e5f6a7b8 Add sessions     -->   PR #2: feat-b -> feat-a
    |
       [feat-a]
    o  kpqvunts 9d8c7b6a Add auth         -->   PR #1: feat-a -> main
    |
  trunk()
```

Each bookmark becomes a PR targeting the previous bookmark (or trunk). When you update your stack, `ryu` updates the PRs.

## Install

```sh
# npm (prebuilt binaries)
npm install -g jj-ryu

# cargo
cargo install jj-ryu
```

Binary name is `ryu`.

**macOS quarantine:** If you see "ryu can't be opened", run `xattr -d com.apple.quarantine $(which ryu)`

## Quick start

```sh
# View your bookmark stacks
ryu

# Submit a stack as PRs
ryu submit feat-c

# Sync all stacks with remote
ryu sync
```

## Authentication

### GitHub

Uses (in order):
1. `gh auth token` (GitHub CLI)
2. `GITHUB_TOKEN` env var
3. `GH_TOKEN` env var

For GitHub Enterprise: `export GH_HOST=github.mycompany.com`

### GitLab

Uses (in order):
1. `glab auth token` (GitLab CLI)
2. `GITLAB_TOKEN` env var
3. `GL_TOKEN` env var

For self-hosted: `export GITLAB_HOST=gitlab.mycompany.com`

### Test authentication

```sh
ryu auth github test
ryu auth gitlab test
```

## Usage

### Viewing stacks

Running `ryu` with no arguments shows your bookmark stacks:

```
$ ryu

Bookmark Stacks
===============

Stack #1: feat-c

       [feat-c]
    @  yskvutnz e5f6a7b8 Add logout endpoint
    |
       [feat-b] ^
    o  mzwwxrlq a1b2c3d4 Add session management
    |
       [feat-a] *
    o  kpqvunts 7d3a1b2c Add user authentication
    |
  trunk()

1 stack, 3 bookmarks

Legend: * = synced, ^ = needs push, @ = working copy
```

### Submitting

```sh
ryu submit feat-c
```

This pushes all bookmarks in the stack, creates PRs for any without one, updates PR base branches, and adds stack navigation comments.

Each PR gets a comment showing the full stack:

```
* #13
* **#12 👈**
* #11

---
This stack of pull requests is managed by jj-ryu.
```

### Syncing

```sh
# Sync all stacks
ryu sync

# Sync a specific stack only
ryu sync --stack feat-c
```

## Workflow example

```sh
# Start a feature
jj new main
jj bookmark create feat-auth

# Work on it
jj commit -m "Add user model"

# Stack another change on top
jj bookmark create feat-session
jj commit -m "Add session handling"

# View the stack
ryu

# Submit both as PRs (feat-session -> feat-auth -> main)
ryu submit feat-session

# Make changes, then update PRs
jj commit -m "Address review feedback"
ryu submit feat-session

# After feat-auth merges, rebase and re-submit
jj rebase -d main
ryu submit feat-session
```

## Advanced options

### Preview and confirmation

```sh
ryu submit feat-c --dry-run    # Preview without making changes
ryu submit feat-c --confirm    # Preview and prompt before executing
```

### Controlling submission scope

```sh
# Submit only up to a specific bookmark (excludes descendants)
ryu submit feat-c --upto feat-b

# Submit only one bookmark (parent must already have a PR)
ryu submit feat-b --only

# Include all descendants in submission
ryu submit feat-a --stack

# Only update existing PRs, don't create new ones
ryu submit feat-c --update-only

# Interactively select which bookmarks to submit
ryu submit feat-c --select
```

### Draft PRs

```sh
# Create new PRs as drafts
ryu submit feat-c --draft

# Publish draft PRs (mark as ready for review)
ryu submit feat-c --publish
```

## CLI reference

```
ryu [OPTIONS] [COMMAND]

Commands:
  submit  Submit a bookmark stack as PRs
  sync    Sync all stacks with remote
  auth    Authentication management

Options:
  -p, --path <PATH>  Path to jj repository
  -h, --help         Print help
  -V, --version      Print version
```

### submit

```
ryu submit <BOOKMARK> [OPTIONS]

Options:
      --dry-run          Preview without making changes
  -c, --confirm          Preview and prompt for confirmation
      --upto <BOOKMARK>  Submit only up to this bookmark
      --only             Submit only this bookmark (parent must have PR)
      --update-only      Only update existing PRs
  -s, --stack            Include all descendants in submission
      --draft            Create new PRs as drafts
      --publish          Publish draft PRs
  -i, --select           Interactively select bookmarks
      --remote <REMOTE>  Git remote (default: origin)
```

### sync

```
ryu sync [OPTIONS]

Options:
      --dry-run          Preview without making changes
  -c, --confirm          Preview and prompt for confirmation
      --stack <BOOKMARK> Only sync this stack
      --remote <REMOTE>  Git remote (default: origin)
```

### auth

```
ryu auth github test    # Test GitHub auth
ryu auth github setup   # Show setup instructions
ryu auth gitlab test    # Test GitLab auth
ryu auth gitlab setup   # Show setup instructions
```

## Coming from Graphite?

Ryu's CLI is inspired by Graphite. Here's how commands map:

| Graphite | Ryu |
|----------|-----|
| `gt submit` | `ryu submit <bookmark>` |
| `gt submit --stack` | `ryu submit <bookmark> --stack` |
| `gt submit --only` | `ryu submit <bookmark> --only` |
| `gt submit --draft` | `ryu submit <bookmark> --draft` |
| `gt submit --publish` | `ryu submit <bookmark> --publish` |
| `gt submit --confirm` | `ryu submit <bookmark> --confirm` |
| `gt sync` | `ryu sync` |
| `gt branch create` | `jj bookmark create` |
| `gt restack` | `jj rebase` |

Key differences:
- Ryu requires an explicit bookmark argument (jj doesn't track "current branch")
- Stack management uses jj commands (`jj bookmark`, `jj rebase`), not ryu
- `ryu sync --stack <bookmark>` syncs a single stack (Graphite syncs all)

## License

MIT
