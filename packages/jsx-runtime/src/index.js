/** JSX factory. Returns a plain element tree, never a DOM node. */

import schema from "./components.json" with { type: "json" };

/** Same document `app.components` returns. Kinds, props, and events. */
export const componentSchema = schema;

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

const INTRINSICS = new Set(schema.components.map((c) => c.name));
const PROPS = new Set(
  schema.components.flatMap((c) => c.props.map((p) => p.name)).concat(Object.keys(schema.propAliases ?? {})),
);

function childrenOf(props) {
  if (!props || props.children == null) return [];
  return Array.isArray(props.children) ? props.children : [props.children];
}

/**
 * Walk a JSX element tree and talk to a host (`tekto.create`, `setText`,
 * `setProp`, `append`). Works with the QuickJS `tekto` global and with
 * a mock used by [`renderToOps`]. No Notes-specific APIs.
 */
export function mount(element, tekto, options = {}) {
  if (!tekto || typeof tekto.create !== "function") {
    throw new Error("mount needs a host with create / setText / setProp / append");
  }
  const created = [];
  try {
    return walk(element, null, tekto, options, created);
  } catch (error) {
    if (typeof tekto.remove === "function") {
      for (const id of created.reverse()) {
        try { tekto.remove(id); } catch { /* Preserve the mount error. */ }
      }
    }
    throw error;
  }
}

function walk(node, parentId, tekto, options, created) {
  if (node == null || node === false || node === true) return null;
  if (Array.isArray(node)) {
    for (const child of node) walk(child, parentId, tekto, options, created);
    return null;
  }
  if (typeof node === "string" || typeof node === "number") {
    if (parentId != null && tekto.setText) tekto.setText(parentId, String(node));
    return null;
  }
  if (!isElement(node)) {
    throw new Error("mount expected a Tekto element");
  }
  if (node.type === Fragment) {
    for (const child of childrenOf(node.props)) walk(child, parentId, tekto, options, created);
    return null;
  }
  if (typeof node.type === "function") {
    return walk(node.type({ ...node.props }), parentId, tekto, options, created);
  }
  const kind = node.type;
  if (typeof kind !== "string" || !INTRINSICS.has(kind)) {
    throw new Error(`unknown element type ${String(kind)}`);
  }
  const id = tekto.create(kind);
  created.push(id);
  for (const [key, value] of Object.entries(node.props ?? {})) {
    if (key === "children" || value == null) continue;
    if (key === "onInput") {
      if (kind !== "text_field" || typeof value !== "function") throw new TypeError("onInput requires a text_field and a function");
      if (typeof tekto.onInput !== "function") throw new Error("onInput requires a retained QuickJS host");
      if (typeof tekto.remove !== "function") throw new Error("onInput requires host remove for rollback");
      tekto.onInput(id, value);
      continue;
    }
    if (key === "onClick") {
      if (kind !== "button" || typeof value !== "function") throw new TypeError("onClick requires a button and a function");
      if (typeof tekto.onClick !== "function") throw new Error("onClick requires a retained QuickJS host");
      if (typeof tekto.remove !== "function") throw new Error("onClick requires host remove for rollback");
      tekto.onClick(id, value);
      continue;
    }
    if (key === "text" && typeof value === "function") {
      if (kind !== "text") throw new TypeError("reactive text requires a text element");
      if (typeof tekto.onDispose !== "function" || typeof tekto.setText !== "function") throw new Error("reactive text requires a retained QuickJS host");
      if (childrenOf(node.props).length) throw new Error("reactive text cannot have children");
      if (typeof options.createEffect !== "function" || typeof tekto.remove !== "function") throw new Error("reactive text requires createEffect and host remove for rollback");
      const effect = options.createEffect(() => tekto.setText(id, String(value())));
      try { tekto.onDispose(id, effect.dispose); } catch (error) { effect.dispose(); throw error; }
      continue;
    }
    if (key === "title" && tekto.setProp) {
      tekto.setProp(id, "name", String(value));
      continue;
    }
    if (PROPS.has(key) && tekto.setProp) {
      tekto.setProp(id, key, String(value));
    }
  }
  const kids = childrenOf(node.props);
  const textKids = kids.filter((c) => typeof c === "string" || typeof c === "number");
  const elementKids = kids.filter(
    (c) => !(typeof c === "string" || typeof c === "number" || c == null || c === false || c === true),
  );
  if (textKids.length && tekto.setText) {
    tekto.setText(id, textKids.map((c) => String(c)).join(""));
  }
  for (const child of elementKids) walk(child, id, tekto, options, created);
  if (parentId != null && tekto.append) tekto.append(parentId, id);
  return id;
}

/**
 * Lower a JSX tree to the `HostOp` JSON list that `tekto-js` applies
 * to a Rust scene tree. Ids start at 1, same as an empty `Tree`.
 */
export function renderToOps(element) {
  const ops = [];
  let nextId = 1;
  const tekto = {
    create(kind) {
      const id = nextId++;
      ops.push({ op: "create", kind });
      return id;
    },
    setText(id, text) {
      ops.push({ op: "setText", id, text });
    },
    setProp(id, key, value) {
      ops.push({ op: "setProp", id, key, value: String(value) });
    },
    append(parent, child) {
      ops.push({ op: "append", parent, child });
    },
  };
  mount(element, tekto);
  return ops;
}
