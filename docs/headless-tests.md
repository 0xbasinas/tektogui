# Headless app tests

`tekto-test` is a dev-only helper for the real scene tree and AccessKit mapping.
Add it as a dev dependency and build a tree using the same widgets as the app.

```rust
use tekto_test::TestApp;
use tekto_tree::Tree;
use tekto_widgets::{button, window};

let mut tree = Tree::new();
let root = window(&mut tree, "Form", 320.0, 240.0).unwrap();
button(&mut tree, root, "Save").unwrap();
let mut app = TestApp::new(tree, 320, 240).unwrap();
let save = app.find("Button", "Save").unwrap();
app.click(save).unwrap();
assert!(app.is_focused(save).unwrap());
```

`find` requires an exact, unique role and accessible name. `node` rejects ids
that are missing or absent from the attached AccessKit tree. `click`,
`type_text`, and `focus` call the tree's normal action methods, so disabled
controls and overlay blocking still apply. Use `value`, `checked`, and
`disabled` to read semantic state. `wait_until` advances a deterministic clock
in fixed steps. `png` returns a CPU-rendered PNG at the fixed viewport size.

An optional `test_key` on a Rust node, or `testKey` on a TSX element, can be
resolved with `find_key`. It must be unique among attached nodes; missing and
duplicate keys return errors. The key is separate from the accessible name.

`compare_png(reference, artifacts, update)` compares decoded pixels. Pass
`true` explicitly to update the reference. A mismatch writes `actual.png`,
`expected.png`, and `diff.png` into the chosen artifact directory.

Enable the `quickjs` feature for `JsTestApp`, which retains desktop JavaScript
callbacks and delivers click and committed input events after the tree action.
QuickJS is off by default. Call `dispose` when the session is finished.
