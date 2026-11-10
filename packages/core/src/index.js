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
