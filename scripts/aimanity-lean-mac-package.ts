import { copyFileSync, mkdirSync, mkdtempSync, renameSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";

const [root, images, work] = process.argv.slice(2);
const previousCommit = "59c8936f13456aac64fab537ff648f4334d2533e";
const previousAsset = "AImanity-0.0.2-macos-arm64.dmg";
const previousDigest = "9df0a34813007ebce7bc8335cefacc2dfa583aa864e9f35dadea9cd3accef080";
const { run, output: commandOutput, digest, hostKey } = await import(root + "/scripts/tooling/process.ts");
const { checkSource, sourceFiles } = await import(root + "/scripts/tooling/source.ts");
const { collect, json, records, save } = await import(root + "/scripts/tooling/inventory.ts");
const { buildNative, prepareServices } = await import(root + "/scripts/tooling/native-build.ts");
const { writeTar, buildReleaseTools } = await import(root + "/scripts/tooling/archive.ts");
const { buildConfig } = await import(root + "/scripts/tooling/resources.ts");
const { buildClient, noticeManifest } = await import(root + "/scripts/tooling/client-build.ts");
const { imageManifest } = await import(root + "/scripts/tooling/image-capsule.ts");

if (hostKey() !== "macos-arm64") throw Error("Apple Silicon builder required");
run(["git", "diff", "--quiet", previousCommit, "HEAD", "--", "apps/web", "apps/android-*", "packages/protocol",
  "scripts/tooling/web-runtime.ts", "scripts/tooling/phone-apps.ts", "config/container-host-runtime.json"], {cwd:root});
const source = sourceFiles(root);
checkSource(root, {repository:true, manifest:true}, source);

const resources = join(root, "apps/client/src-tauri/gen/host");
const oldFile = join(work, previousAsset);
const reuse = join(work, "previous");
mkdirSync(reuse, {recursive:true});
mkdirSync(join(resources, "speech"), {recursive:true});
run(["curl", "-fsSL", "--retry", "3", "-o", oldFile,
  "https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.2/" + previousAsset], {cwd:root});
if (await digest(oldFile) !== previousDigest) throw Error("Previous official DMG digest differs");
const mount = mkdtempSync(join(work, "mount-"));
run(["hdiutil", "attach", oldFile, "-readonly", "-nobrowse", "-mountpoint", mount], {cwd:root});
try {
  const app = join(mount, "AImanity.app");
  const host = join(app, "Contents/Resources/host");
  run(["codesign", "--verify", "--deep", "--strict", app], {cwd:root});
  for (const member of ["android.payload", "speech/ggml-tiny-q5_1.bin"]) {
    copyFileSync(join(host, member), join(resources, member));
  }
  run(["tar", "-xf", join(host, "source.payload"), "-C", reuse,
    "runtime/container-host", "runtime/phone-apps", "runtime/web", "runtime/services/bin/desktop-bridge"], {cwd:root});
} finally {
  run(["hdiutil", "detach", mount], {cwd:root});
  rmSync(mount, {recursive:true});
  rmSync(oldFile);
}

buildReleaseTools(root);
const arm = json(join(dirname(images), "images.receipt.json"));
const imageDigest = await digest(images);
const image = await imageManifest(images, root);
const commit = commandOutput(["git", "rev-parse", "HEAD"], {cwd:root});
if (image.architecture !== "arm64" || arm.source !== commit || arm.archive_sha256 !== imageDigest) {
  throw Error("Lean native image capsule differs from its receipt");
}
copyFileSync(images, join(resources, "images.payload"));
noticeManifest(root);
process.env.AI_PHONE_RELEASE_DESKTOP_BRIDGE = join(reuse, "runtime/services/bin/desktop-bridge");
await buildNative(root);
const services = await prepareServices(root);
const files = [...source, ...["web","phone-apps","container-host"].flatMap(role =>
  collect(join(reuse, "runtime", role), "runtime/" + role).files), ...services];
const manifest = { format:2, components:["web","native-services","phone-apps","container-host"], files:records(files) };
const archive = join(resources, "source.payload");
await writeTar(archive, [...files, {path:"HOST-MANIFEST.json", mode:0o644, data:Buffer.from(JSON.stringify(manifest))}],
  {codec:"xz", architecture:"arm64", root});
save(join(resources, "integrity.json"), {
  "source.payload": await digest(archive),
  "images.payload": imageDigest,
  "android.payload": await digest(join(resources, "android.payload")),
  "speech/ggml-tiny-q5_1.bin": await digest(join(resources, "speech/ggml-tiny-q5_1.bin")),
});
buildConfig(root);
const result = await buildClient("macos", root, {temporarySigning:true});
const release = join(work, "release");
mkdirSync(release);
const output = join(release, result.installer_delivery.file);
renameSync(result.artifact, output);
renameSync(result.artifact + ".manifest.json", output + ".manifest.json");
console.log(JSON.stringify({artifact:output, bytes:result.installer_delivery.size,
  source:result.source_commit, imageBytes:arm.bytes, signing:result.signing}));
