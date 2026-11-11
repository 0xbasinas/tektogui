const { spawnSync } = require("node:child_process");

const result = spawnSync(process.execPath, [
  "--test", "--test-reporter=tap", "packages/wasm/test.js", "packages/wasm/tsx-test.js",
], { encoding: "utf8", env: { ...process.env, TEKTO_REQUIRE_WASM: "1" } });
if (result.error) throw result.error;
process.stdout.write(result.stdout);
process.stderr.write(result.stderr);
if (/^\s*(?:ok|not ok)\s+\d+.*#\s*SKIP\b/im.test(result.stdout)) {
  console.error("Required WASM tests must not skip.");
  process.exit(1);
}
process.exit(result.status ?? 1);
