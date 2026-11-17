import { existsSync, mkdirSync, linkSync, lstatSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { output, run } from "./process";
import { json } from "./inventory";
import { fetchPin } from "./factory";
import { resourceMap } from "./resources";
import { buildReleaseTools } from "./archive";

export async function unpackWindowsInstaller(
  root: string,
  installer: string,
  folder: string,
  members = ["{app}\\*"],
) {
  const program: string = JSON.parse(output([buildReleaseTools(root), "windows-unpacker", root], { cwd: root }));
  run(
    [
      program,
      "-x", "-b", "-q", "-a", "-c{app}",
      "-d" + folder, installer, ...members,
    ],
    { stdio: ["ignore", "inherit", "inherit"] },
  );
}

export async function buildWindowsInstaller(
  root: string,
  binary: string,
  destination: string,
) {
  const folder = join(root, ".tools/windows-installer"),
    policy = json(join(root, "config/windows-installer.json")),
    archive = await fetchPin(folder, "inno-setup.exe", policy.compiler),
    compilerFolder = join(folder, "compiler"),
    compiler = join(compilerFolder, "ISCC.exe");
  if (!existsSync(compiler))
    await unpackWindowsInstaller(root, archive, compilerFolder);
  const stage = join(folder, "application");
  mkdirSync(stage);
  linkSync(binary, join(stage, "ai-phone-client.exe"));
  for (const [input, output] of Object.entries(resourceMap(root))) {
    const target = join(stage, output);
    mkdirSync(dirname(target), { recursive: true });
    linkSync(join(root, input), target);
  }
  const version = json(join(root, "config/release-policy.json")).version;
  const qemu = join(stage, "host/qemu");
  const base = json(join(qemu, "manifest.json")).files.find(
    (file: { path: string }) => file.path === "guest/guest.qcow2",
  ).sha256;
  run([
    compiler,
    "/Qp",
    "/DAppVersion=" + version,
    "/DNumericVersion=" + version.split("-")[0] + ".0",
    "/DAppSource=" + stage,
    "/DProjectRoot=" + root,
    "/DBaseDiskSHA256=" + base,
    "/O" + dirname(destination),
    "/F" + basename(destination, ".exe"),
    join(root, "apps/native/windows/installer.iss"),
  ]);
  console.log(JSON.stringify({ installer: basename(destination), bytes: lstatSync(destination).size }));
}
