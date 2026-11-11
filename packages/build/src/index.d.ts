export type BuildTarget = "web" | "desktop";
export function repoRoot(): string;
export function esbuildBin(): string;
export function wrapperSource(entry: string, target?: BuildTarget): string;
export function bundleApp(entry: string, outfile: string, options?: { target?: BuildTarget }): string;
export function esbuildWatchArgs(entry: string, outfile: string): string[];
