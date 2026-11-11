#!/usr/bin/env node
/** Thin wrapper around `tekto new`; the templates live in the Rust CLI. */
import { spawnSync } from "node:child_process";
import { statSync, writeSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const out = (message) => writeSync(1, `${message}\n`);
const error = (message) => writeSync(2, `${message}\n`);
const usage = "usage: create-tektogui <name>\nSet TEKTO_ROOT=<checkout> to use a tektogui checkout.";
const args = process.argv.slice(2);
if (args.length === 1 && (args[0] === "--help" || args[0] === "-h")) {
  out(usage);
  process.exit(0);
}
if (args.length !== 1 || args[0].startsWith("-")) {
  error(usage);
  process.exit(1);
}

const isFile = (file) => {
  try {
    return statSync(file).isFile();
  } catch {
    return false;
  }
};
const explicitRoot = process.env.TEKTO_ROOT;
const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(explicitRoot === undefined ? path.join(here, "../../..") : explicitRoot);
const cargoToml = path.join(repoRoot, "Cargo.toml");
if (explicitRoot === "" || !isFile(cargoToml) || !isFile(path.join(repoRoot, "crates/tekto/Cargo.toml"))) {
  if (explicitRoot !== undefined) {
    error(`TEKTO_ROOT=${explicitRoot || ""} does not look like a tektogui checkout`);
  } else {
    error("create-tektogui: could not find a tektogui checkout; set TEKTO_ROOT=<checkout>.");
  }
  process.exit(1);
}
if (!isFile(path.join(repoRoot, "crates/tekto-cli/Cargo.toml"))) {
  error(`create-tektogui: missing crates/tekto-cli/Cargo.toml in ${repoRoot}; set TEKTO_ROOT=<checkout> to a complete checkout.`);
  process.exit(1);
}

const result = spawnSync(
  "cargo",
  ["run", "-q", "--manifest-path", cargoToml, "-p", "tekto-cli", "--", "new", args[0]],
  { stdio: "inherit", cwd: process.cwd(), env: { ...process.env, TEKTO_ROOT: repoRoot } },
);
if (result.error) error(`create-tektogui: could not start cargo: ${result.error.message}`);
process.exit(result.status === null ? 1 : result.status);
