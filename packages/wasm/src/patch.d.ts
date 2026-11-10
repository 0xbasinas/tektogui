import type { TektoWasmHost } from "./index.js";

/** Same JSON body as desktop `POST /patch` / `ui.patch`. */
export interface UiPatch {
  id: number;
  text?: string;
  name?: string;
  fill?: string;
  color?: string;
  fontSize?: number | string;
  font_size?: number | string;
  padding?: number | string;
  gap?: number | string;
}

export function applyUiPatch(host: TektoWasmHost, patch: UiPatch): { ok: true; id: number };
