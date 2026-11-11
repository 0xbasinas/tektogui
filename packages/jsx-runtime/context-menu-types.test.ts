import { jsx } from "./src/index.js";
const menu = jsx("menu", { name: "Actions" });
const target: import("./src/index.js").JSX.IntrinsicElements["button"] = { contextMenu: menu };
// @ts-expect-error contextMenu must be a Tekto element
const wrong: import("./src/index.js").JSX.IntrinsicElements["button"] = { contextMenu: "menu" };
void [target, wrong];
