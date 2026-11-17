import { ROOT, output } from "./process";
import { buildReleaseTools } from "./archive";

export type SourceFile = { path: string; data: Buffer; mode: number };
type SourceEntry = { path: string; sha256: string; mode: number };

function source(root: string, ...args: string[]) {
  return output([buildReleaseTools(root), "source", ...args], { cwd: root });
}
export function sourceEntries(root = ROOT): SourceEntry[] {
  return JSON.parse(source(root, "list"));
}

export function checkSource(
  root = ROOT,
  { repository = false, manifest = false } = {},
) {
  source(root, "check", ...(repository ? ["--repository"] : []),
    ...(manifest ? ["--manifest"] : []));
}
