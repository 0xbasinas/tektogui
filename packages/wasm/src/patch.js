/**
 * Apply one `ui.patch` / `POST /patch` body to a wasm host.
 *
 * Same fields as the desktop dev server: `id` plus optional `text`,
 * `name`, `fill`, `color`, `fontSize` (or `font_size`), `padding`, `gap`, `disabled`.
 * No DOM types. The wasm ABI does not return a node dump, so the result
 * is `{ ok, id }` rather than the desktop `{ ok, node, writeback }`.
 */

const PROP_KEYS = ["disabled", "name", "fill", "color", "fontSize", "padding", "gap"];

export function applyUiPatch(host, patch) {
  if (!host || typeof host.setText !== "function" || typeof host.setProp !== "function") {
    throw new Error("applyUiPatch needs a wasm host");
  }
  if (!patch || typeof patch !== "object") {
    throw new Error("missing patch");
  }
  const id = patch.id;
  if (typeof id !== "number" || !Number.isFinite(id)) {
    throw new Error("missing id");
  }
  if (patch.text != null) host.setText(id, String(patch.text));
  for (const key of PROP_KEYS) {
    let value = patch[key];
    if (value == null && key === "fontSize") value = patch.font_size;
    if (value != null) host.setProp(id, key, String(value));
  }
  return { ok: true, id };
}
