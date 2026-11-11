#!/usr/bin/env node
/** Thin wrapper around `tekto new`; the templates live in the Rust CLI. */
import { spawnSync } from "node:child_process";
import { realpathSync, statSync, writeSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const usage = "usage: create-tektogui <name>\nSet TEKTO_ROOT=<checkout> to use a tektogui checkout.";
const isFile = (file) => {
  try { return statSync(file).isFile(); } catch { return false; }
};

/** Run the wrapper with injectable process I/O for platform-independent tests. */
export function main({ args = process.argv.slice(2), env = process.env, cwd = process.cwd(),
  here = path.dirname(fileURLToPath(import.meta.url)), launch = spawnSync,
  out = message => writeSync(1, `${message}\n`), error = message => writeSync(2, `${message}\n`) } = {}) {
  if (args.length === 1 && (args[0] === "--help" || args[0] === "-h")) {
    out(usage); return 0;
  }
  if (args.length !== 1 || args[0].startsWith("-")) {
    error(usage); return 1;
  }
  const explicitRoot = env.TEKTO_ROOT;
  const repoRoot = path.resolve(cwd, explicitRoot === undefined ? path.join(here, "../../..") : explicitRoot);
  const cargoToml = path.join(repoRoot, "Cargo.toml");
  if (explicitRoot === "" || !isFile(cargoToml) || !isFile(path.join(repoRoot, "crates/tekto/Cargo.toml"))) {
    if (explicitRoot !== undefined) error(`TEKTO_ROOT=${explicitRoot || ""} does not look like a tektogui checkout`);
    else error("create-tektogui: could not find a tektogui checkout; set TEKTO_ROOT=<checkout>.");
    return 1;
  }
  if (!isFile(path.join(repoRoot, "crates/tekto-cli/Cargo.toml"))) {
    error(`create-tektogui: missing crates/tekto-cli/Cargo.toml in ${repoRoot}; set TEKTO_ROOT=<checkout> to a complete checkout.`);
    return 1;
  }
  const result = launch("cargo", ["run", "-q", "--manifest-path", cargoToml, "-p", "tekto-cli", "--", "new", args[0]],
    { stdio: "inherit", cwd, env: { ...env, TEKTO_ROOT: repoRoot } });
  if (result.error) error(`create-tektogui: could not start cargo: ${result.error.message}`);
  return result.status === null ? 1 : result.status;
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  process.exit(main());
}
