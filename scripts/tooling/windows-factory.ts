import { mkdirSync } from "node:fs";
import { join, basename } from "node:path";
import { ROOT, hostKey, bunProgram, run, output } from "./process";
import { json, save } from "./inventory";
import { checkSource } from "./source";
import { factoryAndroidInput, prepareFactoryAndroid } from "./android-component";
import { deliveryMetadata } from "./client-payload";
import { buildReleaseTools } from "./archive";
import { windowsReleaseSigning } from "./installer-signing";
import { hostResources } from "./resources";
import {
  windowsSourceIdentity as sourceIdentity,
  type WindowsVmInputs,
} from "./windows-vm-payload";

export async function windowsFactory(
  root: string,
  vm: WindowsVmInputs,
  releaseSigning = false,
) {
  if (hostKey() !== "windows-x64")
    throw Error("Complete Windows assembly requires Windows x64");
  const androidInput = factoryAndroidInput();
  if (releaseSigning) windowsReleaseSigning();
  checkSource(root, { repository: true, manifest: true });
  const source = await sourceIdentity(root),
    work = join(root, ".tools/complete-windows");
  mkdirSync(work);
  buildReleaseTools(root);
  run(
    [
      "cargo",
      "build",
      "--release",
      "--locked",
      "-p",
      "ai-phone-host-runtime",
      "--bin",
      "host-resources",
    ],
    { cwd: root },
  );
  const android = await prepareFactoryAndroid(androidInput, work, 6, root);
  run(
    [
      bunProgram(ROOT),
      join(ROOT, "scripts/build-client.ts"),
      "--root",
      root,
      "--platform",
      "windows",
      releaseSigning ? "--sign-release" : "--sign-temporary",
      "--android-runtime",
      android.file,
      "--qemu",
      vm.qemu,
      "--qemu-identity",
      vm.qemuIdentity,
      "--guest",
      vm.guest,
      "--guest-identity",
      vm.guestIdentity,
    ],
    { cwd: root },
  );
  const version = json(join(root, "config/release-policy.json")).version,
    file = join(root, "dist", `AImanity-${version}-windows-x64-setup.exe`),
    manifest = json(file + ".manifest.json"),
    delivery = deliveryMetadata(manifest.prebuilt_components, "windows-x64");
  if (
    !delivery.complete_device_bundle ||
    manifest.signing !== (releaseSigning ? "authenticode-trusted" : "authenticode-self-signed") ||
    (releaseSigning && !(manifest.authenticode?.cryptographically_verified && manifest.authenticode.timestamped)) ||
    manifest.complete_device_bundle !== true
  )
    throw Error("Windows installer incomplete");
  const artifact = {
    file: basename(file),
    size: manifest.installer_delivery.size,
    sha256: manifest.sha256,
    delivery,
  };
  if (JSON.stringify(await sourceIdentity(root)) !== JSON.stringify(source))
    throw Error("Source inventory changed during Windows assembly");
  const backendIdentity = {
    backend_source_commit: source.source_commit,
    backend_source_manifest_sha256: source.source_manifest_sha256,
  };
  save(file + ".manifest.json", { ...manifest, ...backendIdentity });
  const report = {
    format: 1,
    host: "windows-x64",
    ...source,
    ...backendIdentity,
    builder_commit: output(["git", "rev-parse", "HEAD"], { cwd: ROOT }),
    artifact,
    package_delivery: {
      format: "single",
      file: artifact.file,
      size: artifact.size,
      sha256: artifact.sha256,
    },
    android: {
      ...android.receipt,
      programs_prepared: true,
      guest_boot: "not-executed",
    },
    backend_runtime: "qemu-whpx-backend",
    backend_vm: json(join(root, hostResources, "qemu/manifest.json")),
    backend_runtime_acceptance: "not-tested",
    physical_installation: "not-tested",
    windows_boot_verified: false,
    binary_publication: false,
  };
  save(join(root, "dist/windows-full-factory-report.json"), report);
  if (!manifest.installer_delivery.github_asset_eligible)
    throw Error(
      `Complete Windows EXE is ${artifact.size} bytes; GitHub release assets require less than 2 GiB`,
    );
  return report;
}
