---
name: release-and-ci
description: Branching, CI, and how work moves from a feature branch to nightly and then to main. Use when editing GitHub Actions, opening or merging PRs, or cutting a release. Keep Actions minutes low.
---

# Release and CI

## Branch flow

- `main` is the default branch and should stay releasable.
- Day-to-day work lands as **small PRs into `nightly`**, not into `main`.
- After nightly looks good, merge `nightly` into `main` (a merge commit or a PR). Do not force-push either branch.
- Feature branches: `feat/…` or `fix/…` from the latest `nightly`.

```bash
git fetch origin
git checkout nightly
git pull --ff-only origin nightly
git checkout -b feat/short-name
# … commit as 0xbasinas <241305723+0xbasinas@users.noreply.github.com>
git push -u origin HEAD
gh pr create --base nightly --title "…" --body "…"
```

## CI (`.github/workflows/ci.yml`)

Triggers: pull requests, pushes to `nightly` or `main`, and `workflow_dispatch`.

| Job | When it runs | What it does |
|---|---|---|
| Linux check | every PR and every push | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets` |
| Windows compile | `push` to `nightly`, or manual dispatch | `cargo check --workspace --all-targets` on `windows-latest` |
| Android compile | `push` to `nightly`, or manual dispatch | `cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu` |

`RUSTFLAGS=-D warnings`. Caches use Swatinem/rust-cache. Concurrency cancels older runs on the same ref.

**Keep it light:** do not run Windows, Android, emulators, or (later) wasm on every PR. Put expensive jobs on nightly or `workflow_dispatch`. Timeouts are already 25–30 minutes.

## Local check before you open the PR

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npm test
```

If you changed platform code, also run the Windows and/or Android compile commands from the `android-build-and-run` skill (compile only).

## Verify

- The PR targets `nightly`.
- Linux CI is green.
- After merge to nightly, the Windows and Android compile jobs on that push are green or you filed a follow-up (see Month 1 issues).
