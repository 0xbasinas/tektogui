---
name: add-widget
description: Add a built-in Tekto widget (Rust first, then a TSX sketch, AccessKit role, and a headless snapshot or tree test). Use when adding a button, field, list, or any new reusable UI control.
---

# Add a widget

Widgets live in `crates/tekto-widgets`. They build nodes on the shared scene tree. The AccessKit tree is the agent tree — do not invent a second one.

JSX lowers through `@tektogui/jsx-runtime` `mount` / `renderToOps` (`window`, `group`, `rect`, `text`, `button`, `text_field`, `scroll`, `list`, `list_item`) onto the same Rust tree. The Rust helper may set default fill, color, size, and padding. JSX does not. Set those props on the element or the node stays unstyled.

## Files to touch

- `crates/tekto-widgets/src/lib.rs` — constructor, default spacing/type, unit test
- `crates/tekto-tree/src/lib.rs` — add a `NodeKind` variant only if none of `Window`, `Group`, `Rect`, `Text`, `Button`, `TextField`, `Scroll`, `List`, `ListItem` fits
- `crates/tekto-layout/src/lib.rs` and `crates/tekto-render/src/lib.rs` — measure and paint the new kind if it carries text or chrome (Button is measured like Text; paint insets the label by `layout.padding`)
- `crates/tekto-a11y/src/lib.rs` — `role_for` must map the kind (Button already maps to `Role::Button`)
- `crates/tekto/src/lib.rs` — re-export the new function if apps should call it
- `packages/jsx-runtime/src/components.json` — add the kind, its props, and its events. The JSX runtime and `app.components` both read this file. Do not hand-copy the kind list into `index.js`.
- `packages/jsx-runtime` — `renderToOps` test. Props must be keys `tekto-js` already accepts (`HOST_PROP_KEYS`), or an alias in `propAliases`.
- `examples/notes/src/app.tsx` or a new example — TSX sketch using plain elements (no DOM)
- `crates/tekto/tests/headless_snapshot.rs` or a widgets unit test — tree + PNG asserts
- `docs/getting-started.md` — one sentence if apps should call the helper
- This skill, if you change the steps or commands

## Steps

