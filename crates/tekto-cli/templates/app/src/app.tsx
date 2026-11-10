/** TypeScript + JSX sketch of this app.
 *
 * JSX is lowered to HostOps with `renderToOps` / `mount` from `@tektogui/jsx-runtime`.
 * Layout and colors match the Rust Notes widget.
 */

import { createSignal } from "@tektogui/core";

export function App() {
  const [greeting] = createSignal("Hello from Tekto");
  return (
    <window title="{{title}}" width={640} height={400} fill="#f4f3f0" padding={24} gap={20}>
      <rect name="banner" fill="#3a424a" height={64} padding={20} justifyCenter={true}>
        <text fontSize={22} color="#f4f3f0">
          {{title}}
        </text>
      </rect>
      <text fontSize={20} color="#2c333a">
        {greeting()}
      </text>
    </window>
  );
}
