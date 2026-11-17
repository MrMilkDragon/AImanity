import {
  existsSync,
  mkdirSync,
  readFileSync,
  statfsSync,
  lstatSync,
} from "node:fs";
import { join, resolve, basename } from "node:path";
import { randomUUID } from "node:crypto";
import { run, output, digest, download } from "./process";
import { buildReleaseTools } from "./archive";
import { json } from "./inventory";
import { buildImages } from "./image-capsule";
import { verifyGuest } from "./guest-build";

export type Pin = {
  url: string;
  sha256: string;
  size_bytes?: number;
  file?: string;
};
export function capacity(root: string, minimumGiB = 35) {
  const disk = statfsSync(root);
  if (disk.bavail * disk.bsize < minimumGiB * 1024 ** 3)
    throw Error(`Factory requires ${minimumGiB} GiB free workspace`);
}
export async function fetchPin(folder: string, name: string, pin: Pin) {
  if (basename(name) !== name) throw Error("Invalid factory download name");
  mkdirSync(folder, { recursive: true });
  const file = join(folder, name);
  if (!existsSync(file)) await download(pin.url, file);
  if (
    lstatSync(file).isSymbolicLink() ||
    (pin.size_bytes !== undefined && lstatSync(file).size !== pin.size_bytes) ||
    (await digest(file)) !== pin.sha256
  )
    throw Error("Factory input differs: " + name);
  return file;
}
export function checkDocker() {
  output(["docker", "buildx", "version"]);
  const version = JSON.parse(
    output(["docker", "version", "--format", "{{json .}}"]),
  );
  for (const role of ["Client", "Server"]) {
    const [major, minor] = String(version[role].ApiVersion)
      .split(".")
      .map(Number);
    if (!(major! > 1 || (major === 1 && minor! >= 49)))
      throw Error("Factory requires Docker API 1.49 or newer");
  }
  const info = JSON.parse(output(["docker", "info", "--format", "{{json .}}"]));
  if (
    new Map(info.DriverStatus).get("driver-type") !==
    "io.containerd.snapshotter.v1"
  )
    throw Error("Factory requires the platform-aware image store");
}
export function compilerEnvironment(sdk: string): NodeJS.ProcessEnv {
  sdk = resolve(sdk);
  const java = process.env.JAVA_HOME;
  if (
    !java ||
    ![
      "platforms/android-35/android.jar",
      ...["aapt2", "d8", "apksigner"].map(
        (name) => "build-tools/35.0.0/" + name,
      ),
    ].every((name) => existsSync(join(sdk, name))) ||
    !existsSync(join(java, "bin/javac"))
  )
    throw Error("Factory requires Android 35 build tools and Java 17");
  return {
    ...process.env,
    ANDROID_HOME: sdk,
    ANDROID_SDK_ROOT: sdk,
    JAVA_HOME: resolve(java),
  };
}
export function prepareApps(root: string, env: NodeJS.ProcessEnv) {
  return JSON.parse(output([buildReleaseTools(root), "phone-apps", "build", root], { cwd: root, env }));
}
export function databaseImage(root: string, role: "postgres" | "redis") {
  const compose = readFileSync(join(root, "compose.yaml"), "utf8"),
    matches = [
      ...compose.matchAll(
        new RegExp("\\$\\{AI_PHONE_" + role.toUpperCase() + "_IMAGE:-([^}]+)\\}", "g"),
      ),
    ],
    image = matches[0]?.[1];
  if (
    matches.length !== 1 ||
    !image ||
    !new RegExp("^" + role + ":[^@]+@sha256:[0-9a-f]{64}$").test(image)
  )
    throw Error("Immutable database image pin missing");
  return role + "@" + image.split("@")[1];
}
export async function prepareImages(
  root: string,
  cache: string,
  identity: string,
  architecture: "arm64" | "amd64",
  guest?: string,
  databaseImages = true,
) {
  if (!/^[0-9a-f]{64}$/.test(identity))
    throw Error("Factory source identity missing");
  const prefix =
    "ai-phone-factory/" +
    identity.slice(0, 12) +
    "-" +
    randomUUID().replaceAll("-", "").slice(0, 12);
  const sources: Record<string, string> = {};
  const databaseRoles = ["postgres", "redis"] as const;
  for (const role of databaseImages ? databaseRoles : []) {
    const reference = databaseImage(root, role);
    const platform = "linux/" + architecture;
    const cached = Bun.spawnSync(["docker", "image", "inspect", "--platform", platform, reference], {
      stdout: "ignore",
      stderr: "ignore",
    });
    if (cached.exitCode !== 0)
      run(["docker", "pull", "--platform", platform, reference], { cwd: root });
    sources[role] = reference;
  }
  if (guest) await verifyGuest(root, guest, architecture);
  for (const role of ["workspace", "desktop"]) {
    const reference = prefix + "/" + role + ":build";
    const present = Bun.spawnSync(["docker", "image", "inspect", reference], {
      stdout: "ignore",
      stderr: "ignore",
    });
    if (present.exitCode === 0) throw Error("Factory image already exists");
    const args = [
      "docker",
      "buildx",
      "build",
      "--platform",
      "linux/" + architecture,
      "--load",
      "--provenance=false",
      "--tag",
      reference,
    ];
    for (const variable of ["HTTP_PROXY", "HTTPS_PROXY"])
      if (process.env[variable]) args.push("--build-arg", variable);
    if (role === "desktop")
      args.push("--build-arg", "WORKSPACE_IMAGE=" + sources.workspace);
    if (role === "workspace" && guest)
      args.push(
        "--build-arg",
        "GUEST_RUNTIME_SOURCE=guest_runtime",
        "--build-context",
        "guest_runtime=" + resolve(guest),
      );
    args.push("--file", join(root, "infra", role, "Dockerfile"), root);
    run(args, { cwd: root });
    sources[role] = reference;
  }
  const file = join(cache, "images.payload");
  return { file, receipt: await buildImages(root, sources, file) };
}
