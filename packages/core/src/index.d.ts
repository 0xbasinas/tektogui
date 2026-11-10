export declare function transitionOp(
  id: number,
  props?: {
    opacity?: number;
    x?: number;
    y?: number;
    width?: number;
    height?: number;
    durationMs?: number;
  },
): {
  op: "transition";
  id: number;
  opacity?: string;
  x?: string;
  y?: string;
  width?: string;
  height?: string;
  durationMs?: number;
};

export declare function createSignal<T>(value: T): [() => T, (value: T | ((prev: T) => T)) => void];
export declare function createEffect(fn: () => void): () => void;
