import { spawn } from "node:child_process";
import { once } from "node:events";
import { ROOT, output } from "./process";
import { buildReleaseTools, entryBytes, readTar } from "./archive";

export type SourceFile = { path: string; data: Buffer; mode: number };
type SourceEntry = { path: string; sha256: string; mode: number };

function source(root: string, ...args: string[]) {
  return output([buildReleaseTools(root), "source", ...args], { cwd: root });
}
export function sourceEntries(root = ROOT): SourceEntry[] {
  return JSON.parse(source(root, "list"));
}

export async function sourceFiles(root = ROOT): Promise<SourceFile[]> {
  const child = spawn(buildReleaseTools(root), ["source", "files"], {
    cwd: root, stdio: ["ignore", "pipe", "inherit"],
  });
  const completed = once(child, "close");
  const files: SourceFile[] = [];
  try {
    for await (const entry of readTar(child.stdout))
      files.push({ path: entry.name, mode: entry.mode, data: await entryBytes(entry) });
    if ((await completed)[0] !== 0) throw Error("Source inventory failed");
    return files;
  } finally {
    if (child.exitCode === null) child.kill();
    await completed;
  }
}

export function checkSource(
  root = ROOT,
  { repository = false, manifest = false } = {},
) {
  source(root, "check", ...(repository ? ["--repository"] : []),
    ...(manifest ? ["--manifest"] : []));
}
