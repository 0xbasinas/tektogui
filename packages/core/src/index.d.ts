export declare function createSignal<T>(value: T): [() => T, (value: T | ((prev: T) => T)) => void];
export declare function createEffect(fn: () => void): () => void;
