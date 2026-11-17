import { copyFileSync, cpSync, lstatSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { ROOT, digest, hash, output, run } from "../../exact/scripts/tooling/process";
import { json, save } from "../../exact/scripts/tooling/inventory";
import { fetchPin } from "../../exact/scripts/tooling/factory";
import { unpackWindowsInstaller } from "../../exact/scripts/tooling/windows-installer";
import { prepareWindowsQemu } from "../../exact/scripts/tooling/windows-qemu";
import { preparePayload } from "../../exact/scripts/tooling/client-payload";

const { AIMANITY_QA_ROOT: root, PRODUCT_SOURCE: source, CALCIUM_BASE_SHA: baseHash, CALCIUM_ANDROID_SHA: androidHash } = process.env;
const guestSource = process.env.QA_GUEST_SOURCE ?? source;
if (process.platform !== "win32" || process.env.RUNNER_ENVIRONMENT !== "github-hosted" ||
    output(["git", "rev-parse", "HEAD"], { cwd: ROOT }) !== source)
  throw Error("Expected a disposable Windows runner checking the exact source");
const phase = process.argv[2];
const storage = join(ROOT, "apps/native/gen/host");
if (phase !== "base" && (!root || !/^[A-Z]:\\Dysprosium-QA-/i.test(root))) throw Error("A dedicated Windows QA root is required");
if (phase === "launch-layout") {
  const dev = join(root!, "dev");
  mkdirSync(dev);
  cpSync(storage, join(dev, "host"), { recursive: true });
  copyFileSync(join(ROOT, "apps/native/target/debug/aimanity-native.exe"), join(dev, "aimanity-native.exe"));
  process.exit(0);
}
if (phase !== "base" && phase !== "dev") throw Error("Unknown resource preparation phase");
const config = json(join(ROOT, "config/windows-qemu.json"));
const pin = config.calcium_installer;
const installer = await fetchPin(join(ROOT, "dist", "qa-calcium-input"), pin.file, pin);
if (phase === "base") {
  const folder = join(ROOT, "dist", "immutable");
  const names = ["guest.qcow2", "bzImage", "kernel.config", "KERNEL-LICENSE.txt"];
  await unpackWindowsInstaller(ROOT, installer, folder, [
    "{app}\\host\\qemu\\manifest.json",
    ...names.map(name => "{app}\\host\\qemu\\guest\\" + name),
  ]);
  const bundled = join(folder, "host", "qemu");
  if (await digest(join(bundled, "manifest.json")) !== config.bundled_manifest_sha256)
    throw Error("Calcium QEMU manifest changed");
  const manifest = json(join(bundled, "manifest.json"));
  if (manifest.guest.disk.sha256 !== baseHash) throw Error("Calcium base changed");
  for (const name of names) {
    const info = manifest.files.find((row: { path: string }) => row.path === "guest/" + name);
    const file = join(bundled, info.path);
    if (lstatSync(file).size !== info.size || await digest(file) !== info.sha256) throw Error("Calcium bytes changed: " + name);
  }
  rmSync(installer);
} else {
  const temp = join(root!, "inputs"), layer = join(root!, "layer"), qemu = join(temp, "qemu"), copies = join(temp, "calcium");
  mkdirSync(temp);
  await prepareWindowsQemu(qemu, installer);
  await unpackWindowsInstaller(ROOT, installer, copies, ["{app}\\host\\android.payload"]);
  rmSync(installer);
  const android = join(copies, "host", "android.payload");
  const integrity = await preparePayload(ROOT, { androidRuntime: android });
  if (integrity["android.payload"] !== androidHash) throw Error("Calcium Android image changed");
  const targetQemu = join(storage, "qemu");
  mkdirSync(targetQemu);
  copyFileSync(join(layer, "manifest.json"), join(targetQemu, "manifest.json"));
  cpSync(join(layer, "guest"), join(targetQemu, "guest"), { recursive: true });
  for (const dir of ["runtime", "licenses"]) cpSync(join(qemu, dir), join(targetQemu, dir), { recursive: true });
  const manifestPath = join(targetQemu, "manifest.json"), manifest = json(manifestPath);
  const guestManifest = run(["git", "show", `${guestSource}:SOURCE-MANIFEST.json`], {cwd:ROOT, stdio:["ignore","pipe","pipe"]}).stdout;
  if (manifest.source_commit !== guestSource || manifest.source_manifest_sha256 !== hash(guestManifest) ||
      manifest.guest.disk.sha256 !== baseHash || !/^[0-9a-f]{64}$/.test(manifest.release_sha256))
    throw Error("Linux layer is not tied to the current source");
  if (guestSource !== source) {
    const changed = output(["git", "diff", "--name-only", guestSource!, source!, "--"]).split("\n");
    if (!changed.every(path => path.startsWith("apps/native/") || path === "SOURCE-MANIFEST.json"))
      throw Error("Reused Linux layer cannot cover source changes outside the native frontend: " + changed.join(", "));
  }
  for (const record of manifest.files) {
    const file = join(targetQemu, record.path);
    if (lstatSync(file).size !== record.size || await digest(file) !== record.sha256) throw Error("Guest or runtime bytes changed: " + record.path);
  }
  integrity["qemu/manifest.json"] = await digest(manifestPath);
  save(join(storage, "integrity.json"), integrity);
  rmSync(temp, { recursive: true });
  console.log(JSON.stringify({ exactSource: source, guestSource, windowsDevOnly: true, offlineAndroid: true, guestLayer: manifest.release_sha256 }));
}
