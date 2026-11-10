/** Notes example (TypeScript + JSX).
 *
 * The TS view of the same app as `main.rs`. Run it with:
 *   cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx
 * Saving this file rebuilds the JS bundle and swaps it into the running
 * QuickJS runtime. The Rust window and dev server stay up.
 */

import { createSignal } from "@tektogui/core";

export function App() {
  const [greeting] = createSignal("Hello from Tekto");
  return (
    <window title="Notes" width={640} height={400}>
      <rect name="banner">
        <text fontSize={22}>Notes</text>
      </rect>
      <text fontSize={20}>{greeting()}</text>
    </window>
  );
}
