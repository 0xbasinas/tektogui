/** Solid-style signals. No DOM. */

let current = null;

export function createSignal(value) {
  const subs = new Set();
  let v = value;
  const get = () => {
    if (current) {
      subs.add(current);
      current.dependencies.add(subs);
    }
    return v;
  };
  const set = (next) => {
    v = typeof next === "function" ? next(v) : next;
    for (const sub of [...subs]) sub();
  };
  return [get, set];
}

/**
 * Host op for a quiet style tween. The Rust tree samples the current
 * value and the desktop loop advances time. `props` may set `opacity`,
 * `x`, `y`, `width`, `height`, and `durationMs` (default 180 in Rust).
 */
export function transitionOp(id, props = {}) {
  const op = { op: "transition", id };
  for (const key of ["opacity", "x", "y", "width", "height"]) {
    if (props[key] != null) op[key] = String(props[key]);
  }
  if (props.durationMs != null) op.durationMs = props.durationMs;
  return op;
}

/**
 * JSON for `tekto.applyNativeOp` on the desktop QuickJS host.
 * `op` is one NativeOp object (`shortcutsRegister`, `clipboardGetText`, …).
 * The host returns a string or null, and throws when the platform
 * cannot run the call (`not available` or `not implemented`).
 */
export function nativeOpJson(op) {
  if (!op || typeof op !== "object" || typeof op.op !== "string" || op.op.length === 0) {
    throw new TypeError("native op needs an op name");
  }
  return JSON.stringify(op);
}

export function createEffect(fn) {
  let active = true;
  const run = () => {
    if (!active) return;
    for (const subs of run.dependencies) subs.delete(run);
    run.dependencies.clear();
    const previous = current;
    current = run;
    try {
      fn();
    } finally {
      current = previous;
    }
  };
  run.dependencies = new Set();
  run.dispose = () => {
    active = false;
    for (const subs of run.dependencies) subs.delete(run);
    run.dependencies.clear();
  };
  try { run(); } catch (error) { run.dispose(); throw error; }
  return run;
}
