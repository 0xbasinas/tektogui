---
name: add-widget
description: Add a built-in Tekto widget (Rust first, then a TSX sketch, AccessKit role, and a headless snapshot or tree test). Use when adding a button, field, list, or any new reusable UI control.
---

# Add a widget

Widgets live in `crates/tekto-widgets`. They build nodes on the shared scene tree. The AccessKit tree is the agent tree — do not invent a second one.

JSX lowers through `@tektogui/jsx-runtime` `mount` / `renderToOps` (`window`, `group`, `rect`, `text`, `button`, `text_field`, `scroll`, `list`, `list_item`) onto the same Rust tree. `window`, `button`, and `text_field` get the same default fill, color, size, and padding as the Rust helpers unless the element sets those props. Other kinds stay unstyled until you set those props.

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
7. JSX: `<button>Save</button>` lowers to `{ op: "create", kind: "button" }` then `{ op: "setText", id, text: "Save" }`. `<text_field name="Title">Hello</text_field>` lowers to kind `text_field`, `setProp` name, and `setText`. JSX `window`, `button`, and `text_field` receive the same defaults as the Rust helpers. Explicit `fill`, `color`, `fontSize`, and `padding` props override those defaults regardless of prop order. Other kinds still need those props set when they should be styled. Add a new kind to `components.json` (that file fills `INTRINSICS`) and to `NodeKind::all` (the match there is exhaustive). List every host prop on that kind: `name`, `title`, `text`, `width`, `height`, `fill`, `color`, `fontSize`, `padding`, `gap`, `column`, `justifyCenter`. JSX forwards any catalog prop on every kind, and `setProp` applies those keys on every node. `component_schema_lists_every_host_prop` fails if one is missing. Events come from `NodeKind::agent_events` (`click` on every kind; `type`, `key`, `ime` on a text field; `scroll` on a scroll container). `app_components_lists_core_widgets` fails if a kind or those events are missing.
8. Add a TSX sketch (see `examples/notes/src/app.tsx`) with the same structure. Import signals from `@tektogui/core` if needed. JSX must return a plain element, never a DOM node.
9. Add a test that checks `tree.to_pretty_text()` (for example `button#…` and the label), `click_at` on the center, `last_click`, and an AccessKit node with role `Button` and the label. For a text field, also check `text` / `ui.patch`, `apply_field_key` (insert and backspace, no window), `apply_ime` (preedit then commit, no window), and an AccessKit node with role `TextInput`, label `name`, and value equal to the composed text. If the widget is visible, also assert a headless PNG (see the `write-headless-test` skill). Cover `renderToOps` in `packages/jsx-runtime/test.js`. For a list of many rows, see that skill's virtualization section.

## Disabled controls

Checkboxes and switches support `disabled` with the same behavior. `switch(tree, parent, label, checked)` and JSX `<switch checked={false}>Label</switch>` use the AccessKit Switch role and toggle on Space or click. `checkbox(tree, parent, label, checked)` builds a labeled toggle; Space and clicks toggle `Node::checked` and emit `TreeEvent::Changed`. AccessKit reports `Role::CheckBox` and the toggled state. JSX accepts `<checkbox checked={false}>Label</checkbox>`; a JSX change callback is a follow-up.

Buttons and text fields accept `disabled` (Rust `Node` / `NodePatch`, JSX `disabled` prop). Disabled controls are skipped by Tab, reject pointer, keyboard, and agent activation or edits, paint at half opacity, and report AccessKit disabled state. Disabling the focused control moves focus to the root and clears preedit. Enabled controls keep the existing look; snapshots must stay identical when `disabled` is false.

A single-select uses `select` and `select_option` in Rust, or `<select name="Size" onInput={handler}><menu_item checked={true}>Small</menu_item></select>` in TSX. Option labels are unique non-empty values; option ids persist across opening and closing. Selection uses the overlay and delivers `onInput` with the selected label.

A popup menu uses `menu(tree, label)` and `menu_item(tree, menu, label)` or TSX `<menu name="Actions"><menu_item onClick={handler}>Open</menu_item></menu>`. Open it with the overlay API. Up, Down, Home, End, Enter, and Space operate enabled items; selection closes the menu after the click action.

Rust context menus use `tree.bind_context_menu(target, menu)` with a detached named menu. Desktop right-click opens it at the pointer; the menu and item IDs survive dismissal and are removed with the target. TSX uses `contextMenu={<menu name="Actions">...</menu>}`; desktop and browser right-click use the same binding.

