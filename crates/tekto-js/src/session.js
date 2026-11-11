// Session-local functions stay in QuickJS; no callback crosses the HostOp boundary.
(() => {
  const clicks = new Map();
  const inputs = new Map();
  const cleanups = new Map();
  const children = new Map();
  const contextMenus = new Set();
  const append = tekto.append;
  const remove = tekto.remove;
  const openOverlay = tekto.openOverlay;
  const closeOverlay = tekto.closeOverlay;
  const bindContextMenu = tekto.bindContextMenu;
  const bindTabPanel = tekto.bindTabPanel;
  let overlay = null;
  tekto.onClick = (id, fn) => {
    if (typeof fn !== "function") throw new TypeError("onClick needs a function");
    clicks.set(id, fn);
  };
  tekto.onInput = (id, fn) => {
    if (typeof fn !== "function") throw new TypeError("onInput needs a function");
    inputs.set(id, fn);
  };
  tekto.onDispose = (id, fn) => {
    if (typeof fn !== "function") throw new TypeError("onDispose needs a function");
    const list = cleanups.get(id) || [];
    list.push(fn);
    cleanups.set(id, list);
  };
  const forget = (id) => {
    let firstError, failed = false;
    const visit = (node) => {
      if (node === overlay) overlay = null;
      const descendants = children.get(node) || [];
      children.delete(node);
      contextMenus.delete(node);
      clicks.delete(node);
      inputs.delete(node);
      const list = cleanups.get(node) || [];
      cleanups.delete(node);
      for (const ids of children.values()) ids.delete(node);
      for (const child of descendants) visit(child);
      for (const fn of list) {
        try { fn(); } catch (error) { if (!failed) { firstError = error; failed = true; } }
      }
    };
    visit(id);
    if (failed) throw firstError;
  };
  globalThis.__tektoDisposeSession = () => {
    let firstError, failed = false;
    const ids = new Set([...children.keys(), ...clicks.keys(), ...inputs.keys(), ...cleanups.keys()]);
    for (const id of ids) {
      try { forget(id); } catch (error) { if (!failed) { firstError = error; failed = true; } }
    }
    if (failed) throw firstError;
  };
  tekto.append = (parent, child) => {
    append(parent, child);
    for (const ids of children.values()) ids.delete(child);
    if (!children.has(parent)) children.set(parent, new Set());
    children.get(parent).add(child);
  };
  tekto.bindTabPanel = (tab, panel) => {
    bindTabPanel(tab, panel);
    for (const ids of children.values()) ids.delete(panel);
    if (!children.has(tab)) children.set(tab, new Set());
    children.get(tab).add(panel);
  };
  tekto.bindContextMenu = (target, menu) => {
    bindContextMenu(target, menu);
    contextMenus.add(menu);
    if (!children.has(target)) children.set(target, new Set());
    children.get(target).add(menu);
  };
  tekto.openOverlay = (id, anchor, side) => {
    openOverlay(id, anchor, side);
    if (overlay !== null && !contextMenus.has(overlay)) forget(overlay);
    overlay = id;
  };
  tekto.closeOverlay = () => {
    closeOverlay();
    const previous = overlay;
    overlay = null;
    if (previous !== null && !contextMenus.has(previous)) forget(previous);
  };
  tekto.remove = (id) => { remove(id); if (id === overlay) overlay = null; forget(id); };
  globalThis.__tektoForgetNode = forget;
  globalThis.__tektoDispatchInput = (id, value) => {
    const fn = inputs.get(id);
    if (!fn) return false;
    fn(value);
    return true;
  };
  globalThis.__tektoDispatchClick = (id) => {
    const fn = clicks.get(id);
    if (!fn) return false;
    fn();
    return true;
  };
})();
