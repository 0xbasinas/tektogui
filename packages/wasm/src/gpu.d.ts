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
}

export function choosePaintBackend(probe: PaintProbe): PaintChoice;

export function detectWebGpu(env: { gpu?: { requestAdapter?: () => Promise<unknown> } } | null | undefined): Promise<boolean>;

export function detectWebGl2(canvas: { getContext?: (kind: string) => unknown } | null | undefined): boolean;

export function paintPreferred(
  host: Pick<TektoWasmHost, "renderPng"> & {
    paintWebGpu?: (canvas: ProbeCanvas, width: number, height: number) => Promise<unknown>;
  },
  canvas: ProbeCanvas,
  options?: {
    gpu?: boolean;
    width?: number;
    height?: number;
    env?: { gpu?: { requestAdapter?: () => Promise<unknown> } };
    detect?: { webgpu?: boolean; webgl2?: boolean };
  },
): Promise<PaintResult>;
