import { copyFileSync, mkdirSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ROOT, digest, output, run } from "../../exact/scripts/tooling/process";
import { json, save } from "../../exact/scripts/tooling/inventory";
import { buildPhone } from "../../exact/scripts/tooling/phone-apps";
import { buildFactoryDependency } from "../../exact/scripts/tooling/factory-inputs";
import { preparePayload } from "../../exact/scripts/tooling/client-payload";
import { buildNative } from "../../exact/scripts/tooling/native-build";
import { stageWindowsServices } from "../../exact/scripts/tooling/wsl-factory";
import { prepareImages } from "../../exact/scripts/tooling/factory";

const { QA_PREVIOUS_LAYER: prior, QA_LAYER: destination, PRODUCT_SOURCE: source,
  CALCIUM_BASE_SHA: baseHash, QA_PREVIOUS_SOURCE: previousSource } = process.env;
if (!prior || !destination || !previousSource || process.env.RUNNER_ENVIRONMENT !== "github-hosted" ||
    process.platform !== "linux" || output(["git", "rev-parse", "HEAD"], { cwd: ROOT }) !== source)
  throw Error("Expected disposable Linux runner and exact product source");
const work = join(ROOT, ".tools", "windows-native-dev"), compiled = join(work, "compiled");
const previousGuest = join(prior, "guest");
const manifest = json(join(prior, "manifest.json"));
if (manifest.source_commit !== previousSource || manifest.guest.disk.sha256 !== baseHash)
  throw Error("Prior artifact does not expose the expected immutable Calcium backing disk");
if (process.argv[2] === "services") {
  const ocr = join(work, "ocr");
  await buildPhone(ROOT);
  await buildFactoryDependency(ROOT, join(work, "ocr-context"), ocr, "linux-ocr");
  await buildNative(ROOT);
  await preparePayload(ROOT, { nativeServices: true, phoneApps: true, linuxOcr: ocr });
  const payload = join(ROOT, "apps/native/gen/host/source.payload");
  const components = JSON.parse(output(["tar", "-xOf", payload, "HOST-MANIFEST.json"])).components;
  if (JSON.stringify(components) !== JSON.stringify(["native-services", "phone-apps"]))
    throw Error("Expected native-only guest component set");
  mkdirSync(compiled, { recursive: true });
  const extracted = Bun.spawnSync(["tar", "-xOf", payload, "runtime/services/bin/assistant"], {
    maxBuffer: 64 * 1024 * 1024, stderr: "inherit",
  });
  if (extracted.exitCode !== 0 || extracted.stdout.length < 1024) throw Error("Guest bootstrap executable was unavailable");
  writeFileSync(join(compiled, "bootstrap.program"), extracted.stdout, { mode: 0o755 });
  copyFileSync(payload, join(compiled, "source.payload"));
  process.exit(0);
}
if (process.argv[2] !== "layer") throw Error("Expected services or layer phase");
const immutableNames = ["guest.qcow2", "bzImage", "kernel.config", "KERNEL-LICENSE.txt"];
const guest = join(destination, "guest");
mkdirSync(guest, { recursive: true });
for (const name of immutableNames) {
  const file = join(previousGuest, name);
  const record = manifest.files.find((row: { path: string }) => row.path === "guest/" + name);
  if (!record || statSync(file).size !== record.size || await digest(file) !== record.sha256)
    throw Error("Calcium immutable bytes differ: " + name);
  copyFileSync(file, join(guest, name));
}

const layerSource = join(compiled, "source.payload");
const sourceManifest = await digest(join(ROOT, "SOURCE-MANIFEST.json"));
const { receipt: { images, archive_sha256 } } = await prepareImages(
  ROOT, compiled, sourceManifest, "amd64", undefined, false,
);
if (Object.keys(images).sort().join(",") !== "desktop,workspace")
  throw Error("Native databases require exactly the desktop and workspace images");
const base = join(work, "reused-base");
const baseDirectory = join(base, "host/qemu");
mkdirSync(baseDirectory, {recursive:true});
symlinkSync(previousGuest, join(baseDirectory, "guest"));
stageWindowsServices(ROOT, compiled);
console.log(JSON.stringify({ imageRoles: Object.keys(images).sort(), nativePostgres: true, nativeRedis: true }));

run(["bash", join(import.meta.dir, "legacy-guest-disk.sh"), join(guest, "guest.qcow2"), join(destination, "qa-disk")], {cwd: ROOT});
const releaseSha = await digest(layerSource);
run(["bash", join(ROOT, "scripts/windows-release-layer.sh"), compiled, base, guest, releaseSha], { cwd: ROOT });


manifest.source_commit = source;
manifest.source_manifest_sha256 = sourceManifest;
manifest.release_sha256 = releaseSha;
manifest.release_images_sha256 = archive_sha256;
manifest.release_image_roles = Object.keys(images).sort();
manifest.windows_boot_verified = false;
for (const name of ["release-init.cpio.gz", "release.ext4"]) {
  const path = "guest/" + name;
  const previous = manifest.files.findIndex((row: { path: string }) => row.path === path);
  const replacement = { path, size: statSync(join(destination, path)).size, sha256: await digest(join(destination, path)) };
  if (previous < 0) manifest.files.push(replacement);
  else manifest.files[previous] = replacement;
}
save(join(destination, "manifest.json"), manifest);
console.log(JSON.stringify({ source, baseHash, sourceManifest, release: manifest.release_sha256,
  images: manifest.release_images_sha256, imageRoles: manifest.release_image_roles, immutableBackingFrom: previousSource }));
