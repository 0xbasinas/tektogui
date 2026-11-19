/** Solid-style signals. No DOM. */

let current = null;
/** Run without subscribing the active effect to reads inside the callback. */
export function untrack(fn) {
  const previous = current;
  current = null;
  try { return fn(); } finally { current = previous; }
}
const keyed = new Map();
const restored = globalThis.__tektoReloadState ?? {};
const reloadReport = { restored: [], reset: [], removed: [] };
const typeOf = (value) => value === null ? "null" : Array.isArray(value) ? "array" : typeof value;
function jsonValue(value) {
  const seen = new Set();
  function validate(item) {
    if (item === null || typeof item === "string" || typeof item === "boolean") return;
    if (typeof item === "number" && Number.isFinite(item)) return;
    if (typeof item !== "object" || seen.has(item)) throw new TypeError("keyed signal value must be JSON serializable");
    if (!Array.isArray(item) && Object.getPrototypeOf(item) !== Object.prototype && Object.getPrototypeOf(item) !== null)
      throw new TypeError("keyed signal value must use plain objects");
    seen.add(item);
    if (Array.isArray(item)) {
      for (let i = 0; i < item.length; i++) {
        if (!Object.prototype.hasOwnProperty.call(item, i))
          throw new TypeError("keyed signal value must not contain sparse arrays");
      }
    }
    for (const child of Object.values(item)) validate(child);
    seen.delete(item);
  }
  validate(value);
  return JSON.parse(JSON.stringify(value));
}
function compatible(saved, initial) {
  if (typeOf(saved) !== typeOf(initial)) return false;
  if (typeOf(initial) === "object") {
    const left = Object.keys(saved).sort(), right = Object.keys(initial).sort();
    return left.length === right.length && left.every((key, index) => key === right[index] && compatible(saved[key], initial[key]));
  }
  if (Array.isArray(initial) && initial.length && saved.length)
    return saved.every((item) => compatible(item, initial[0]));
  return true;
}
globalThis.__tektoSnapshotSignals = () => {
  const state = Object.create(null);
  for (const [key, get] of keyed) state[key] = jsonValue(get());
  return JSON.stringify(state);
};
globalThis.__tektoSignalReloadReport = () => {
  reloadReport.removed = Object.keys(restored).filter((key) => !keyed.has(key));
  return JSON.stringify(reloadReport);
};

export function createSignal(value, options = {}) {
  const subs = new Set();
  const key = options.key;
  let v = value;
  if (key !== undefined) {
    if (typeof key !== "string" || !key.length) throw new TypeError("signal key must be a non-empty string");
    if (keyed.has(key)) throw new Error(`duplicate signal key: ${key}`);
    jsonValue(value);
    if (Object.prototype.hasOwnProperty.call(restored, key)) {
      if (compatible(restored[key], value)) {
        v = restored[key];
        reloadReport.restored.push(key);
      } else reloadReport.reset.push(key);
    } else reloadReport.reset.push(key);
  }
  const get = () => {
    if (current) {
      subs.add(current);
      current.dependencies.add(subs);
    }
    return v;
  };
  const set = (next) => {
    const updated = typeof next === "function" ? next(v) : next;
    if (key !== undefined) jsonValue(updated);
    v = updated;
    for (const sub of [...subs]) sub();
  };
  if (key !== undefined) keyed.set(key, get);
  return [get, set];
}

/**
 * Host op for a quiet style tween. The Rust tree samples the current
 * value and the desktop loop advances time. `props` may set `opacity`,
 * `x`, `y`, `width`, `height`, `fill`, `color` (hex strings),
 * `easing` (`smoothstep`, `linear`, `ease-in`, `ease-out`), `repeat`,
 * and `durationMs` (default 180 in Rust).
 */
export function transitionOp(id, props = {}) {
  const op = { op: "transition", id };
  for (const key of ["opacity", "x", "y", "width", "height"]) {
    if (props[key] != null) op[key] = String(props[key]);
  }
  for (const key of ["fill", "color", "easing"]) {
    if (props[key] != null) op[key] = String(props[key]);
  }
  if (props.repeat != null) op.repeat = Boolean(props.repeat);
  if (props.durationMs != null) op.durationMs = props.durationMs;
  return op;
}

/**
 * Host op for a partial design-token set. Keys match `TokenPatch`:
 * `page`, `ink`, `banner`, `onBanner`, `field`, `border`, `focus` (hex
 * strings), and `gap`, `padding`, `bodySize`, `titleSize` (numbers).
 * Existing nodes keep their resolved colours; later nodes and the
 * renderer use the new set.
 */
export function tokensOp(tokens = {}) {
  return { op: "setTokens", json: JSON.stringify(tokens) };
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
