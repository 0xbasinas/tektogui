/** A labeled set of mutually exclusive options. */
export function Settings() {
  return <window title="Settings"><radio_group name="Size"><radio checked={true}>Small</radio><radio>Large</radio></radio_group></window>;
}
