/** Mount the app first; mount this subtree and open it with host.openOverlay(id, windowId, "center"). */
export function ConfirmDialog() {
  return <dialog name="Confirm"><text>Continue?</text><button>Cancel</button></dialog>;
}
