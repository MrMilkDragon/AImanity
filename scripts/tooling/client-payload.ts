import {
  existsSync,
  lstatSync,
  mkdirSync,
  constants,
  copyFileSync,
  rmSync,
} from "node:fs";
import { join, basename } from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { text } from "node:stream/consumers";
import { ROOT, digest, output, nativeProgram } from "./process";
import { collect, json, save } from "./inventory";
import { buildReleaseTools, writeTarStream } from "./archive";
import { prepareServices } from "./native-build";
import { prepareContainerHost } from "./container-host";
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
export async function preparePayload(
  root = ROOT,
  options: PayloadOptions = {},
) {
  const folder = join(root, hostResources);
  mkdirSync(folder, { recursive: true });
  const releaseTools = buildReleaseTools(root);
  const speech = JSON.parse(output(
    [releaseTools, "prepare-speech-model", join(root, ".tools/speech"), folder],
    { cwd: root },
  ));
  const selected = options.nativeServices ? await prepareServices(root, options.linuxOcr) : [];
  if (options.phoneApps) {
    output([releaseTools, "phone-apps", "verify", root], { cwd: root });
    selected.push(...collect(join(root, ".tools/phone-apps"), "runtime/phone-apps").files);
  }
  if (options.containerHost) selected.push(...await prepareContainerHost(root));
  const child = spawn(releaseTools, ["source", "payload", join(folder, "source.payload")], {
    cwd: root, stdio: ["pipe", "pipe", "inherit"],
  });
  const complete = once(child, "close"),
    receipt = text(child.stdout);
  const [[status], report] = await Promise.all([
    complete, receipt, writeTarStream(child.stdin, selected),
  ]).finally(() => {
    if (child.exitCode === null) child.kill();
  });
  if (status !== 0) throw Error("Source payload compilation failed");
  const integrity: Record<string, string> = {
    "source.payload": JSON.parse(report).sha256,
    [speech.resource]: speech.sha256,
  };
  for (const [name, source] of [
    ["android.payload", options.androidRuntime],
    ["images.payload", options.imageRuntime],
  ] as const) {
    const target = join(folder, name);
    if (!source) {
      rmSync(target, { force: true });
      continue;
    }
    if (source !== target) copyFileSync(source, target, constants.COPYFILE_FICLONE);
    integrity[name] = await digest(target);
  }
  const windowsVm = join(folder, "qemu");
  if (options.windowsVm)
    integrity["qemu/manifest.json"] = await stageWindowsVm(root, windowsVm, options.windowsVm);
  else rmSync(windowsVm, { recursive: true, force: true });
  save(join(folder, "integrity.json"), integrity);
  return integrity;
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
export async function stageSource(archive: string, home: string, root = ROOT) {
  const result = output(
    [
      nativeProgram(root),
      "--stage-source",
      archive,
      home,
      await digest(archive),
    ],
    { cwd: root },
  );
  const offset = result.indexOf(" ");
  if (offset !== 64) throw Error("Invalid source staging receipt");
  return result.slice(offset + 1);
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
