// Session-local functions stay in QuickJS; no callback crosses the HostOp boundary.
(() => {
  const clicks = new Map();
  const cleanups = new Map();
  const children = new Map();
  const append = tekto.append;
  const remove = tekto.remove;
  tekto.onClick = (id, fn) => {
    if (typeof fn !== "function") throw new TypeError("onClick needs a function");
    clicks.set(id, fn);
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
      const descendants = children.get(node) || [];
      children.delete(node);
      clicks.delete(node);
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
    const ids = new Set([...children.keys(), ...clicks.keys(), ...cleanups.keys()]);
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
  tekto.remove = (id) => { remove(id); forget(id); };
  globalThis.__tektoForgetNode = forget;
  globalThis.__tektoDispatchClick = (id) => {
    const fn = clicks.get(id);
    if (!fn) return false;
    fn();
    return true;
  };
})();
