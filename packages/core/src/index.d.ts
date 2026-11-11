/** Tagged native plugin call. The `op` names match Rust `NativeOp`. */
import type { NativeOp } from "./native-ops.js";
export type { NativeOp };

export declare function transitionOp(
  id: number,
  props?: {
    opacity?: number;
    x?: number;
    y?: number;
    width?: number;
    height?: number;
    /** Hex fill, for example `#3a424a`. */
    fill?: string;
    /** Hex text colour. */
    color?: string;
    easing?: "smoothstep" | "linear" | "ease-in" | "ease-out";
    /** Swap endpoints and run again when the tween finishes. */
    repeat?: boolean;
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
  fill?: string;
  color?: string;
  easing?: string;
  repeat?: boolean;
  durationMs?: number;
};

export declare function tokensOp(tokens?: {
  page?: string;
  ink?: string;
  banner?: string;
  onBanner?: string;
  field?: string;
  border?: string;
  focus?: string;
  gap?: number;
  padding?: number;
  bodySize?: number;
  titleSize?: number;
}): { op: "setTokens"; json: string };

export declare function createSignal<T>(value: T, options?: { key?: string }): [() => T, (value: T | ((prev: T) => T)) => void];
export declare function createEffect(fn: () => void): (() => void) & { dispose(): void };

/**
 * JSON text for desktop `tekto.applyNativeOp`. Shortcut register, unregister,
 * and take use `shortcutsRegister`, `shortcutsUnregister`, and
 * `shortcutsTakeActivation`. The host throws on a headless or unsupported call.
 */
export declare function nativeOpJson(op: NativeOp): string;
export function untrack<T>(fn: () => T): T;
