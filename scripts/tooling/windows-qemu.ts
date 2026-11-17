import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  renameSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { json, save } from "./inventory";
import { fetchPin } from "./factory";
import { digest, output, ROOT } from "./process";
import { unpackWindowsInstaller } from "./windows-installer";
import { stageWindowsQemu, type WindowsQemuManifest } from "./windows-vm-payload";

export async function prepareWindowsQemu(destination: string, calcium?: string) {
  if (process.platform !== "win32" || process.arch !== "x64")
    throw Error("Prepare the QEMU component on Windows x64");
  destination = resolve(destination);
  if (existsSync(destination)) throw Error("Use a new QEMU output directory");
  mkdirSync(dirname(destination), { recursive: true });

  const { calcium_installer: pin, bundled_manifest_sha256: expected } =
    json(join(ROOT, "config/windows-qemu.json"));
  const work = mkdtempSync(join(dirname(destination), ".qemu-component-"));
  try {
    const installer = calcium
      ? resolve(calcium)
      : await fetchPin(work, pin.file, pin);
    if (calcium) {
      const info = lstatSync(installer);
      if (!info.isFile() || info.size !== pin.size_bytes || (await digest(installer)) !== pin.sha256)
        throw Error("Installer differs from the pinned Calcium release");
    }

    const extraction = join(work, "extracted");
    const guestFiles = ["guest.qcow2", "bzImage", "kernel.config", "KERNEL-LICENSE.txt"];
    await unpackWindowsInstaller(ROOT, installer, extraction, [
      ...guestFiles.map(name => "{app}\\host\\qemu\\guest\\" + name),
      "{app}\\host\\qemu\\manifest.json",
      "{app}\\host\\qemu\\runtime\\*",
      "{app}\\host\\qemu\\licenses\\*",
    ]);
    const bundled = join(extraction, "host", "qemu");
    const installedManifest = join(bundled, "manifest.json");
    if (await digest(installedManifest) !== expected)
      throw Error("QEMU manifest differs from the pinned Calcium runtime");
    const installed = json<{
      qemu: WindowsQemuManifest;
      files: WindowsQemuManifest["files"];
    }>(installedManifest);
    if (installed.qemu.version !== "11.1.3")
      throw Error("Expected the qualified QEMU 11.1.3 component");

    const component = {
      ...installed.qemu,
      files: installed.files
        .filter(({ path }) => path.startsWith("runtime/") || path.startsWith("licenses/")),
    };
    const stage = join(work, "output");
    await stageWindowsQemu(bundled, component, stage);
    const baseline = join(stage, "baseline", "host", "qemu");
    mkdirSync(join(baseline, "guest"), { recursive: true });
    copyFileSync(installedManifest, join(baseline, "manifest.json"));
    for (const name of guestFiles) {
      const input = join(bundled, "guest", name);
      const record = installed.files.find(row => row.path === "guest/" + name);
      if (!record || lstatSync(input).size !== record.size)
        throw Error("Missing pinned Calcium guest file: " + name);
      copyFileSync(input, join(baseline, "guest", name));
    }

    const runtime = join(stage, "runtime");
    const accelerators = output([join(runtime, "qemu-system-x86_64.exe"), "-accel", "help"]);
    if (!/^whpx$/im.test(accelerators) ||
        !output([join(runtime, "qemu-img.exe"), "--version"]).includes(component.version))
      throw Error("QEMU runtime failed the Windows x64 component check");

    const manifest = {
      ...component,
      binary_origin: { ...pin, manifest_sha256: expected },
    };
    save(join(stage, "windows-qemu.json"), manifest);
    renameSync(stage, destination);
    return manifest;
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}
