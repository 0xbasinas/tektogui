/** Solid-style signals. No DOM. */

let current = null;

export function createSignal(value) {
  const subs = new Set();
  let v = value;
  const get = () => {
    if (current) subs.add(current);
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

export function createEffect(fn) {
  const run = () => {
    current = run;
    try {
      fn();
    } finally {
      current = null;
    }
  };
  run();
  return run;
}
