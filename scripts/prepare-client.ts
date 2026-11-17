import { ROOT } from "./tooling/process";
import { preparePayload, stageSource } from "./tooling/client-payload";
import { verifyNative, nativeDatabaseReceipt } from "./tooling/native-check";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
const args = process.argv.slice(2),
  option = (name: string) =>
    args.includes(name) ? args[args.indexOf(name) + 1] : undefined;
console.log(
  JSON.stringify(
    await preparePayload(ROOT, {
      nativeServices: args.includes("--with-service-runtime"),
      phoneApps: args.includes("--with-phone-apps"),
      containerHost: args.includes("--with-container-host"),
      androidRuntime: option("--android-runtime"),
      imageRuntime: option("--images-runtime"),
      linuxOcr: option("--linux-ocr"),
    }),
  ),
);
if (
  args.some((arg) =>
    [
      "--verify-service-runtime",
      "--pg-container",
    ].includes(arg),
  )
) {
  const home = mkdtempSync(join(tmpdir(), "aiphone-payload-check-"));
  try {
    const staged = await stageSource(
      join(ROOT, "apps/native/gen/host/source.payload"),
      home,
      ROOT,
    );
    if (args.includes("--verify-service-runtime"))
      console.log(JSON.stringify(verifyNative(staged)));
    if (args.includes("--pg-container"))
      console.log(
        JSON.stringify(
          nativeDatabaseReceipt(
            staged,
            option("--pg-container")!,
            Number(option("--pg-port")),
          ),
        ),
      );
  } finally {
    rmSync(home, { recursive: true });
  }
}
