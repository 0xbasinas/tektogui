/** Tab list sketch. Connect onClick to application state when choosing a panel. */
export function App() {
  return (
    <window title="Settings" width={480} height={240}>
      <tab_list name="Settings pages">
        <tab checked={true}>General</tab>
        <tab>Advanced</tab>
      </tab_list>
      <text>Choose a settings page.</text>
    </window>
  );
}
