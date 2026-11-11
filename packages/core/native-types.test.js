import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

test("generated native op types match the source list", () => {
  const script = fileURLToPath(new URL("../../scripts/generate-native-types.mjs", import.meta.url));
  const output = execFileSync(process.execPath, [script, "--check"], { encoding: "utf8" });
  assert.equal(output, "");
});

test("a mistyped native call is a TypeScript error", (t) => {
  const tsc = fileURLToPath(new URL("../../node_modules/typescript/bin/tsc", import.meta.url));
  if (!existsSync(tsc)) {
    t.skip("TypeScript is not installed");
    return;
  }
  const declarations = fileURLToPath(new URL("./src/index.d.ts", import.meta.url));
  const dir = mkdtempSync(join(tmpdir(), "tekto-native-types-"));
  try {
    const check = (name, source) => {
      const file = join(dir, name);
      writeFileSync(file, source);
      return spawnSync(process.execPath, [tsc, "--noEmit", "--strict", "--skipLibCheck", file], {
        encoding: "utf8",
      }).status;
    };
    const importLine = `import type { NativeOp } from ${JSON.stringify(declarations)};\n`;
    assert.equal(check("good.ts", `${importLine}const op: NativeOp = { op: "clipboardSetText", text: "hi" };\n`), 0);
    assert.notEqual(check("bad-op.ts", `${importLine}const op: NativeOp = { op: "clipboardSetTxt", text: "hi" };\n`), 0);
    assert.notEqual(check("bad-arg.ts", `${importLine}const op: NativeOp = { op: "clipboardSetText", text: 1 };\n`), 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
