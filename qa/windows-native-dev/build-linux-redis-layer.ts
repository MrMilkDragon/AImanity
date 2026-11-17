import { copyFileSync, mkdirSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ROOT, digest, output, run } from "../../exact/scripts/tooling/process";
import { collect, json, records, save } from "../../exact/scripts/tooling/inventory";
import { phoneFingerprint } from "../../exact/scripts/tooling/phone-apps";
import { verifyDependency } from "../../exact/scripts/tooling/linux-dependencies";
import { preparePayload } from "../../exact/scripts/tooling/client-payload";
import { buildNative } from "../../exact/scripts/tooling/native-build";
import { wslPlan } from "../../exact/scripts/tooling/wsl-factory";

const { QA_PREVIOUS_LAYER: prior, QA_LAYER: destination, PRODUCT_SOURCE: source,
  CALCIUM_BASE_SHA: baseHash, QA_PREVIOUS_SOURCE: previousSource } = process.env;
if (!prior || !destination || !previousSource || process.env.RUNNER_ENVIRONMENT !== "github-hosted" ||
    process.platform !== "linux" || output(["git", "rev-parse", "HEAD"], { cwd: ROOT }) !== source)
  throw Error("Expected disposable Linux runner and exact product source");
const work = join(ROOT, ".tools", "windows-native-dev"), compiled = join(work, "compiled");
const previousGuest = join(prior, "guest"), previousDisk = join(previousGuest, "release.ext4");
const manifest = json(join(prior, "manifest.json"));
const unchanged = ["apps/android-input", "apps/android-home", "scripts/android-input-probe", "scripts/tooling/android-apk.ts",
  "config/phone-browser.json", "config/linux-ocr.json", "config/ocr-models.json", "infra/desktop", "infra/workspace",
  "infra/network", "compose.yaml", "compose.dev.yaml", "compose.desktop.yaml"];
const changes = output(["git", "diff", "--name-only", previousSource, source!, "--", ...unchanged], {cwd:ROOT});
if (manifest.source_commit !== previousSource || manifest.guest.disk.sha256 !== baseHash ||
    changes) throw Error("The previous OCI and Android app inputs do not cover these changes: " + changes);
const diskRecord = manifest.files.find((row: {path:string}) => row.path === "guest/release.ext4");
if (process.argv[2] === "services") {
  if (statSync(previousDisk).size !== diskRecord.size || await digest(previousDisk) !== diskRecord.sha256)
    throw Error("Previous ext4 bytes differ from the checked guest artifact");
  const priorPayload = join(work, "staged-previous-source.payload");
  const ocr = join(work, "ocr");
  mkdirSync(ocr, { recursive:true });
  run(["debugfs", "-R", `dump /source.payload ${priorPayload}`, previousDisk]);
  if (await digest(priorPayload) !== manifest.release_sha256) throw Error("Prior Linux source bytes differ");
  run(["tar", "-x", "-f", priorPayload, "--strip-components=3", "-C", ocr, "--wildcards", "runtime/services/ocr/*"]);
  const phone = join(ROOT, ".tools/phone-apps");
  mkdirSync(phone);
  run(["tar", "-x", "-f", priorPayload, "--strip-components=2", "-C", phone, "--wildcards", "runtime/phone-apps/*"]);
  const phoneManifest = join(phone, "manifest.json"), staged = json(phoneManifest);
  rmSync(phoneManifest);
  save(phoneManifest, {...staged, source_sha256:await phoneFingerprint(ROOT), files:records(collect(phone, "runtime/phone-apps").files)});
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
const images = join(work, "reused-images.payload");
run(["debugfs", "-R", `dump /images.payload ${images}`, previousDisk]);
if (await digest(images) !== manifest.release_images_sha256) throw Error("Prior Windows OCI bytes differ");
const base = join(work, "reused-base");
const baseDirectory = join(base, "host/qemu");
mkdirSync(baseDirectory, {recursive:true});
symlinkSync(previousGuest, join(baseDirectory, "guest"));
const redisImage = wslPlan(ROOT).redis_image;
const staging = join(work, "redis-context");
const redisPath = join(compiled, "redis");
const redisTag = "aimanity-redis-qa:" + process.env.GITHUB_RUN_ID;
mkdirSync(staging);
writeFileSync(join(staging, "Dockerfile"), `FROM ${redisImage}
USER root
RUN mkdir -p /native-redis/lib /native-redis/bin && \\
    cp -L /lib/ld-musl-x86_64.so.1 /native-redis/lib/ && \\
    for command in redis-server redis-cli; do \\
        binary="/usr/local/bin/$command"; \\
        cp -L "$binary" /native-redis/bin/; \\
        libraries="$(ldd "$binary")" || exit 1; \\
        for library in $(printf '%s\\n' "$libraries" | awk '$2 == "=>" && substr($3, 1, 1) == "/" {print $3}'); do \\
            cp -L "$library" /native-redis/lib/ || exit 1; \\
        done; \\
    done
`);
run(["docker", "build", "--platform", "linux/amd64", "--network", "none", "-t", redisTag, staging]);
const redis = output(["docker", "create", "--network", "none", redisTag]);
try {
  run(["docker", "cp", `${redis}:/native-redis`, redisPath]);
} finally {
  run(["docker", "rm", redis]);
}
const version = output([join(redisPath, "lib", "ld-musl-x86_64.so.1"), "--library-path", join(redisPath, "lib"),
  join(redisPath, "bin", "redis-server"), "--version"]);
if (!version.startsWith("Redis server v=8.")) throw Error("Pinned Redis native closure cannot execute: " + version);
console.log(JSON.stringify({ redisImage, version }));
run(["bash", join(import.meta.dir, "legacy-guest-disk.sh"), join(guest, "guest.qcow2"), join(destination, "qa-disk")], {cwd: ROOT});
run(["bash", join(ROOT, "scripts/windows-release-layer.sh"), compiled, base, guest, images], { cwd: ROOT });

manifest.source_commit = source;
manifest.source_manifest_sha256 = sourceManifest;
manifest.release_sha256 = await digest(layerSource);
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
  images: manifest.release_images_sha256, reusedImagesFrom: previousSource }));
