/** JSX factory. Returns a plain element tree, never a DOM node. */

export const Fragment = Symbol.for("tekto.fragment");

export function jsx(type, props, key) {
  return { type, props: props ?? {}, key: key ?? null };
}

export function jsxs(type, props, key) {
  return jsx(type, props, key);
}

export function jsxDEV(type, props, key) {
  return jsx(type, props, key);
}

export function isElement(value) {
  return value !== null && typeof value === "object" && "type" in value && "props" in value;
}
