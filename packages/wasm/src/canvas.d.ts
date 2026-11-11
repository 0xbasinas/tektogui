import type { TektoWasmHost } from "./index.js";

/** Straight RGBA pixels from an 8-bit RGB or RGBA PNG. */
export interface DecodedPng {
  width: number;
  height: number;
  data: Uint8ClampedArray;
}

/**
 * Minimal 2d surface. A browser `<canvas>` satisfies this.
 * Declared here so the package does not depend on DOM lib types.
 */
export interface PixelCanvas {
  width: number;
  height: number;
  getContext(kind: "2d"): PixelContext | null;
}

export interface PixelImageData {
  data: Uint8ClampedArray;
  width?: number;
  height?: number;
}

export interface PixelContext {
  createImageData(width: number, height: number): PixelImageData;
  putImageData(data: PixelImageData, x: number, y: number): void;
}

export function decodePng(bytes: BufferSource): Promise<DecodedPng>;

export function blitPng(canvas: PixelCanvas, pngBytes: BufferSource): Promise<DecodedPng>;

/** CPU PNG (`host.renderPng`) blitted onto `canvas`. */
export function paintCanvas(
  host: Pick<TektoWasmHost, "renderPng">,
  canvas: PixelCanvas,
  width?: number,
  height?: number,
): Promise<Uint8Array>;

/** Set the canvas size and paint again. */
export function resizeCanvas(
  host: Pick<TektoWasmHost, "renderPng">,
  canvas: PixelCanvas,
  width: number,
  height: number,
): Promise<Uint8Array>;

/** Structural input surface used by host pages. */
export interface InputCanvas extends PixelCanvas {
  focus?(): void;
  setPointerCapture?(pointerId: number): void;
  getBoundingClientRect(): { left: number; top: number; width: number; height: number };
  addEventListener(type: string, listener: (event: any) => void, options?: { passive: boolean }): void;
  removeEventListener(type: string, listener: (event: any) => void): void;
}

/** Attach input listeners; the returned function removes them. */
export function attachCanvasInput(
  host: Pick<TektoWasmHost, "renderPng" | "clickAt" | "contextMenuAt" | "scrollAt" | "escape" | "typeText" | "backspace" | "sliderBegin" | "sliderMove" | "sliderKey" | "activate">,
  canvas: InputCanvas,
  repaint?: () => void | Promise<unknown>,
): () => void;
