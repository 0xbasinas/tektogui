import { execFileSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

test("input declarations type-check", () => {
  const tsc = fileURLToPath(new URL("../../node_modules/typescript/bin/tsc", import.meta.url));
  const fixture = fileURLToPath(new URL("./input-types.test.ts", import.meta.url));
  const contextMenu = fileURLToPath(new URL("./context-menu-types.test.ts", import.meta.url));
  execFileSync(process.execPath, [tsc, "--strict", "--noEmit", "--module", "nodenext", "--moduleResolution", "nodenext", fixture, contextMenu], { stdio: "inherit" });
});

test("generated JSX declarations match the component schema", () => {
  const generator = fileURLToPath(new URL("./scripts/generate-jsx-types.js", import.meta.url));
  execFileSync(process.execPath, [generator, "--check"], { stdio: "inherit" });
});
