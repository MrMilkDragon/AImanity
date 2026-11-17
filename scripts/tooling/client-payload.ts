import {
  existsSync,
  lstatSync,
  mkdirSync,
  constants,
  copyFileSync,
  renameSync,
  rmSync,
} from "node:fs";
import { join, basename } from "node:path";
import { ROOT, digest, output, nativeProgram } from "./process";
import { sourceFiles, type SourceFile } from "./source";
import { collect, json, save, records } from "./inventory";
import { buildReleaseTools, writeTar, tarEntries, entryBytes } from "./archive";
import { prepareServices } from "./native-build";
import { prepareContainerHost } from "./container-host";
import { hostResources } from "./resources";
import { stageWindowsVm, type WindowsVmInputs } from "./windows-vm-payload";
import { prepareSpeechModel } from "./speech-model";

export const componentManifests = {
  "native-services": "runtime/services/manifest.json",
  "phone-apps": "runtime/phone-apps/manifest.json",
  "container-host": "runtime/container-host/manifest.json",
};
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
  const speech = await prepareSpeechModel(root);
  const selected: SourceFile[] = await sourceFiles(root),
    components: string[] = [];
  const producers: [
    keyof PayloadOptions,
    keyof typeof componentManifests,
    () => SourceFile[] | Promise<SourceFile[]>,
  ][] = [
    [
      "nativeServices",
      "native-services",
      () => prepareServices(root, options.linuxOcr),
    ],
    ["phoneApps", "phone-apps", () => {
      output([buildReleaseTools(root), "phone-apps", "verify", root], { cwd: root });
      return collect(join(root, ".tools/phone-apps"), "runtime/phone-apps").files;
    }],
    ["containerHost", "container-host", () => prepareContainerHost(root)],
  ];
  for (const [key, name, produce] of producers)
    if (options[key]) {
      components.push(name);
      selected.push(...(await produce()));
    }
  const manifest = {
      format: components.length ? 2 : 1,
      components,
      files: records(selected),
    },
    payload = join(folder, "source.payload"),
    temporary = payload + ".tmp";
  rmSync(temporary, { force: true });
  await writeTar(
    temporary,
    [
      ...selected,
      {
        path: "HOST-MANIFEST.json",
        mode: 0o644,
        data: Buffer.from(JSON.stringify(manifest)),
      },
    ],
    { codec: "xz", root },
  );
  renameSync(temporary, payload);
  const integrity: Record<string, string> = {
    "source.payload": await digest(payload),
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
export async function payloadComponents(root = ROOT) {
  const folder = join(root, hostResources),
    payload = join(folder, "source.payload");
  if (!existsSync(payload)) return [];
  let components: string[] = [];
  for await (const entry of tarEntries(payload, "xz", root))
    if (entry.name === "HOST-MANIFEST.json")
      components =
        JSON.parse((await entryBytes(entry)).toString()).components || [];
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
      ...Object.keys(componentManifests),
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
