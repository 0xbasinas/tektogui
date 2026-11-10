import type { TektoWasmHost } from "./index.js";
import type { PixelCanvas } from "./canvas.js";

export interface PaintProbe {
  gpuFlag?: boolean;
  webgpu?: boolean;
  webgl2?: boolean;
}

export interface PaintChoice {
  backend: "webgpu" | "cpu";
  reason: string;
}

/** Structural canvas. A browser `<canvas>` satisfies this. No DOM lib. */
export interface ProbeCanvas extends PixelCanvas {
  getContext(kind: string): unknown;
}

export interface PaintResult {
  backend: "webgpu" | "cpu";
  reason: string;
  used: "webgpu" | "cpu";
  png?: Uint8Array;
  error?: string;
  /** Canvas that received the frame. A failed GPU context is replaced in place. */
  surface?: ProbeCanvas;
}

export function choosePaintBackend(probe: PaintProbe): PaintChoice;

export function detectWebGpu(env: { gpu?: { requestAdapter?: () => Promise<unknown> } } | null | undefined): Promise<boolean>;

export function detectWebGl2(canvas: { getContext?: (kind: string) => unknown } | null | undefined): boolean;

export function detachedCanvas(env?: { document?: { createElement: (tag: string) => ProbeCanvas } } | null): ProbeCanvas | null;

export function replacePaintCanvas(canvas: ProbeCanvas & {
  ownerDocument?: { createElement: (tag: string) => ProbeCanvas };
  parentNode?: { replaceChild: (next: ProbeCanvas, prev: ProbeCanvas) => void };
  id?: string;
  className?: string;
}): ProbeCanvas | null;

export function paintPreferred(
  host: Pick<TektoWasmHost, "renderPng"> & {
    paintWebGpu?: (canvas: ProbeCanvas, width: number, height: number) => Promise<unknown>;
  },
  canvas: ProbeCanvas,
  options?: {
    gpu?: boolean;
    width?: number;
    height?: number;
    env?: {
      gpu?: { requestAdapter?: () => Promise<unknown> };
      document?: { createElement: (tag: string) => ProbeCanvas };
    };
    detect?: { webgpu?: boolean; webgl2?: boolean };
  },
): Promise<PaintResult>;
