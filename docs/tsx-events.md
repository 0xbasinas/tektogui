# Desktop TSX events and reactive text

The retained QuickJS session supports the following narrow syntax:

```tsx
import { createSignal } from "@tektogui/core";

export function App() {
  const [count, setCount] = createSignal(0);
  return <window>
    <text text={() => count()} />
    <button onClick={() => setCount(n => n + 1)}>Increment</button>
  </window>;
}
```

The `text` function is evaluated immediately and when its signals change. It
updates the existing text node; it does not rebuild the tree. Use it only on a
`text` element without children. `onClick` accepts a no-argument function only
on `button`. Other dynamic props, function children, text-input callbacks and
reconciliation are not supported. An effect is disposed when its node is
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
