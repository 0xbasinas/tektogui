---
name: pick-up-an-issue
description: Pick a GitHub issue, implement a small change, and open a PR against nightly. Use at the start of a tektogui task, especially good-first-agent issues, or when you are unsure which branch and checklist to use.
---

# Pick up an issue

Repo: https://github.com/0xbasinas/tektogui

## Steps

1. Read the issue and its milestone (`Month 1` … `Month 6`). Prefer labels `good-first-agent` or a clearly scoped bug.

```bash
gh issue list --repo 0xbasinas/tektogui --label good-first-agent --state open
gh issue view NUMBER --repo 0xbasinas/tektogui
```

2. If the work matches another skill (widget, crate, native feature, MCP tool, test, Android, web, CI), **read that skill first**.
3. Branch from latest `nightly` (never directly from stale `main` unless you are only merging nightly → main):

```bash
git fetch origin
git checkout nightly && git pull --ff-only origin nightly
git checkout -b feat/issue-NUMBER-short-slug
```

4. Keep the PR small. Do not wrap Electron, Tauri, or Flutter. Do not add GPL/LGPL deps. Default features stay lean. Public APIs stay free of DOM types. Commit as `0xbasinas <241305723+0xbasinas@users.noreply.github.com>`. Never force-push `main` or `nightly`. Never print secrets.

5. Check locally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npm test
```

6. Open the PR **against `nightly`**:

```bash
git push -u origin HEAD
gh pr create --base nightly --title "…" --body "Fixes #NUMBER"
```

## PR checklist (also in `.github/PULL_REQUEST_TEMPLATE.md`)

- Base branch is `nightly`, not `main`.
- What changed is listed in plain English.
- `cargo fmt --all -- --check` and `cargo test` (Linux, including the headless snapshot) were run.
- Windows / Android compile was run if the PR touches platform code.
- If you changed a command, flag, or crate rule, you updated the matching skill under `.agents/skills/` (and the symlink in `.claude/skills/` still points at it).

## Verify

The PR page shows base `nightly`, the issue is linked, and Linux CI is green.
