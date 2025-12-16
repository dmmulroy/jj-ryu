# jj-ryu
<img width="366" height="366" alt="image" src="https://github.com/user-attachments/assets/1691edfc-3b65-4f8d-b959-71ff21ff23e5" />

Stacked PRs for [Jujutsu](https://jj-vcs.github.io/jj/latest/). Push bookmark stacks to GitHub and GitLab as chained pull requests.

## What it does

```
trunk()                           PR #1: feat-a → main
  │                               PR #2: feat-b → feat-a
  ○ feat-a  ──────────────────►   PR #3: feat-c → feat-b
  │
  ○ feat-b
  │
  ○ feat-c
```

Each bookmark becomes a PR. Each PR targets the previous bookmark (or trunk). When you update your stack, `ryu` updates the PRs.

## Install

```sh
# npm (includes prebuilt binaries)
npm install -g jj-ryu

# or with npx
npx ryu

# cargo
cargo install jj-ryu
```

Binary name is `ryu`.

**macOS:** If you see "ryu can't be opened", run:
```sh
xattr -d com.apple.quarantine $(which ryu)
```

## Quick start

```sh
# View your bookmark stacks
ryu

# Submit a stack as PRs
ryu submit feat-c

# Preview what would happen
ryu submit feat-c --dry-run

# Sync all stacks
ryu sync
```

## Usage

### Visualize stacks

Running `ryu` with no arguments shows your bookmark stacks:

```
$ ryu

Bookmark Stacks
===============

Stack #1: feat-c

  trunk()
    │
    ○  kpqvunts 7d3a1b2c Add user authentication
    │  └─ [feat-a] ✓
    │
    ○  mzwwxrlq a1b2c3d4 Add session management
    │  └─ [feat-b] ↑
    │
    @  yskvutnz e5f6a7b8 Add logout endpoint
    │  └─ [feat-c]

1 stack, 3 bookmarks

Legend: ✓ = synced with remote, ↑ = needs push, @ = working copy

To submit a stack: ryu submit <bookmark>
```

### Submit a stack

```sh
ryu submit feat-c
```

This will:
1. Push all bookmarks in the stack to remote
2. Create PRs for bookmarks without one
3. Update PR base branches if needed
4. Add stack navigation comments to each PR

Output:

```
Submitting 3 bookmarks in stack:
  - feat-a (synced)
  - feat-b
  - feat-c

Pushing bookmarks...
  - feat-a already synced
  ✓ Pushed feat-b
  ✓ Pushed feat-c
Creating PRs...
  ✓ Created PR #12 for feat-b
    https://github.com/you/repo/pull/12
  ✓ Created PR #13 for feat-c
    https://github.com/you/repo/pull/13
Updating stack comments...
Done!

Successfully submitted 3 bookmarks
Created 2 PRs
```

### Stack comments

Each PR gets a comment showing the full stack:

```
This PR is part of a stack of 3 bookmarks:

1. `trunk()`
1. [feat-a](https://github.com/you/repo/pull/11)
1. **feat-b ← this PR**
1. [feat-c](https://github.com/you/repo/pull/13)
```

Comments update automatically when you re-submit.

### Dry run

Preview without making changes:

```sh
ryu submit feat-c --dry-run
```

### Sync all stacks

Push all stacks to remote and update PRs:

```sh
ryu sync
```

## Authentication

### GitHub

Uses (in order):
1. `gh auth token` (GitHub CLI)
2. `GITHUB_TOKEN` env var
3. `GH_TOKEN` env var

For GitHub Enterprise, set `GH_HOST`:

```sh
export GH_HOST=github.mycompany.com
```

### GitLab

Uses (in order):
1. `glab auth token` (GitLab CLI)
2. `GITLAB_TOKEN` env var
3. `GL_TOKEN` env var

For self-hosted GitLab, set `GITLAB_HOST`:

```sh
export GITLAB_HOST=gitlab.mycompany.com
```

### Test authentication

```sh
ryu auth github test
ryu auth gitlab test
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

# Submit both as PRs (feat-session → feat-auth → main)
ryu submit feat-session

# Make changes, then update PRs
jj commit -m "Address review feedback"
ryu submit feat-session

# After feat-auth merges, rebase and re-submit
jj rebase -d main
ryu submit feat-session
```

## Limitations

- Bookmarks with merge commits in their history are excluded
- Linear stacks only (no diamond-shaped dependencies)
- One remote per operation

## CLI reference

```
ryu [OPTIONS] [COMMAND]

Commands:
  submit  Submit a bookmark stack as PRs
  sync    Sync all stacks with remote
  auth    Authentication management

Options:
  -p, --path <PATH>  Path to jj repository
  -V, --version      Print version
  -h, --help         Print help
```

### submit

```
ryu submit <BOOKMARK> [OPTIONS]

Arguments:
  <BOOKMARK>  Bookmark to submit

Options:
  --dry-run          Preview without making changes
  --remote <REMOTE>  Git remote to use (default: origin)
```

### sync

```
ryu sync [OPTIONS]

Options:
  --dry-run          Preview without making changes
  --remote <REMOTE>  Git remote to use (default: origin)
```

### auth

```
ryu auth github test    # Test GitHub auth
ryu auth github setup   # Show setup instructions
ryu auth gitlab test    # Test GitLab auth
ryu auth gitlab setup   # Show setup instructions
```

## License

MIT
