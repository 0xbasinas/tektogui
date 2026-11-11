# Desktop TSX events and reactive text

The desktop QuickJS [counter example](../examples/counter/app.tsx) uses
`text={() => count()}` and `onClick={() => setCount(n => n + 1)}`. From the
repository root, with `esbuild` on `PATH` (or `ESBUILD` set to its binary):

```bash
cargo run -p tekto-cli --features quickjs -- dev --js examples/counter/app.tsx
```

In another terminal, inspect the tree, find the button named `Increment`, and
use its actual id in place of `ID`:

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- click ID
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- click ID
cargo run -p tekto-cli -- tree
```

The existing Count text node should show 0, then 1, then 2. Its id and the
button id stay stable while the session runs. You can also click the button in
the window, then press Enter or Space while it has focus. A fresh session starts
at 0. OS screen reader integration requires the optional `a11y-os` feature;
the dev server exposes the AccessKit tree without it.

The `text` function is evaluated immediately and when its signals change. It
updates the existing text node; it does not rebuild the tree. Use it only on a
`text` element without children. `onClick` accepts a no-argument function only
on `button`. A `text_field` accepts `onInput={(value) => ...}` on desktop QuickJS. Other dynamic props, function children and reconciliation are not supported. An effect is disposed when its node is
removed through the session host. Retire a session by calling `JsSession::dispose` with its host before dropping it.
For ordinary cleanup errors it tries the remaining cleanups, reports the first error,
and clears their registrations. An execution interruption can stop cleanup early. Drop alone cannot run host-backed cleanup.

Call `mount(element, tekto, { createEffect })`, importing `createEffect` from
`@tektogui/core`. Reactive mounts require `tekto.remove` for rollback. When
mounting fails, created nodes are removed in reverse order where the host
provides `remove`; cleanup failures do not replace the original mount error.
Static hosts without `remove` cannot roll back a failed mount and should discard
their candidate tree.

With QuickJS enabled, `tekto dev --js path/to/app.tsx` bundles an exported
`App` (or default function) and supplies `createEffect` to `mount`. A completed
pointer click, Enter or Space on a focused button, and `ui.click` all deliver
one click to the current session. Callback errors are logged; agent clicks
also return the error. The tree is redrawn even if a callback throws.

A Rust embedder can use `create_handle`, `install_js_runtime`, and
`run_windowed_with_handle` to run a retained session without a dev server.
Mount a bundled IIFE with the owner before starting the window, and shut down
the owner when finished. Handle clicks and patches are ordered with mounts;
a successful reload replaces callbacks and resets signal state, while a failed
reload keeps the previous session. Do not synchronously reenter handle actions
from Rust event or notification hooks: the action gate is held during delivery.
Direct concurrent mutation or replacement through the raw tree Arc during an
active JS session is unsupported and bypasses this ordering.

This surface is desktop QuickJS only. A host without registration and disposal
hooks throws when mounting it, including `renderToOps` and the current web
host. Web parity is not claimed.

Each evaluation, callback, node cleanup, and session disposal has a fresh one-second
QuickJS execution budget. The engine periodically checks a monotonic deadline and
interrupts JavaScript when it expires; the caller receives a budget error and click
failures are logged. The budget is fixed in `crates/tekto-js/src/engine.rs`, with
no runtime configuration. A failed reload retains the previous session. After an
interrupted callback the session remains available, but changes already made by the
callback are not rolled back. Interrupted cleanup may leave registrations or effects
unfinished; retiring the session drops the remaining JavaScript state.

The deadline is checked only while QuickJS executes. Native Rust host operations and
lock waits cannot generally be preempted, so this is not a strict wall-clock limit.
Run only trusted app code.

A field's `onInput` receives its complete committed string once after each edit
action, including desktop typing, agent type, deletion, and IME commit. A
multi-character insertion or commit produces one callback with the final value.
Preedit, caret movement, patches, disabled fields, and edits that leave the
value unchanged do not call it. The tree lock is released before delivery; the
action gate keeps delivery ordered with reloads and other actions. Callbacks
use the same one-second budget, error logging, redraw, and session lifecycle
as `onClick`. Agent actions return callback errors; desktop events log them. Controlled inputs and web
callbacks are not supported.
