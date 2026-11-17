import { buildWslBackend, prepareWindowsReleaseContext } from "./tooling/wsl-qualification";

const args = Bun.argv.slice(2);
if (!args.length) {
  const result = await buildWslBackend(undefined, true);
  console.log(JSON.stringify(result.report));
} else if (args.length === 2 && args[0] === "--release-layer" && args[1]) {
  await prepareWindowsReleaseContext(args[1]);
} else {
  throw Error("Usage: wsl-backend.ts [--release-layer <context>]");
}
