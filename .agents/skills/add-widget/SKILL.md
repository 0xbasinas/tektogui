---
name: add-widget
description: Add a built-in Tekto widget (Rust first, then a TSX sketch, AccessKit role, and a headless snapshot or tree test). Use when adding a button, field, list, or any new reusable UI control.
---

# Add a widget

Widgets live in `crates/tekto-widgets`. They build nodes on the shared scene tree. The AccessKit tree is the agent tree — do not invent a second one.

TSX is not wired to the Rust tree yet (Month 2). Still update the TS sketch so the API stays designed in the open.

## Files to touch

- `crates/tekto-widgets/src/lib.rs` — constructor, default spacing/type, unit test
- `crates/tekto-tree/src/lib.rs` — add a `NodeKind` variant only if none of `Window`, `Group`, `Rect`, `Text`, `Button` fits
- `crates/tekto-a11y/src/lib.rs` — `role_for` must map the kind (Button already maps to `Role::Button`)
- `crates/tekto/src/lib.rs` — re-export the new function if apps should call it
- `examples/notes/src/app.tsx` or a new example — TSX sketch using plain elements (no DOM)
- `crates/tekto/tests/headless_snapshot.rs` or a widgets unit test — tree + PNG asserts
- This skill, if you change the steps or commands

## Steps

1. Follow the existing helpers (`window`, `banner`, `text`, `title_on_banner`) in `crates/tekto-widgets/src/lib.rs`.
2. `tree.add(NodeKind::…, Some(parent))?`, then set `name`, `text`, `fill` / `color`, `font_size`, and `layout`. Give every interactive widget a stable name; ids are assigned by the tree and are never reused.
3. Use the quiet defaults: `PAGE_PADDING` 24, `STACK_GAP` 16, `BODY_SIZE` 20, `TITLE_SIZE` 22, colors `Color::PAGE` (`#f4f3f0`), `Color::INK` (`#2c333a`), `Color::BANNER` (`#3a424a`), `Color::ON_BANNER`. Do not add a theme system.
4. If you added a `NodeKind`, update `as_str` and `role_for` in `tekto-a11y`. Interactive widgets need a role and a name/label.
5. Re-export from `crates/tekto/src/lib.rs` if it is part of the Rust app API.
6. Add a TSX sketch (see `examples/notes/src/app.tsx`) with the same structure. Import signals from `@tektogui/core` if needed. JSX must return a plain element, never a DOM node.
7. Add a test that checks `tree.to_pretty_text()` (for example `button#…` and the label) and, if the widget is visible, a headless PNG (see the `write-headless-test` skill).

## Verify

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npm test
```

If you painted pixels:

```bash
TEKTO_SHOT_DIR=/tmp/tekto-shots cargo test -p tekto --test headless_snapshot -- --nocapture
```
