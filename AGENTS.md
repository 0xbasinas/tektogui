# Notes for coding agents

This file is for agents (and humans) working in this repo.

## What Tekto is

Tekto is our own GUI runtime. Do not wrap Electron, Tauri, or Flutter. Apps are general purpose. They can be written in Rust or in TypeScript + JSX. Both must stay first-class.

Priorities, in practice:

1. Agents can build and change apps quickly (tree, MCP, hot reload, live patches).
2. Compile and build times stay short (lean crate graph, feature flags, no heavy deps in default builds).
3. Ships stay small and start quickly.
4. The default look is plain, quiet, and comfortable (spacing, typography, later smooth scroll and animation). Themes come later.

## Git

- Open pull requests against **`nightly`**, not `main`.
- Keep PRs small and focused.
- Never force-push to `main` or `nightly`.
- Commit as `0xbasinas <241305723+0xbasinas@users.noreply.github.com>` when committing in this project.
- Do not print secrets. Keep GitHub Actions minutes low (Linux checks on PRs; Windows and Android only on `nightly` or a manual run).

## Code rules

- Simple English in docs, comments, and commit messages.
- One crate does one job. Put new code in the crate that already owns that job.
- Public APIs should be small and easy to read.
- Default features must stay lean. Optional, heavy work (QuickJS, wgpu / Vello) goes behind feature flags and is off by default.
- Only MIT, Apache-2.0, BSD, or zlib dependencies. If you add a crate, write the license in the PR.
- Do not invent a new DSL. TypeScript and Rust are enough.
- Do not put HTML, CSS, or DOM types in the public app API. The web target paints our tree; apps never see the DOM.
- Every interactive widget needs a stable node id and an AccessKit role and name. The AccessKit tree *is* the agent tree. Do not build a second parallel tree.
- Live edits (`ui.patch` / `tekto patch`) must work in dev mode alongside hot reload. Runtime patches land first; writing the change back to source comes next, but keep the door open.
- Prefer tests you can run headless (CPU renderer, fixed size, bundled Roboto).

## Platform abstractions

`tekto-platform` must stay host-agnostic enough for:

- Windows, Linux (and later macOS)
- Android (and later iOS)
- Web (WASM core + browser JS, not QuickJS)

`tekto-js` is an op protocol. Desktop will embed QuickJS-ng later. Web uses the browser engine and calls the same ops on a WASM module.

`tekto-native` is the plugin API for files, dialogs, clipboard, notifications, tray, and global shortcuts. Fill in one platform at a time; stubs should return a clear error.

## How to check a change

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm test
```

For a headless PNG:

```bash
TEKTO_SHOT_DIR=/tmp/tekto-shots cargo test -p tekto --test headless_snapshot
```
