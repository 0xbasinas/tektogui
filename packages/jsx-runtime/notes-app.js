/** Notes sketch used to prove JSX lowers to the same Rust tree. Not a public API. */

import { createSignal } from "../core/src/index.js";
import { jsx, jsxs } from "./src/index.js";

export function Notes() {
  const [greeting] = createSignal("Hello from Tekto");
  return jsxs("window", {
    title: "Notes",
    width: 640,
    height: 400,
    fill: "#f4f3f0",
    children: [
      jsxs("rect", {
        name: "banner",
        fill: "#3a424a",
        children: [
          jsx("text", {
            fontSize: 22,
            color: "#f4f3f0",
            children: "Notes",
          }),
        ],
      }),
      jsx("text", {
        fontSize: 20,
        color: "#2c333a",
        children: greeting(),
      }),
    ],
  });
}
