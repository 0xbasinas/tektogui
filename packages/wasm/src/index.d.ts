/** One HostOp, same JSON the Rust `HostOp` enum accepts. */
export type HostOpJson =
  | { op: "create"; kind: string }
  | { op: "setText"; id: number; text: string }
  | { op: "setProp"; id: number; key: string; value: string }
  | { op: "append"; parent: number; child: number }
  | { op: "remove"; id: number }
  | { op: "openOverlay"; id: number; anchor: number; side: "above" | "below" | "left" | "right" | "center" }
  | { op: "closeOverlay" }
  | { op: "bindTabPanel"; tab: number; panel: number }
  | { op: "bindContextMenu"; target: number; menu: number }
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
  clickAt(width: number, height: number, x: number, y: number): number | null;
  contextMenuAt(width: number, height: number, x: number, y: number): number | null;
  fieldBegin(width: number, height: number, x: number, y: number): number | null;
  fieldMove(id: number, x: number, y: number): boolean;
  fieldClick(id: number): void;
  escape(): boolean;
  sliderBegin(width: number, height: number, x: number, y: number): number | null;
  sliderMove(id: number, x: number): boolean;
  sliderKey(key: string): boolean;
  activate(space?: boolean): boolean;
  typeText(text: string): boolean;
  backspace(): boolean;
  fieldIme(kind: "preedit" | "commit" | "cancel", text?: string): boolean;
  fieldKey(key: string, shift?: boolean): boolean;
  fieldSelection(): string | null;
  focusedField(): number | null;
  onClick(id: number, callback: () => void): void;
  onInput(id: number, callback: (value: string) => void): void;
  onDispose(id: number, callback: () => void): void;
  scrollAt(width: number, height: number, x: number, y: number, delta: number): number | null;
  create(kind: string): number;
  setText(id: number, text: string): void;
  setProp(id: number, key: string, value: string): void;
  append(parent: number, child: number): void;
  remove(id: number): void;
  openOverlay(id: number, anchor: number, side: "above" | "below" | "left" | "right" | "center"): void;
  closeOverlay(): void;
  bindTabPanel(tab: number, panel: number): void;
  bindContextMenu(target: number, menu: number): void;
}

export function hostFromExports(exports: WebAssembly.Exports): TektoWasmHost;

/** `source` is wasm bytes, or a URL string fetched with `fetch`. */
export function connectWasm(source: BufferSource | string): Promise<TektoWasmHost>;