A named modal dialog uses `dialog(tree, label)` and `tree.open_overlay(id, window, OverlaySide::Center)`, or a TSX `<dialog name="Confirm">` subtree opened through `openOverlay(id, window, "center")`. It traps focus, dims the window, blocks background actions, and restores focus when dismissed.

A labeled radio group uses `radio_group` and `radio` or `<radio_group name="Size"><radio checked={true}>Small</radio></radio_group>`. Direct radio siblings are mutually exclusive; arrow keys move among enabled options and AccessKit exposes RadioGroup and RadioButton roles.

A tab list uses `tab_list` and `tab` in Rust, or `<tab_list name="Pages"><tab checked={true}>General</tab></tab_list>` in TSX. Enabled tabs share one Tab stop; arrow keys move selection. Arrow selection delivers retained `onClick` callbacks. Associate panels with `tab_panel(tree, parent, tab, label)` or `<tab panel={<tab_panel>...</tab_panel>}>`; inactive panels retain state and are not laid out or exposed to focus and accessibility.

A labeled slider uses `slider(tree, parent, label, value)` or `<slider name="Volume" value={0.5} />`; values are normalized to 0..=1, pointer capture permits dragging, and arrow keys, Home, End, and `ui.set_value` change an enabled slider.

A labeled determinate progress indicator uses `progress(tree, parent, label, value)` or `<progress name="Upload" value={0.5} />`. Values are finite and clamp to 0..=1; AccessKit exposes the ProgressIndicator role and numeric range.

Images use `Image::new(width, height, rgba)` and `image(tree, parent, data, width, height, fit, name)`, or TSX `<image name="Logo" imageSource={{ width: 1, height: 1, rgba: [255, 0, 0, 255] }} />`. Use `None` in Rust or `decorative={true}` in TSX for a decorative image. `ImageFit` / `imageFit` selects contain or cover. Pixel data is bounded, not serialized with the scene, and app-supplied. The optional `image-png` feature adds static PNG decoding in Rust. CPU and Vello paint images; do not claim a live GPU check from a compile check.

## Text field limits

`text_area(tree, parent, label, value, width, height)` and `<text_field multiline={true} ... />` start a plain multiline field. Enter inserts a newline; desktop and WASM Up/Down move through shaped lines, retain the preferred horizontal position, and Shift extends the selection. Layout scrolls to the caret and clips the text. Home/End currently use hard-line boundaries. Browser IME has host wiring but needs native browser verification; accessibility text ranges and full editor parity are still open.

`text_field` accepts printable characters, Backspace, Delete, Left, Right, Home, and End on the desktop when it is focused (`Tree::apply_field_key`, wired from winit `KeyboardInput`). Those named keys move or delete by Unicode scalar values and are ignored during preedit. When focus is a text field, the window calls `set_ime_allowed(true)` and handles `WindowEvent::Ime`: `Preedit` sets `preedit` (shown at the caret; AccessKit value includes it) and `Commit` inserts into `text` and clears the preedit. That path is shared Linux and Windows code. Tests inject `ImeEvent` with no display and no IME daemon. `ui.type` / `POST /type` insert plain characters at the caret and drop a preedit. Desktop pointer clicks place the caret at the nearest painted character. Agent `ui.click` and `Tree::click(id)` place it at the end. The WASM/browser `click_at` path is not covered by this change. A click clears preedit, then applies the caret to committed text. Agents read the committed value with node `text` (`ui.tree`, `ui.patch`). `name` stays the label. AccessKit value is `Node::composed_text` (committed plus preedit).

Still open:

- No candidate window of our own. We call `set_ime_cursor_area` with the caret so the OS popup can sit there. X11 uses the position only.
- The composing text has an underline. A preedit cursor `None` hides our caret. A byte range places the caret at the end and highlights a non-empty selected subrange.
- No Android soft keyboard (`set_ime_allowed` on Android would show one later; do not start an emulator for this).
- No Windows runtime IME session in CI. The same `desktop.rs` match is what Windows builds use.
- A keyboard string equal to the last IME commit is dropped only until the event queue goes idle (`about_to_wait`). That catches a Windows echo in the same burst. A later press of the same string is inserted. Printable keys call `Tree::type_keyboard_text`, which does nothing while a preedit is showing. `set_focus` clears the previous field's preedit.
- Single-line AccessKit fields expose grapheme-based text runs and selections. Multiline text-range metadata and browser touch selection remain open. Desktop supports selection by Shift+arrows, Shift+Home/End, Ctrl/Cmd+A, mouse drag, and Ctrl/Cmd+C/X/V through the native clipboard.
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
