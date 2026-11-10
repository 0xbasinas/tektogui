/** TypeScript + JSX sketch of this app.
 *
 * The JSX runtime is not yet wired to the Rust tree (Month 2).
 * This file is here so the UI can be designed in the open.
 */

import { createSignal } from "@tektogui/core";

export function App() {
  const [greeting] = createSignal("Hello from Tekto");
  return (
    <window title="{{title}}" width={640} height={400}>
      <rect name="banner">
        <text fontSize={22}>{{title}}</text>
      </rect>
      <text fontSize={20}>{greeting()}</text>
    </window>
  );
}
