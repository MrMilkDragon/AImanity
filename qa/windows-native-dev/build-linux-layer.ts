import { copyFileSync, mkdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ROOT, digest, output, run } from "../../exact/scripts/tooling/process";
import { json, save } from "../../exact/scripts/tooling/inventory";
import { checkDocker, prepareApps, prepareImages } from "../../exact/scripts/tooling/factory";
import { verifyDependency } from "../../exact/scripts/tooling/linux-dependencies";
import { preparePayload } from "../../exact/scripts/tooling/client-payload";
import { buildNative } from "../../exact/scripts/tooling/native-build";

const { QA_BASE: base, QA_LAYER: destination, PRODUCT_SOURCE: source, CALCIUM_BASE_SHA: baseHash } = process.env;
if (!base || !destination || process.env.RUNNER_ENVIRONMENT !== "github-hosted" ||
    process.platform !== "linux" || output(["git", "rev-parse", "HEAD"], { cwd: ROOT }) !== source)
  throw Error("Expected disposable Linux runner and exact product source");
const work = join(ROOT, ".tools", "windows-native-dev"), compiled = join(work, "compiled");
if (process.argv[2] === "services") {
  checkDocker();
  prepareApps(ROOT, process.env);
  const prior = process.env.QA_PREVIOUS_LAYER;
  if (!prior) throw Error("The verified prior guest layer is required for the matching OCR closure");
  const previousManifest = json(join(prior, "manifest.json"));
  const previousDisk = join(prior, "guest", "release.ext4");
  const previousSource = join(work, "staged-previous-source.payload");
  const ocr = join(work, "ocr");
  mkdirSync(ocr, { recursive:true });
  run(["debugfs", "-R", `dump /source.payload ${previousSource}`, previousDisk]);
  if (await digest(previousSource) !== previousManifest.release_sha256) throw Error("Prior Linux source bytes differ");
  run(["tar", "-x", "-f", previousSource, "--strip-components=3", "-C", ocr, "--wildcards", "runtime/services/ocr/*"]);
  await verifyDependency(ROOT, ocr, "linux-ocr");
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
const immutable = join(base, "host", "qemu"), manifest = json(join(immutable, "manifest.json"));
const pin = json(join(ROOT, "config/windows-qemu.json"));
if (await digest(join(immutable, "manifest.json")) !== pin.bundled_manifest_sha256 ||
    manifest.guest.disk.sha256 !== baseHash) throw Error("Calcium manifest integrity");
const immutableNames = ["guest.qcow2", "bzImage", "kernel.config", "KERNEL-LICENSE.txt"];
const guest = join(destination, "guest");
mkdirSync(guest, { recursive: true });
for (const name of immutableNames) {
  const file = join(immutable, "guest", name);
  const record = manifest.files.find((row: { path: string }) => row.path === "guest/" + name);
  if (!record || statSync(file).size !== record.size || await digest(file) !== record.sha256)
    throw Error("Calcium immutable bytes differ: " + name);
  copyFileSync(file, join(guest, name));
}

const layerSource = join(compiled, "source.payload");
const sourceManifest = await digest(join(ROOT, "SOURCE-MANIFEST.json"));
const { file: images, receipt: imageReceipt } = await prepareImages(ROOT, work, sourceManifest, "amd64");
run(["bash", join(ROOT, "scripts/windows-release-layer.sh"), compiled, base, guest, images], { cwd: ROOT });

manifest.source_commit = source;
manifest.source_manifest_sha256 = sourceManifest;
manifest.release_sha256 = await digest(layerSource);
manifest.release_images_sha256 = imageReceipt.archive_sha256;
manifest.windows_boot_verified = false;
for (const name of ["release-init.cpio.gz", "release.ext4"]) {
  const path = "guest/" + name;
  const previous = manifest.files.findIndex((row: { path: string }) => row.path === path);
  const replacement = { path, size: statSync(join(destination, path)).size, sha256: await digest(join(destination, path)) };
  if (previous < 0) manifest.files.push(replacement);
  else manifest.files[previous] = replacement;
}
save(join(destination, "manifest.json"), manifest);
console.log(JSON.stringify({ source, baseHash, sourceManifest, release: manifest.release_sha256, images: manifest.release_images_sha256, developmentImageXZPreset: 3 }));
