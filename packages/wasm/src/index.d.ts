/** One HostOp, same JSON the Rust `HostOp` enum accepts. */
export type HostOpJson =
  | { op: "create"; kind: string }
  | { op: "setText"; id: number; text: string }
  | { op: "setProp"; id: number; key: string; value: string }
  | { op: "append"; parent: number; child: number }
  | { op: "remove"; id: number }
  | {
      op: "transition";
      id: number;
      opacity?: string;
      x?: string;
      y?: string;
      width?: string;
      height?: string;
      durationMs?: number;
    };

export interface TektoWasmHost {
  applyOps(ops: HostOpJson[]): number[];
  /** Set font or collection bytes before the module initializes text. */
  setFallbackFont(bytes: Uint8Array): void;
  reset(): void;
  renderPng(width: number, height: number): Uint8Array;
  create(kind: string): number;
  setText(id: number, text: string): void;
  setProp(id: number, key: string, value: string): void;
  append(parent: number, child: number): void;
  remove(id: number): void;
}

export function hostFromExports(exports: WebAssembly.Exports): TektoWasmHost;

/** `source` is wasm bytes, or a URL string fetched with `fetch`. */
export function connectWasm(source: BufferSource | string): Promise<TektoWasmHost>;
