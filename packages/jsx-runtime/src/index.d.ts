export const Fragment: symbol;
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
  create(kind: string): number;
  setText(id: number, text: string): void;
  setProp(id: number, key: string, value: string): void;
  append(parent: number, child: number): void;
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
  | { op: "remove"; id: number };
export function mount(element: TektoElement, tekto: HostApi): number | null;
export function renderToOps(element: TektoElement): HostOpJson[];
