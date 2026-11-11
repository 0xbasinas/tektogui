// Prefer the installed package; support an uninstalled source checkout too.
const build = await import("@tektogui/build").catch(async error => {
  if (error.code !== "ERR_MODULE_NOT_FOUND") throw error;
  return import("../../build/src/index.js");
});
export const { repoRoot, esbuildBin, wrapperSource, bundleApp, esbuildWatchArgs } = build;
