#!/usr/bin/env node
/**
 * Thin wrapper around `tekto new`. Prefer the Rust CLI so the
 * templates stay in one place.
 */
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const name = process.argv[2];
if (!name) {
  console.error("usage: create-tektogui <name>");
  process.exit(1);
}

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const cargoToml = path.join(repoRoot, "Cargo.toml");
const inRepo = existsSync(path.join(repoRoot, "crates/tekto-cli/Cargo.toml")) && existsSync(cargoToml);

if (!inRepo) {
  console.error("create-tektogui: run this from a tektogui checkout, or use `tekto new <name>`.");
  process.exit(1);
}

const result = spawnSync(
  "cargo",
  ["run", "-q", "-p", "tekto-cli", "--", "new", name],
  { stdio: "inherit", cwd: process.cwd(), env: { ...process.env, TEKTO_ROOT: repoRoot } },
);
process.exit(result.status === null ? 1 : result.status);
