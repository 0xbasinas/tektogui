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

const INTRINSICS = new Set(["window", "group", "rect", "text", "button"]);
const PROPS = new Set(["name", "title", "text", "fill", "color", "fontSize", "font_size"]);

function childrenOf(props) {
  if (!props || props.children == null) return [];
  return Array.isArray(props.children) ? props.children : [props.children];
}

/**
 * Walk a JSX element tree and talk to a host (`tekto.create`, `setText`,
 * `setProp`, `append`). Works with the QuickJS `tekto` global and with
 * a mock used by [`renderToOps`]. No Notes-specific APIs.
 */
export function mount(element, tekto) {
  if (!tekto || typeof tekto.create !== "function") {
    throw new Error("mount needs a host with create / setText / setProp / append");
  }
  return walk(element, null, tekto);
}

function walk(node, parentId, tekto) {
  if (node == null || node === false || node === true) return null;
  if (Array.isArray(node)) {
    for (const child of node) walk(child, parentId, tekto);
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
    for (const child of childrenOf(node.props)) walk(child, parentId, tekto);
    return null;
  }
  if (typeof node.type === "function") {
    return walk(node.type({ ...node.props }), parentId, tekto);
  }
  const kind = node.type;
  if (typeof kind !== "string" || !INTRINSICS.has(kind)) {
    throw new Error(`unknown element type ${String(kind)}`);
  }
  const id = tekto.create(kind);
  for (const [key, value] of Object.entries(node.props ?? {})) {
    if (key === "children" || value == null) continue;
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
  for (const child of elementKids) walk(child, id, tekto);
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
