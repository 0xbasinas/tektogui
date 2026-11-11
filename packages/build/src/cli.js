#!/usr/bin/env node
import { bundleApp } from "./index.js";

const [target, entry, outfile, ...extra] = process.argv.slice(2);
if (!target || !entry || !outfile || extra.length || !["web", "desktop"].includes(target)) {
  process.stderr.write("usage: tekto-build <web|desktop> <entry.tsx> <outfile>\n");
  process.exitCode = 1;
} else {
  try {
    bundleApp(entry, outfile, { target });
    process.stdout.write(`Built ${outfile}\n`);
  } catch (error) {
    process.stderr.write(`tekto-build: ${error.message}\n`);
    process.exitCode = 1;
  }
}