1. Follow the existing helpers (`window`, `banner`, `text`, `title_on_banner`, `button`) in `crates/tekto-widgets/src/lib.rs`.
2. `tree.add(NodeKind::…, Some(parent))?`, then set `name`, `text`, `fill` / `color`, `font_size`, and `layout`. Give every interactive widget a stable name; ids are assigned by the tree and are never reused.
3. Use the quiet defaults: `PAGE_PADDING` 24, `STACK_GAP` 16, `BODY_SIZE` 20, `TITLE_SIZE` 22, colors `Color::PAGE` (`#f4f3f0`), `Color::INK` (`#2c333a`), `Color::BANNER` (`#3a424a`), `Color::ON_BANNER`, `Color::FIELD` (`#e4e2dc`). Do not add a theme system. `button(tree, parent, label)` is the reference control: `NodeKind::Button`, name and text set to the label, fill `Color::BANNER`, color `Color::ON_BANNER`, `font_size` `BODY_SIZE`, `layout.padding` 12. It returns that node id for clicks and accessibility. `text_field(tree, parent, label, value)` is the single-line field: `NodeKind::TextField`, `name` is the label, `text` is the committed value, fill `Color::FIELD`, color `Color::INK`, padding 10, caret at the end of `value`. When committed text and IME preedit are both empty, the renderer draws the field's `name` at reduced opacity (55% of the field color) as a hint, and layout measures that hint. The hint is never stored as the value. Once there is committed text or preedit, the hint is gone. Desktop IME preedit and commit go through `Tree::apply_ime` (`ImeEvent`).
4. If the node shows text, measure it in `tekto-layout` the same way as `Text` (leaf with context). When painting, inset the glyphs by the node's padding so the label sits inside the fill.
5. If you added a `NodeKind`, update `as_str` and `role_for` in `tekto-a11y`. Interactive widgets need a role and a name/label. `NodeKind::Button` maps to `Role::Button` (label is the node name). `NodeKind::TextField` maps to `Role::TextInput`: AccessKit label is `name`, value is `Node::composed_text` (committed text plus any IME preedit, not the hint), same stable id. `NodeKind::Scroll` maps to `Role::ScrollView`. `NodeKind::List` maps to `Role::List` and `NodeKind::ListItem` to `Role::ListItem` (same scene ids). `scroll(tree, parent, width, height)` is a vertical viewport: children keep their height, paint is clipped to the box, and `Tree::scroll_by` / `set_scroll_y` shift `scroll_y` (clamped, no rubber-band). A wheel notch is `SCROLL_WHEEL_LINE` (48px). Desktop wheel and pointer drag call that API. Touch scrolling is not wired yet. Layout windows a scrolling list (viewport plus `LIST_OVERSCAN_PX`, 64), using the list's position in the scroll (padding and earlier siblings), not the raw `scroll_y`. The helper does not virtualize on its own; it still builds every row, and layout detaches the ones outside that window. `Tree::remove` drops those detached rows and the spacers. `append` takes a row out of the old list's slots. `list(tree, parent, width, viewport_height, rows)` stacks `ListRow { label, height }` rows (`LIST_GAP` 8, `LIST_ROW_PADDING` 10, ink, body size). A non-finite row height hugs the label (measured text plus vertical padding). When the stacked rows are taller than a finite viewport, the list gets a `scroll` parent; a shorter list, or a non-finite viewport, stays a direct child. `list_item` adds one row and revisits that decision: it inserts the scroll container once the stack overflows, and does not wrap a list that already has a scroll parent. JSX `<list>` / `<list_item>` do not insert that scroll; put `<list>` inside `<scroll>` in TSX when the viewport should clip.
6. Re-export from `crates/tekto/src/lib.rs` if it is part of the Rust app API.
7. JSX: `<button>Save</button>` lowers to `{ op: "create", kind: "button" }` then `{ op: "setText", id, text: "Save" }`. `<text_field name="Title">Hello</text_field>` lowers to kind `text_field`, `setProp` name, and `setText`. JSX does not copy the Rust fill, color, size, or padding. Set `fill`, `color`, `fontSize`, and `padding` on the element when the sketch should match the helper. Add a new kind to `components.json` (that file fills `INTRINSICS`) and to `NodeKind::all` (the match there is exhaustive). List every host prop on that kind: `name`, `title`, `text`, `width`, `height`, `fill`, `color`, `fontSize`, `padding`, `gap`, `column`, `justifyCenter`. JSX forwards any catalog prop on every kind, and `setProp` applies those keys on every node. `component_schema_lists_every_host_prop` fails if one is missing. Events come from `NodeKind::agent_events` (`click` on every kind; `type`, `key`, `ime` on a text field; `scroll` on a scroll container). `app_components_lists_core_widgets` fails if a kind or those events are missing.
8. Add a TSX sketch (see `examples/notes/src/app.tsx`) with the same structure. Import signals from `@tektogui/core` if needed. JSX must return a plain element, never a DOM node.
9. Add a test that checks `tree.to_pretty_text()` (for example `button#…` and the label), `click_at` on the center, `last_click`, and an AccessKit node with role `Button` and the label. For a text field, also check `text` / `ui.patch`, `apply_field_key` (insert and backspace, no window), `apply_ime` (preedit then commit, no window), and an AccessKit node with role `TextInput`, label `name`, and value equal to the composed text. If the widget is visible, also assert a headless PNG (see the `write-headless-test` skill). Cover `renderToOps` in `packages/jsx-runtime/test.js`. For a list of many rows, see that skill's virtualization section.

## Text field limits

`text_field` accepts printable characters and Backspace on the desktop when it is focused (`Tree::apply_field_key`, wired from winit `KeyboardInput`). When focus is a text field, the window calls `set_ime_allowed(true)` and handles `WindowEvent::Ime`: `Preedit` sets `preedit` (shown at the caret; AccessKit value includes it) and `Commit` inserts into `text` and clears the preedit. That path is shared Linux and Windows code. Tests inject `ImeEvent` with no display and no IME daemon. `ui.type` / `POST /type` insert plain characters at the caret and drop a preedit. Clicking the field moves the caret to the end and clears a preedit. Agents read the committed value with node `text` (`ui.tree`, `ui.patch`). `name` stays the label. AccessKit value is `Node::composed_text` (committed plus preedit).

Still open:

- No candidate window of our own. We call `set_ime_cursor_area` with the caret so the OS popup can sit there. X11 uses the position only.
- No marked-range highlight. A preedit cursor `None` hides our caret. A byte range places the caret at the end of that range, not a selection.
- No Android soft keyboard (`set_ime_allowed` on Android would show one later; do not start an emulator for this).
- No Windows runtime IME session in CI. The same `desktop.rs` match is what Windows builds use.
- A keyboard string equal to the last IME commit is dropped only until the event queue goes idle (`about_to_wait`). That catches a Windows echo in the same burst. A later press of the same string is inserted. Printable keys call `Tree::type_keyboard_text`, which does nothing while a preedit is showing. `set_focus` clears the previous field's preedit.
- No selection, arrow keys, copy, or paste
- `ui.type` does not send Backspace or a composition

## Verify

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
node --test packages/core/test.js packages/jsx-runtime/test.js
```

If you painted pixels:

```bash
TEKTO_SHOT_DIR=/tmp/tekto-shots cargo test -p tekto --test headless_snapshot -- --nocapture
```
