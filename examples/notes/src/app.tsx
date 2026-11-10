/** Notes example (TypeScript + JSX).
 *
 * This file is the TS view of the same app as `main.rs`. The JSX runtime
 * is not yet wired to the Rust tree (that is a Month 2 issue). The source
 * is here so the API can be designed in the open.
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
