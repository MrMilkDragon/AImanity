import {
  existsSync,
  readFileSync,
  mkdirSync,
  copyFileSync,
  chmodSync,
  mkdtempSync,
  renameSync,
  rmSync,
} from "node:fs";
import { join, basename, dirname } from "node:path";
import { hash, digest, output } from "./process";
import {
  json,
  save,
  collect,
  records,
  verifyFiles,
} from "./inventory";
import { extractTar, writeTar } from "./archive";

type DependencyProfile = {
  config: string;
  source: string[];
  roots: string[];
  required: string[];
  prefix: string;
};
const profiles: Record<string, DependencyProfile> = {
  "linux-ocr": {
    config: "config/linux-ocr.json",
    source: [
      "config/linux-ocr.json",
      "config/linux-factory.json",
      "scripts/tooling/factory-inputs.ts",
      "scripts/tooling/linux-dependencies.ts",
      "scripts/linux-ocr/Dockerfile",
      "scripts/linux-ocr/collect.ts",
    ],
    roots: ["bin", "lib", "tessdata", "licenses", "source-records.json"],
    required: [
      "bin/tesseract",
      "lib/ld-linux-x86-64.so.2",
      "lib/libtesseract.so.5",
      "tessdata/eng.traineddata",
      "tessdata/chi_sim.traineddata",
      "tessdata/configs/tsv",
      "source-records.json",
      "licenses/tesseract-ocr.copyright",
    ],
    prefix: "runtime/services/ocr/",
  },
};
export function dependencyFingerprint(
  root: string,
  profile: DependencyProfile,
) {
  return hash(
    JSON.stringify(
      profile.source.map((name) => [
        name,
        hash(readFileSync(join(root, name))),
      ]),
    ),
  );
}
export function dependencyArchive(root: string, role: string): string {
  const value = json(join(root, profiles[role]!.config));
  if (value.format !== 1 || value.host !== "linux-x64")
    throw Error("Unsupported Linux dependency");
  return value.archive.file;
}
export async function verifyDependency(
  root: string,
  folder: string,
  role: string,
) {
  const profile = profiles[role]!,
    value = json(join(folder, "manifest.json"));
  if (
    value.format !== 1 ||
    value.host !== "linux-x64" ||
    value.source_sha256 !== dependencyFingerprint(root, profile)
  )
    throw Error("Linux component identity changed");
  await verifyFiles(folder, value.files);
  return value;
}
export async function buildDependency(
  root: string,
  archive: string,
  destination: string,
  role: string,
) {
  const archiveSha256 = await digest(archive);
  if (existsSync(destination)) {
    const component = await verifyDependency(root, destination, role);
    if (component.archive_sha256 !== archiveSha256)
      throw Error("Linux dependency archive changed");
    return component;
  }
  const profile = profiles[role]!;
  mkdirSync(dirname(destination), { recursive: true });
  const work = mkdtempSync(join(dirname(destination), ".dependency-"));
  try {
    await extractTar(archive, work, "gzip", root);
    const files = collect(work, "").files;
    const names = new Set(files.map((f) => f.path.slice(1)));
    if (
      profile.required.some((name) => !names.has(name)) ||
      [...names].some((name) => !profile.roots.includes(name.split("/")[0]!))
    )
      throw Error("Linux dependency closure incomplete");
    const inventory = files.map((f) => ({
      ...f,
      path: f.path.slice(1),
    }));
    const value = {
      format: 1,
      host: "linux-x64",
      source_sha256: dependencyFingerprint(root, profile),
      archive_sha256: archiveSha256,
      files: records(inventory),
    };
    save(join(work, "manifest.json"), value);
    renameSync(work, destination);
    return value;
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}
export async function prepareDependency(
  root: string,
  folder: string,
  role: string,
) {
  await verifyDependency(root, folder, role);
  return collect(folder, profiles[role]!.prefix.replace(/\/$/, "")).files;
}
export function nativeClosure(
  root: string,
  programs: Record<string, string>,
) {
  const files: Record<string, { target: string; sha256: string }> = {};
  const explicit = new Set(Object.values(programs));
  for (const name of explicit) {
    const shared = output(["ldd", name])
      .split("\n")
      .flatMap((line) => {
        const match = line.match(/=>\s+(\/\S+)/) || line.match(/^\s*(\/\S+)/);
        return match ? [match[1]!] : [];
      });
    for (const file of [name, ...shared]) {
      const group = explicit.has(file) ? "bin" : "lib",
        target = group + "/" + basename(file),
        data = readFileSync(file),
        identity = hash(data),
        destination = join(root, target);
      if (
        existsSync(destination) &&
        hash(readFileSync(destination)) !== identity
      )
        throw Error("Conflicting native library basename");
      mkdirSync(dirname(destination), { recursive: true });
      copyFileSync(file, destination);
      chmodSync(destination, 0o755);
      files[file] = { target, sha256: identity };
    }
  }
  return files;
}
export function packageRecords(root: string, files: unknown) {
  const packages = output([
    "dpkg-query",
    "-W",
    "-f=${Package} ${Version}\n",
  ]).split("\n");
  mkdirSync(join(root, "licenses"), { recursive: true });
  for (const row of packages) {
    const name = row.split(" ")[0]!,
      source = join("/usr/share/doc", name, "copyright");
    if (existsSync(source))
      copyFileSync(source, join(root, "licenses", name + ".copyright"));
  }
  save(join(root, "source-records.json"), { native_files: files, packages });
}
export async function archiveDependency(folder: string, destination: string) {
  const files = collect(folder, "").files.map((f) => ({
    ...f,
    path: f.path.slice(1),
  }));
  await writeTar(destination, files, { codec: "gzip" });
}
