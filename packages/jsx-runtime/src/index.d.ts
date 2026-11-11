export const Fragment: symbol;
/** Retained keyed list. Each child must produce one Tekto node. */
export function For<T>(props: { each: () => T[]; by: (item: T) => string | number; children: (item: () => T) => TektoElement }): TektoElement;
/** Keep the current branch mounted until the condition changes truthiness. */
export function Show<T>(props: { when: () => T; children: TektoElement | ((value: () => NonNullable<T>) => TektoElement); fallback?: TektoElement | null }): TektoElement;

export type ComponentProp = {
  name: string;
  type: string;
  description?: string;
  jsxType?: string;
  required?: boolean;
};
export type ComponentEvent = {
  name: string;
  description?: string;
};
export type ComponentSpec = {
  name: string;
  description?: string;
  props: ComponentProp[];
  jsxProps?: ComponentProp[];
  events: ComponentEvent[];
};
export type ComponentSchema = {
  version: number;
  description?: string;
  propAliases?: Record<string, string>;
  components: ComponentSpec[];
};
/** Kinds, props, and events. Same JSON as the MCP tool `app.components`. */
export const componentSchema: ComponentSchema;
export type TektoElement = {
  type: string | symbol | ((props: Record<string, unknown>) => TektoElement);
  props: Record<string, unknown>;
  key: string | number | null;
};
export function jsx(type: TektoElement["type"], props: Record<string, unknown>, key?: string | number | null): TektoElement;
export function jsxs(type: TektoElement["type"], props: Record<string, unknown>, key?: string | number | null): TektoElement;
export function jsxDEV(type: TektoElement["type"], props: Record<string, unknown>, key?: string | number | null): TektoElement;
export function isElement(value: unknown): value is TektoElement;

export type HostApi = {
  onInput?(id: number, callback: (value: string) => void): void;
  onClick?(id: number, callback: () => void): void;
  onDispose?(id: number, cleanup: () => void): void;
  create(kind: string): number;
  setText(id: number, text: string): void;
  setProp(id: number, key: string, value: string): void;
  append(parent: number, child: number): void;
  remove?(id: number): void;
  openOverlay?(id: number, anchor: number, side: "above" | "below" | "left" | "right" | "center"): void;
  closeOverlay?(): void;
  bindTabPanel?(tab: number, panel: number): void;
  bindContextMenu?(target: number, menu: number): void;
  /**
   * Desktop QuickJS only. `spec` is `nativeOpJson(...)` from `@tektogui/core`.
   * Returns a string, or null when the op has no value. Throws when the
   * call is headless (`not available`) or not built (`not implemented`).
   */
  applyNativeOp?(spec: string): string | null;
};
export type HostOpJson =
  | { op: "create"; kind: string }
  | { op: "setText"; id: number; text: string }
  | { op: "setProp"; id: number; key: string; value: string }
  | { op: "append"; parent: number; child: number }
  | { op: "remove"; id: number }
  | { op: "openOverlay"; id: number; anchor: number; side: "above" | "below" | "left" | "right" | "center" }
  | { op: "closeOverlay" }
  | { op: "bindTabPanel"; tab: number; panel: number }
  | { op: "bindContextMenu"; target: number; menu: number };
export function mount(element: TektoElement, tekto: HostApi, options?: { createEffect: (fn: () => void) => { dispose(): void }; createSignal?: <T>(value: T) => [() => T, (value: T | ((previous: T) => T)) => void]; untrack?: <T>(fn: () => T) => T }): number | null;
export function renderToOps(element: TektoElement): HostOpJson[];

/** JSX intrinsic declarations are generated from components.json. */
export { JSX } from "./jsx-types.js";
