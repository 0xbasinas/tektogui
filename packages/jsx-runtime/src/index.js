/** JSX factory. Returns a plain element tree, never a DOM node. */

import schema from "./components.json" with { type: "json" };

/** Same document `app.components` returns. Kinds, props, and events. */
export const componentSchema = schema;

export const Fragment = Symbol.for("tekto.fragment");

/** A keyed list. `each` is an accessor; `children` receives an item accessor. */
export function For(props) { return { type: FOR, props, key: null }; }
const FOR = Symbol.for("tekto.for");

/** A conditional whose active branch stays mounted while its truthiness is unchanged. */
export function Show({ when, children, fallback = null }) {
  if (typeof when !== "function") throw new TypeError("Show requires a when accessor");
  return jsx("group", { children: [
    jsx(For, { each: () => { const value = when(); return value ? [value] : []; },
      by: () => "shown", children: value => typeof children === "function" ? children(value) : children }),
    ...(fallback == null ? [] : [jsx(For, { each: () => when() ? [] : [true],
      by: () => "fallback", children: () => fallback })]),
  ] });
}

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
  const runtime = { ...options, rootId: null, pendingPanels: [], mounting: true };
  runtime.flushPanels = () => {
    if (runtime.mounting) return;
    while (runtime.pendingPanels.length) {
      const [tab, panel] = runtime.pendingPanels.shift();
      if (runtime.rootId == null || typeof tekto.bindTabPanel !== "function") throw new Error("tab panel requires host support and a root");
      tekto.append(runtime.rootId, panel);
      tekto.bindTabPanel(tab, panel);
    }
  };
  try {
    const root = walk(element, null, tekto, runtime, created);
    runtime.mounting = false;
    runtime.flushPanels();
    return root;
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
  if (node.type === FOR) {
    if (parentId == null) throw new Error("For requires a parent");
    if (typeof options.createEffect !== "function" || typeof options.createSignal !== "function" || typeof options.untrack !== "function" ||
        typeof tekto.remove !== "function" || typeof tekto.onDispose !== "function") {
      throw new Error("For requires createEffect, createSignal, untrack, remove and onDispose");
    }
    const { each, by, children } = node.props;
    if (typeof each !== "function" || typeof by !== "function" || typeof children !== "function") {
      throw new TypeError("For requires each, by and children functions");
    }
    const group = tekto.create("group");
    created.push(group);
    tekto.append(parentId, group);
    let entries = new Map();
    const effect = options.createEffect(() => {
      const items = each();
      if (!Array.isArray(items)) throw new TypeError("For each must return an array");
      const keys = items.map(by);
      if (keys.some(key => typeof key !== "string" && !(typeof key === "number" && Number.isFinite(key)))) {
        throw new TypeError("For keys must be strings or finite numbers");
      }
      if (new Set(keys).size !== keys.length) throw new Error("For keys must be unique");
      const next = new Map(), added = [];
      const pendingStart = options.pendingPanels.length;
      try {
        for (let i = 0; i < items.length; i++) {
          const key = keys[i], item = items[i];
          let entry = entries.get(key);
          if (!entry) {
            const [get, set] = options.createSignal(item);
            const fresh = [];
            try {
              const id = options.untrack(() => walk(children(get), group, tekto, options, fresh));
              if (id == null) throw new Error("For child must have one root node");
              entry = { id, set };
              added.push(...fresh);
            } catch (error) {
              for (const id of fresh.reverse()) { try { tekto.remove(id); } catch {} }
              throw error;
            }
          }
          next.set(key, entry);
        }
      } catch (error) {
        options.pendingPanels.length = pendingStart;
        for (const id of added.reverse()) { try { tekto.remove(id); } catch {} }
        throw error;
      }
      for (const [key, entry] of entries) if (!next.has(key)) tekto.remove(entry.id);
      for (let i = 0; i < items.length; i++) {
        const entry = next.get(keys[i]);
        entry.set(() => items[i]);
        tekto.append(group, entry.id);
      }
      entries = next;
      options.flushPanels?.();
    });
    try { tekto.onDispose(group, effect.dispose); } catch (error) { effect.dispose(); throw error; }
    return group;
  }
  if (typeof node.type === "function") {
    return walk(node.type({ ...node.props }), parentId, tekto, options, created);
  }
  const kind = node.type;
  if (typeof kind !== "string" || !INTRINSICS.has(kind)) {
    throw new Error(`unknown element type ${String(kind)}`);
  }
  if (kind === "image") {
    const { imageSource, name, title, decorative } = node.props;
    if (!imageSource || typeof imageSource !== "object" || !Number.isInteger(imageSource.width) || !Number.isInteger(imageSource.height) ||
        !(Array.isArray(imageSource.rgba) || imageSource.rgba instanceof Uint8Array)) throw new TypeError("image needs RGBA imageSource");
    if (decorative !== true && !(typeof (name ?? title) === "string" && (name ?? title).trim())) throw new TypeError("image needs a name or decorative=true");
  }
  const id = tekto.create(kind);
  if (options.rootId == null) options.rootId = id;
  created.push(id);
  for (const [key, value] of Object.entries(node.props ?? {})) {
    if (key === "children" || key === "contextMenu" || key === "panel" || value == null) continue;
    if (kind === "image" && key === "imageSource") {
      tekto.setProp(id, key, JSON.stringify({ width: value.width, height: value.height, rgba: Array.from(value.rgba) }));
      continue;
    }
    if (key === "onInput") {
      if ((kind !== "text_field" && kind !== "select") || typeof value !== "function") throw new TypeError("onInput requires a text_field or select and a function");
      if (typeof tekto.onInput !== "function") throw new Error("onInput requires a retained host");
      if (typeof tekto.remove !== "function") throw new Error("onInput requires host remove for rollback");
      tekto.onInput(id, value);
      continue;
    }
    if (key === "onClick") {
      if ((kind !== "button" && kind !== "menu_item" && kind !== "tab") || typeof value !== "function") throw new TypeError("onClick requires a button, menu item, or tab and a function");
      if (typeof tekto.onClick !== "function") throw new Error("onClick requires a retained host");
      if (typeof tekto.remove !== "function") throw new Error("onClick requires host remove for rollback");
      tekto.onClick(id, value);
      continue;
    }
    if (key === "text" && typeof value === "function") {
      if (kind !== "text") throw new TypeError("reactive text requires a text element");
      if (typeof tekto.onDispose !== "function" || typeof tekto.setText !== "function") throw new Error("reactive text requires a retained host");
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
  if (node.props.panel != null) {
    if (kind !== "tab" || !isElement(node.props.panel) || node.props.panel.type !== "tab_panel") throw new TypeError("panel requires a tab_panel element on a tab");
    const panel = walk(node.props.panel, null, tekto, options, created);
    options.pendingPanels.push([id, panel]);
  }
  if (node.props.contextMenu != null) {
    if (!isElement(node.props.contextMenu) || node.props.contextMenu.type !== "menu") throw new TypeError("contextMenu requires a menu element");
    if (typeof tekto.bindContextMenu !== "function") throw new Error("contextMenu requires host support");
    const menu = walk(node.props.contextMenu, null, tekto, options, created);
    tekto.bindContextMenu(id, menu);
  }
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
    bindContextMenu(target, menu) {
      ops.push({ op: "bindContextMenu", target, menu });
    },
    bindTabPanel(tab, panel) {
      ops.push({ op: "bindTabPanel", tab, panel });
    },
  };
  mount(element, tekto);
  return ops;
}
