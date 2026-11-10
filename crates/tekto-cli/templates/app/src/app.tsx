/** TypeScript + JSX sketch of this app.
 *
 * JSX is lowered to HostOps with `renderToOps` / `mount` from `@tektogui/jsx-runtime`.
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
