---
name: write-headless-test
description: Add or update a headless Tekto test that asserts the scene/AccessKit tree and a CPU-rendered PNG, or a JSON record/replay script. Use when changing widgets, layout, text, or painting, or when adding a snapshot or a recorded session.
---

# Write a headless test

Prefer tests that do not need a window. Use the CPU renderer, a fixed size (Notes is 640×400), and bundled Roboto so CI matches local runs.

## Files to touch

- Integration snapshot: `crates/tekto/tests/headless_snapshot.rs`
- Record/replay script: `crates/tekto/tests/record_replay.rs` and `crates/tekto/tests/fixtures/session.json`
- Or a unit test next to the code (`crates/tekto-widgets`, `tekto-layout`, `tekto-text`, `tekto-a11y`, `tekto/src/lib.rs`)
- This skill, if the snapshot command, `TEKTO_SHOT_DIR`, or the recording helper changes

## Pattern

```rust
use tekto::{accesskit_tree_update, layout_tree, notes_tree, render_headless_to_file};

let mut tree = notes_tree(640.0, 400.0).expect("tree");
layout_tree(&mut tree, 640.0, 400.0);

let pretty = tree.to_pretty_text();
assert!(pretty.contains("window#1"), "{pretty}");
assert!(pretty.contains("Hello from Tekto"), "{pretty}");

let update = accesskit_tree_update(&tree).expect("accesskit");
assert!(update.nodes.len() >= 3);

let dir = std::path::PathBuf::from("target/tektogui-shots");
std::fs::create_dir_all(&dir).unwrap();
let path = dir.join("notes-headless.png");
let png = render_headless_to_file(&tree, 640, 400, Some(&path)).unwrap();
assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
```

The existing snapshot test writes a copy to `$TEKTO_SHOT_DIR` if set, otherwise `target/tektogui-shots/notes-headless.png`, and compares PNG bytes to the blessed file `crates/tekto/tests/snapshots/notes-headless.png`. CI fails if the pixels change.

## Commands

```bash
cargo test --workspace --all-targets
TEKTO_SHOT_DIR=/tmp/tekto-shots cargo test -p tekto --test headless_snapshot -- --nocapture
# Look at /tmp/tekto-shots/notes-headless.png

# After an intentional typography or paint change, bless the new reference:
TEKTO_BLESS=1 cargo test -p tekto --test headless_snapshot
# Then commit crates/tekto/tests/snapshots/notes-headless.png

# JS unit tests (signals / JSX factory only; they do not paint):
npm test
# also: pnpm test    or    bun test ./packages/core/test.js ./packages/jsx-runtime/test.js
```

Do not start a display or an emulator for these tests.

## Motion

`Tree::transition` then `Tree::advance(ms)` moves opacity and bounds. Assert a midpoint and the end. Paint with `render_tree_png` if you need pixels. A short loop of frames is enough to note CPU cost; the renderer repaints the whole scene each time. Set `TEKTO_REDUCE_MOTION=1` only when the test should snap. Do not read OS animation settings in the test.

## Record and replay

`tekto::Recording` is a version 1 JSON log of tree actions: `patch`, `click`, `type`, `key`, and `pointer_click`. Call `record(&mut tree, action)` to apply an action and append it. Save the log with `serde_json`. Call `replay(&mut tree, layout)` on a tree that was built and laid out the same way. The layout callback runs after each action so pointer hits see current bounds.

This helper does not watch a live window or the dev server. It does not run app callbacks or write patches back to source. Pass each action in yourself.

The CI test loads `crates/tekto/tests/fixtures/session.json`, records those events, replays the log, and checks that the tree text, focus, and last click match.

```bash
cargo test -p tekto --test record_replay
```
