/** Notes example (TypeScript + JSX).
 *
 * The TS view of the same app as `main.rs`. Layout and colors match the
 * Rust widget (page fill, banner fill, padding, gap, title color).
 *
 *   cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx
 */

import { createSignal } from "@tektogui/core";

export function App() {
  const [greeting] = createSignal("Hello from Tekto");
  return (
    <window title="Notes" width={640} height={400} fill="#f4f3f0" padding={24} gap={20}>
      <rect name="banner" fill="#3a424a" height={64} padding={20} justifyCenter={true}>
        <text fontSize={22} color="#f4f3f0">
          Notes
        </text>
      </rect>
      <text fontSize={20} color="#2c333a">
        {greeting()}
      </text>
    </window>
  );
}
