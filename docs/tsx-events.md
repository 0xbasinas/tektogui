# TSX events and reactive text (integration in progress)

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
It runs all registered cleanups, reports the first error after trying the rest,
and clears registrations even if cleanup fails. Drop alone cannot run host-backed cleanup.

Call `mount(element, tekto, { createEffect })`, importing `createEffect` from
`@tektogui/core`. Reactive mounts require `tekto.remove` for rollback. When
mounting fails, created nodes are removed in reverse order where the host
provides `remove`; cleanup failures do not replace the original mount error.
Static hosts without `remove` cannot roll back a failed mount and should discard
their candidate tree.

**Desktop action delivery is not wired yet.** The retained session is an
engine API, not a working desktop event path. A host without the registration
and disposal hooks throws when mounting this syntax, including `renderToOps`
and the current web host. Web parity is not claimed. Callback errors are
returned by the session to its caller; the desktop integration must surface
them in its logs and agent responses.
