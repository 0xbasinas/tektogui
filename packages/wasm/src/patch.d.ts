import type { TektoWasmHost } from "./index.js";

/** Same JSON body as desktop `POST /patch` / `ui.patch`. */
export interface UiPatch {
  id: number;
  text?: string;
  disabled?: boolean;
  value?: number;
  name?: string;
  fill?: string;
  color?: string;
  fontSize?: number | string;
  font_size?: number | string;
  radius?: number | string;
  borderColor?: string;
  border_color?: string;
  borderWidth?: number | string;
  border_width?: number | string;
  padding?: number | string;
  gap?: number | string;
}

export function applyUiPatch(host: TektoWasmHost, patch: UiPatch): { ok: true; id: number };
