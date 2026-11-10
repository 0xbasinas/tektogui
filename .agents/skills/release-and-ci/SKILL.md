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
| Linux check | every PR and every push | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`. That test step is the replay check and the screenshot check: `crates/tekto/tests/record_replay.rs` loads `fixtures/session.json`, and `headless_snapshot` compares PNG bytes to `tests/snapshots/notes-headless.png`. Do not add a second PR job for them. |
| Windows compile | `push` to `nightly`, or manual dispatch with `full=true` | `cargo check --workspace --all-targets` on `windows-latest` |
| Android compile | `push` to `nightly`, or manual dispatch with `full=true` | `cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu` |
| WASM core check | `push` to `nightly`, or any `workflow_dispatch` | `cargo check -p tekto-wasm --target wasm32-unknown-unknown` and `cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js` |

`RUSTFLAGS=-D warnings`. Caches use Swatinem/rust-cache. Concurrency cancels older runs on the same ref.

**Keep it light:** do not run Windows, Android, emulators, packaging, or wasm on every PR. Windows and Android stay on nightly or `workflow_dispatch` with `full=true`. The wasm job runs on a push to `nightly` or any `workflow_dispatch`. Timeouts are already 20–30 minutes.

## Packaging (local, not CI)

Notes ships as a small release binary. Do not enable `quickjs` or `gpu` for the default package. Do not commit `dist/` artifacts. No store signing and no MSI. Requirements we have not met are in [docs/signing.md](../../../docs/signing.md). Do not buy certificates, commit keystores, or add a macOS signing job.

```bash
export CARGO_TARGET_DIR=/path/to/this/worktree/target
./scripts/package-notes-linux.sh
```

That writes `dist/tektogui-notes-linux-x86_64.tar.gz`. Record the size in [docs/packaging.md](../../../docs/packaging.md) when it changes. Windows is a folder or zip assembled on a Windows machine (`notes.exe` plus `README.txt`). Do not claim a Windows package from a Linux check that only has the `x86_64-pc-windows-gnu` rustc target.

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
