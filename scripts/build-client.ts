import { resolve } from "node:path";
import { ROOT, output } from "./tooling/process";
import { checkSource } from "./tooling/source";
import { preparePayload, type PayloadOptions } from "./tooling/client-payload";
import { buildNative } from "./tooling/native-build";
import {
  buildClient,
  type Platform,
} from "./tooling/client-build";
import { macReleaseSigning, windowsReleaseSigning } from "./tooling/installer-signing";

const args = process.argv.slice(2),
  option = (name: string) =>
    args.includes(name) ? args[args.indexOf(name) + 1] : undefined;
const platform = (option("--platform") || "macos") as Platform,
  root = option("--root") ? resolve(option("--root")!) : ROOT;
if (!["macos", "windows"].includes(platform))
  throw Error("--platform macos|windows is required");
const releaseSigning = args.includes("--sign-release");
const temporarySigning = args.includes("--sign-temporary");
if (releaseSigning) {
  if (temporarySigning) throw Error("Select release signing or a temporary signature");
  if (platform === "macos") macReleaseSigning();
  else windowsReleaseSigning();
}
checkSource(root);
if (output(["rustc", "--version"]).split(" ")[1] !== "1.96.0")
  throw Error("Rust 1.96.0 is required");
const options: PayloadOptions = {
    nativeServices: args.includes("--with-service-runtime"),
    phoneApps: args.includes("--with-phone-apps"),
    containerHost: args.includes("--with-container-host"),
    androidRuntime: option("--android-runtime"),
    imageRuntime: option("--images-runtime"),
    linuxOcr: option("--linux-ocr"),
    windowsVm: option("--qemu") ? {
      qemu: option("--qemu")!,
      qemuIdentity: option("--qemu-identity")!,
      guest: option("--guest")!,
      guestIdentity: option("--guest-identity")!,
    } : undefined,
};
if (options.nativeServices) await buildNative(root);
await preparePayload(root, options);
console.log(JSON.stringify(await buildClient(platform, root, {
  temporarySigning,
  releaseSigning,
})));
