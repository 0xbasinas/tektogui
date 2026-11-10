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
