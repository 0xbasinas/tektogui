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

The existing snapshot test writes a copy to `$TEKTO_SHOT_DIR` if set, otherwise `target/tektogui-shots/notes-headless.png`, and compares PNG bytes to the blessed file `crates/tekto/tests/snapshots/notes-headless.png`. Linux PR CI runs this inside `cargo test --workspace --all-targets` (workflow step "Test (replay + headless PNG compare)"). There is no separate screenshot job. CI fails if the pixels change.

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
# also: pnpm test    or    bun run test   (not plain `bun test`, which finds no test files)
```

Do not start a display or an emulator for these tests.

## Long lists

`layout_tree` windows a `list` inside `scroll`. After layout, only rows that meet the viewport plus `LIST_OVERSCAN_PX` (64) are children, so paint walks that window. `scroll_y` is the scroll box. Subtract the list's offset (scroll padding plus earlier siblings) before picking rows. Other rows keep the ids `list_item` returned until `Tree::remove` drops the list (that also drops spacers) or `append` moves the row. `scroll_extent` is still the full stack. Set `scroll_y`, lay out again, and assert the offset did not move and the rows for that offset are the children. Cover a list that is not at the top of the scroll. `crates/tekto/tests/list_virtual.rs` is the pattern. CPU `paint_tree` is enough. GPU uses the same walk when the `gpu` feature is on.

## Motion

`Tree::transition` then `Tree::advance(ms)` moves opacity and bounds. A channel starts on the first `advance` after the call, so advance a stale clock before `transition` if you are testing an idle gap. Later calls merge props; they do not cancel other channels. Assert a midpoint and the end. After the end, run `layout_tree` again and assert the target bounds still hold (`LayoutStyle` keeps finished x, y, width, and height). Paint with `render_tree_png` if you need pixels. A short loop of frames is enough to note CPU cost; the renderer repaints the whole scene each time. Set `TEKTO_REDUCE_MOTION=1` only when the test should snap. Do not read OS animation settings in the test. AccessKit and the OS reduce-motion settings are still a follow-up.

Touch scroll is headless too. `scroll_touch_begin(x, y, ms)` stops a fling on the scroll under the point. `scroll_touch_move(y, ms)` tracks 1:1 (finger down lowers `scroll_y`). `scroll_touch_end(ms)` flings only if the last 80 ms were faster than about 0.35 px/ms. Then `advance` coasts with a 220 ms decay and clamps with no bounce. A later `scroll_touch_begin` must leave `scroll_y` still on the next `advance`. The unit tests live in `crates/tekto-tree/src/scroll_feel.rs`. Desktop routing (touch clears the swallowed mouse; a cancelled touch does not click, including a control that is not inside a scroll) is in `crates/tekto-platform/src/pointer_route.rs` and does not need a window. No emulator and no display.

## Record and replay

`tekto::Recording` is a version 1 JSON log of tree actions: `patch`, `click`, `type`, `key`, and `pointer_click`. Call `record(&mut tree, action)` to apply an action and append it. Save the log with `serde_json`. Call `replay(&mut tree, layout)` on a tree that was built and laid out the same way. The layout callback runs after each action so pointer hits see current bounds.

This helper does not watch a live window or the dev server. It does not run app callbacks or write patches back to source. Pass each action in yourself.

The same Linux `cargo test --workspace --all-targets` step loads `crates/tekto/tests/fixtures/session.json`, records those events, replays the log, and checks that the tree text, focus, and last click match. That is the CI replay test. Do not add another workflow job for it.

```bash
cargo test -p tekto --test record_replay
```
