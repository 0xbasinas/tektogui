export const Fragment: symbol;

export type ComponentProp = {
  name: string;
  type: string;
  description?: string;
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
  onClick?(id: number, callback: () => void): void;
  onDispose?(id: number, cleanup: () => void): void;
  create(kind: string): number;
  setText(id: number, text: string): void;
  setProp(id: number, key: string, value: string): void;
  append(parent: number, child: number): void;
  remove?(id: number): void;
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
export function mount(element: TektoElement, tekto: HostApi, options?: { createEffect: (fn: () => void) => { dispose(): void } }): number | null;
export function renderToOps(element: TektoElement): HostOpJson[];

/** JSX props for Tekto nodes. Reactive text is limited to text nodes. */
export namespace JSX {
  type CommonProps = { name?: string; title?: string; width?: number; height?: number; fill?: string; color?: string; fontSize?: number; padding?: number; gap?: number; column?: boolean; justifyCenter?: boolean; children?: unknown };
  type StaticProps = CommonProps & { text?: string | number };
  interface IntrinsicElements {
    text: CommonProps & { text?: string | number | (() => string | number) };
    button: StaticProps & { disabled?: boolean; onClick?: () => void };
    window: StaticProps; group: StaticProps; rect: StaticProps;
    text_field: StaticProps & { disabled?: boolean }; scroll: StaticProps; list: StaticProps; list_item: StaticProps;
  }
}
