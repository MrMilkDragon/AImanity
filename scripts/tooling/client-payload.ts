import { existsSync, lstatSync, mkdirSync } from "node:fs";
import { join, basename } from "node:path";
import { ROOT, output, run } from "./process";
import { json } from "./inventory";
import { buildReleaseTools } from "./archive";
import { hostResources } from "./resources";
import { stageWindowsVm, type WindowsVmInputs } from "./windows-vm-payload";

export type PayloadOptions = {
  nativeServices?: boolean;
  phoneApps?: boolean;
  containerHost?: boolean;
  androidRuntime?: string;
  imageRuntime?: string;
  linuxOcr?: string;
  windowsVm?: WindowsVmInputs;
};
export async function preparePayload(root = ROOT, options: PayloadOptions = {}) {
  const args = [buildReleaseTools(root), "client-payload"];
  if (options.nativeServices) args.push("--with-service-runtime");
  if (options.phoneApps) args.push("--with-phone-apps");
  if (options.containerHost) args.push("--with-container-host");
  if (options.androidRuntime) args.push("--android-runtime", options.androidRuntime);
  if (options.imageRuntime) args.push("--images-runtime", options.imageRuntime);
  if (options.linuxOcr) args.push("--linux-ocr", options.linuxOcr);
  if (options.windowsVm) {
    const folder = join(root, hostResources);
    mkdirSync(folder, { recursive: true });
    const digest = await stageWindowsVm(root, join(folder, "qemu"), options.windowsVm);
    args.push("--qemu-manifest", digest);
  }
  run(args, { cwd: root });
}
export function payloadComponents(root = ROOT): string[] {
  const folder = join(root, hostResources),
    payload = join(folder, "source.payload");
  if (!existsSync(payload)) return [];
  const components = JSON.parse(output([buildReleaseTools(root), "source", "components", payload], { cwd: root }));
  const integrity = json(join(folder, "integrity.json"));
  return [
    ...components,
    ...("android.payload" in integrity ? ["android-runtime"] : []),
    ...("images.payload" in integrity ? ["container-images"] : []),
    ...("qemu/manifest.json" in integrity ? ["windows-qemu"] : []),
  ];
}
export function installerMetadata(file: string, host: string) {
  const format = file.split(".").at(-1)!.toLowerCase(),
    supported: Record<string, string> = {
      "macos-arm64": "dmg",
      "windows-x64": "exe",
    },
    info = lstatSync(file);
  if (supported[host] !== format || !info.isFile() || info.size === 0)
    throw Error("Unsupported single-file installer");
  return {
    file: basename(file),
    format,
    size: info.size,
    single_file: true,
    user_assembly_required: false,
    github_asset_eligible: info.size < 2 * 1024 ** 3,
  };
}
export function deliveryMetadata(components: string[], host: string) {
  const known = new Set([
      "native-services",
      "phone-apps",
      "container-host",
      "android-runtime",
      "container-images",
      "windows-qemu",
    ]),
    selected = new Set(components);
  if (
    selected.size !== components.length ||
    components.some((name) => !known.has(name))
  )
    throw Error("Unknown client component");
  const required =
    host === "windows-x64"
      ? ["android-runtime", "windows-qemu"]
      : [
          "native-services",
          "phone-apps",
          "container-host",
          "android-runtime",
          "container-images",
        ];
  const missing = required.filter((name) => !selected.has(name));
  return {
    prebuilt_components: components,
    server_included:
      selected.has("windows-qemu") || selected.has("native-services"),
    runtime_payload_complete: !missing.length,
    complete_device_bundle: !missing.length,
    missing_runtime_components: missing,
    deployment_mode: missing.length ? "component-bundle" : "embedded-local",
    server_requirement: missing.length
      ? "Complete local runtime components are required."
      : "None; the client initializes its local device runtime.",
  };
}
