import { copyFileSync, mkdirSync, mkdtempSync, rmdirSync } from "node:fs";
import { join, resolve } from "node:path";

const [sourceRoot, donorPath, workPath] = process.argv.slice(2);
if (!sourceRoot || !donorPath || !workPath) throw Error("Provide source root, signed Beryllium DMG and a fresh work directory");
const root = resolve(sourceRoot), donor = resolve(donorPath), work = resolve(workPath);
const { run, digest, hostKey } = await import(root + "/scripts/tooling/process.ts");
const { checkSource, sourceFiles } = await import(root + "/scripts/tooling/source.ts");
const { collect, records, save } = await import(root + "/scripts/tooling/inventory.ts");
const { buildNative, prepareServices } = await import(root + "/scripts/tooling/native-build.ts");
const { writeTar, buildReleaseTools } = await import(root + "/scripts/tooling/archive.ts");
const { buildGuest, guestBinary } = await import(root + "/scripts/tooling/guest-build.ts");
const { prepareWeb } = await import(root + "/scripts/tooling/web-runtime.ts");
const { buildConfig } = await import(root + "/scripts/tooling/resources.ts");
const { buildClient, noticeManifest } = await import(root + "/scripts/tooling/client-build.ts");

if (hostKey() !== "macos-arm64") throw Error("Apple Silicon builder required");
run(["git", "diff", "--quiet", "1140106eba7b4edf80879b49d3eae0f515d02c93", "HEAD", "--",
  ":(glob)apps/android-*/**", "scripts/tooling/phone-apps.ts",
  "config/container-host-runtime.json"], { cwd: root });
const source = sourceFiles(root);
checkSource(root, { repository: true, manifest: true }, source);
if (await digest(donor) !== "d7257a004e4f1140e2627182a833e9aad597151f24c636870b9eae1f330d0945")
  throw Error("Expected the published Beryllium DMG");
mkdirSync(work);
const resources = join(root, "apps/client/src-tauri/gen/host");
const reuse = join(work, "immutable");
const mount = mkdtempSync(join(work, "mount-"));
mkdirSync(reuse);
mkdirSync(join(resources, "speech"), { recursive: true });
run(["hdiutil", "attach", donor, "-readonly", "-nobrowse", "-mountpoint", mount], { cwd: root });
try {
  const app = join(mount, "AImanity.app");
  const host = join(app, "Contents/Resources/host");
  run(["codesign", "--verify", "--deep", "--strict", app], { cwd: root });
  for (const member of ["android.payload", "images.payload", "speech/ggml-tiny-q5_1.bin"])
    copyFileSync(join(host, member), join(resources, member));
  run(["tar", "-xf", join(host, "source.payload"), "-C", reuse, "runtime/container-host", "runtime/phone-apps"], { cwd: root });
} finally {
  run(["hdiutil", "detach", mount], { cwd: root });
  rmdirSync(mount);
}
const guest = await buildGuest(root, join(work, "arm-bridge"));
const newBridge = join(guest, guestBinary);
process.env.AI_PHONE_RELEASE_DESKTOP_BRIDGE = newBridge;
buildReleaseTools(root);
noticeManifest(root);
await buildNative(root);
const services = await prepareServices(root);
const stagedBridge = services.find((entry: {path: string}) => entry.path === "runtime/services/bin/desktop-bridge");
if (!stagedBridge) throw Error("The new desktop bridge is required in the offline payload");
const files = [
  ...source,
  ...prepareWeb(root),
  ...services,
  ...collect(join(reuse, "runtime/phone-apps"), "runtime/phone-apps").files,
  ...collect(join(reuse, "runtime/container-host"), "runtime/container-host").files,
];
const manifest = { format: 2, components: ["web", "native-services", "phone-apps", "container-host"], files: records(files) };
const archive = join(resources, "source.payload");
await writeTar(
  archive,
  [...files, { path: "HOST-MANIFEST.json", mode: 0o644, data: Buffer.from(JSON.stringify(manifest)) }],
  { codec: "xz", architecture: "arm64", root },
);
const inputs = ["source.payload", "images.payload", "android.payload", "speech/ggml-tiny-q5_1.bin"];
save(join(resources, "integrity.json"), Object.fromEntries(await Promise.all(inputs.map(async name => [name, await digest(join(resources, name))]))));
buildConfig(root);
const result = await buildClient("macos", root, { temporarySigning: true });
console.log(JSON.stringify({ artifact: result.artifact, source: result.source_commit, desktop_bridge_sha256: await digest(newBridge), signing: result.signing }));
