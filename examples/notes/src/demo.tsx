/** Notes demo (TypeScript + JSX): the newest widgets, to try by hand.
 *
 * Same screen as the Rust demo in `main.rs`: tab panels, a single-line Title
 * field with a right-click context menu, and a multiline Body field (select
 * across lines, IME, accessibility text runs). `app.tsx` stays the plain Notes
 * sketch because snapshot tests compare it pixel for pixel.
 *
 *   cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/demo.tsx
 */

import { createSignal } from "@tektogui/core";

export function App() {
  const [greeting] = createSignal("Hello from Tekto");
  const titleMenu = (
    <menu name="Title actions">
      <menu_item>Copy</menu_item>
      <menu_item>Paste</menu_item>
    </menu>
  );
  const editPanel = (
    <tab_panel name="Edit panel">
      <text_field name="Title" contextMenu={titleMenu}>
        Groceries
      </text_field>
      <text_field name="Body" multiline={true} height={120}>
        {"Milk\nEggs\nBread"}
      </text_field>
    </tab_panel>
  );
  const aboutPanel = (
    <tab_panel name="About panel">
      <text fontSize={20} color="#2c333a">
        Switch tabs: the Edit text keeps its state.
      </text>
    </tab_panel>
  );
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
      <tab_list name="Sections">
        <tab checked={true} panel={editPanel}>
          Edit
        </tab>
        <tab panel={aboutPanel}>About</tab>
      </tab_list>
    </window>
  );
}
