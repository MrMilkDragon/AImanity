import { buildWslBackend } from "./tooling/wsl-qualification";
const nativeUi = process.argv.includes("--native-ui");
const result = await buildWslBackend(
  undefined,
  nativeUi || process.argv.includes("--with-device-apps"),
  undefined,
  nativeUi,
);
console.log(JSON.stringify(result.report));
