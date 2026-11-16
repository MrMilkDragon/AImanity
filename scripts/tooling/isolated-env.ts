import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

export function isolatedEnvironment(
  root: string,
  original = process.env,
): NodeJS.ProcessEnv {
  root = resolve(root);
  if (
    existsSync(join(root, ".env")) ||
    (existsSync(join(root, "data")) && readdirSync(join(root, "data")).length)
  )
    throw Error("Use an isolated runtime without user state");
  const allowed = new Set([
    "HOME",
    "USERPROFILE",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "LANG",
  ]);
  return {
    ...Object.fromEntries(
      Object.entries(original).filter(([key]) =>
        allowed.has(key.toUpperCase()),
      ),
    ),
    PATH:
      process.platform === "win32"
        ? (original.SystemRoot || "C:\\Windows") + "\\System32"
        : "/bin:/usr/bin",
    AI_PHONE_PROJECT_ROOT: root,
  };
}
