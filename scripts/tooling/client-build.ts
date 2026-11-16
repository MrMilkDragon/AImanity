import {
  readFileSync,
  createReadStream,
  mkdirSync,
  readdirSync,
  lstatSync,
  realpathSync,
  mkdtempSync,
  rmSync,
  linkSync,
  symlinkSync,
  renameSync,
  existsSync,
  copyFileSync,
  chmodSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { join, relative, resolve, basename, dirname, sep } from "node:path";
import { homedir, tmpdir } from "node:os";
import { ROOT, run, output, digest, hash } from "./process";
import { json, save } from "./inventory";
import { buildWindowsInstaller, unpackWindowsInstaller } from "./windows-installer";
import {
  macReleaseSigning,
  notarizeMacImage,
  releaseMacSignature,
  releaseWindowsSignature,
  signMacArtifact,
  signWindowsInstaller,
  signWindowsRelease,
  temporaryMacSignature,
  verifyMacArtifact,
  windowsReleaseSigning,
  type InstallerSignature,
} from "./installer-signing";
import { resourceMap, fileReceipt, verifyCompiledInventory, workbenchNotices, hostResources } from "./resources";
import { rustNotices } from "./dependency-inventory";
import {
  payloadComponents,
  deliveryMetadata,
  installerMetadata,
} from "./client-payload";

export type Platform = "macos" | "windows";
type Receipt = { path: string; size: number; sha256: string };
function filesIn(base: string) {
  const result: Record<string, string> = {};
  const walk = (folder: string) => {
    for (const item of readdirSync(folder, { withFileTypes: true })) {
      const file = join(folder, item.name);
      if (item.isDirectory()) walk(file);
      else if (item.isSymbolicLink()) {
        const target = realpathSync(file);
        if (!target.startsWith(resolve(base) + sep))
          throw Error("Installer link leaves payload");
        if (lstatSync(target).isFile())
          result[relative(base, file).split(sep).join("/")] = file;
      } else if (item.isFile())
        result[relative(base, file).split(sep).join("/")] = file;
    }
  };
  walk(base);
  return result;
}
function noticeManifest(root: string) {
  return {
    rust_missing_files: [],
    texts: { "native-client-licenses.txt": hash(readFileSync(join(root, workbenchNotices))) },
  };
}
async function inspectPayload(files: Record<string, string>, root: string) {
  const resources = resourceMap(root),
    integrity = json<Record<string, string>>(join(root, hostResources, "integrity.json")),
    vmManifest = hostResources + "/qemu/manifest.json",
    vmFiles = new Map<string, Receipt>(
      vmManifest in resources
        ? json<{ files: Receipt[] }>(join(root, vmManifest)).files.map(
            (row) => ["host/qemu/" + row.path, row],
          )
        : [],
    );
  const expected = new Map<string, Omit<Receipt, "path">>();
  for (const [source, destination] of Object.entries(resources)) {
    const path = join(root, source);
    const digest = integrity[destination.slice("host/".length)];
    expected.set(destination, vmFiles.get(destination) ?? (digest
      ? { size: lstatSync(path).size, sha256: digest }
      : await fileReceipt(path, root)));
  }
  const seen = new Set<string>(),
    result: Receipt[] = [];
  for (const [name, file] of Object.entries(files).sort(([a], [b]) =>
    a.localeCompare(b),
  )) {
    const destination = [...expected.keys()].find(
      (s) => name === s || name.endsWith("/" + s),
    );
    if (
      !destination &&
      /(?:^|\/)host\/(?:(?:wsl|qemu)\/|windows\/wsl\.msi$)/i.test(name)
    )
      throw Error("Unexpected Windows backend installer resource");
    const receipt = await fileReceipt(file, root);
    if (destination) {
      const required = expected.get(destination)!;
      if (receipt.sha256 !== required.sha256 || receipt.size !== required.size)
        throw Error("Stale installer resource: " + destination);
      seen.add(destination);
    }
    result.push({ path: name, ...receipt });
  }
  if (seen.size !== expected.size)
    throw Error("Installer resources are incomplete");
  return result;
}
async function inspectPackage(file: string, root: string) {
  const folder = mkdtempSync(join(tmpdir(), "aiphone-package-"));
  try {
    await unpackWindowsInstaller(root, file, folder);
    const files = filesIn(folder),
      program = Object.entries(files).find(([name]) =>
        name.endsWith("ai-phone-client.exe"),
      );
    if (!program) throw Error("Windows client executable is missing");
    const header = readFileSync(program[1]),
      pe = header.readUInt32LE(60);
    if (
      header.subarray(0, 2).toString() !== "MZ" ||
      header.subarray(pe, pe + 4).toString() !== "PE\0\0" ||
      header.readUInt16LE(pe + 4) !== 0x8664
    )
      throw Error("Unexpected Windows client architecture");
    const integrity = json(join(root, hostResources, "integrity.json"));
    if ("qemu/manifest.json" in integrity)
      await verifyCompiledInventory(program[1], integrity);
    if (process.platform === "win32")
      run([program[1], "--verify-bundled-host", join(dirname(program[1]), "host")]);
    return { files: await inspectPayload(files, root) };
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
}
async function inspectMacBundle(bundle: string, root: string) {
  const config = json(join(root, "config/native-client.json"));
  const info = JSON.parse(
    output([
      "plutil",
      "-convert",
      "json",
      "-o",
      "-",
      join(bundle, "Contents/Info.plist"),
    ]),
  );
  const version = json(join(root, "config/release-policy.json")).version;
  if (
    info.CFBundleIdentifier !== config.bundle_id ||
    info.CFBundleExecutable !== config.executable ||
    info.CFBundleShortVersionString !== version ||
    info.LSMinimumSystemVersion !== config.minimum_macos
  )
    throw Error("Unexpected application metadata");
  const executable = info.CFBundleExecutable;
  if (typeof executable !== "string" || basename(executable) !== executable)
    throw Error("Invalid application executable");
  const program = join(bundle, "Contents/MacOS", executable);
  const integrity = join(root, hostResources, "integrity.json");
  if (existsSync(integrity))
    await verifyCompiledInventory(program, json(integrity));
  const allowed = new Set([
    "Contents/Info.plist",
    "Contents/MacOS/" + executable,
    "Contents/Resources/icon.icns",
    ...Object.values(resourceMap(root)).map((s) => "Contents/Resources/" + s),
  ]);
  const files = filesIn(bundle);
  const signature = "Contents/_CodeSignature/CodeResources";
  if (Object.hasOwn(files, signature)) allowed.add(signature);
  if (
    Object.keys(files).length !== allowed.size ||
    Object.keys(files).some((name) => !allowed.has(name))
  )
    throw Error("Unexpected application files");
  run([program, "--verify-bundled-host", join(bundle, "Contents/Resources/host")]);
  const inspected = await inspectPayload(files, root);
  return {
    bundle_id: config.bundle_id,
    version,
    minimum_macos: info.LSMinimumSystemVersion,
    files: inspected,
  };
}
function workbenchTarget(platform: Platform) {
  return platform === "windows" ? "x86_64-pc-windows-msvc" : "aarch64-apple-darwin";
}

async function prepareWorkbenchNotices(root: string, platform: Platform) {
  const value = await rustNotices(root, "apps/native/Cargo.toml", "Iced workbench", workbenchTarget(platform));
  if (value.missing.length) throw Error("Missing workbench notices: " + value.missing.join(", "));
  const file = join(root, workbenchNotices);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, value.text);
}

function nativeBinary(root: string, platform: Platform) {
  const folder = join(root, "apps/native"),
    target = workbenchTarget(platform),
    env = nativeEnvironment();
  env.AIMANITY_BUNDLE_INTEGRITY = join(root, hostResources, "integrity.json");
  if (platform === "windows")
    env.CARGO_ENCODED_RUSTFLAGS += "\x1f-Ctarget-feature=+crt-static";
  run(["cargo", "build", "--release", "--locked", "--target", target,
    "--manifest-path", join(folder, "Cargo.toml")], { cwd: root, env });
  return join(folder, "target", target, "release", platform === "windows" ? "aimanity-native.exe" : "aimanity-native");
}

function macNativeBundle(root: string, stage: string, binary: string, version: string) {
  const config = json(join(root, "config/native-client.json")),
    bundle = join(stage, "AImanity.app"),
    contents = join(bundle, "Contents"),
    executable = join(contents, "MacOS", config.executable),
    resources = join(contents, "Resources");
  mkdirSync(dirname(executable), { recursive: true });
  copyFileSync(binary, executable);
  chmodSync(executable, 0o755);
  mkdirSync(resources);
  linkSync(join(root, "apps/native/icons/icon.icns"), join(resources, "icon.icns"));
  for (const [input, output] of Object.entries(resourceMap(root))) {
    const target = join(resources, output);
    mkdirSync(dirname(target), { recursive: true });
    linkSync(join(root, input), target);
  }
  const info = join(contents, "Info.plist");
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw Error("Invalid app version");
  writeFileSync(info, `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>${config.bundle_id}</string>
  <key>CFBundleName</key><string>AImanity</string>
  <key>CFBundleDisplayName</key><string>AImanity</string>
  <key>CFBundleExecutable</key><string>${config.executable}</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>${config.minimum_macos}</string>
  <key>NSMicrophoneUsageDescription</key><string>Copilot transcribes your speech locally.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>`);
  return bundle;
}
export function nativeEnvironment(): NodeJS.ProcessEnv {
  const home = homedir();
  const nativePrefixMap = `-ffile-prefix-map=${home}=/build-user`;
  return {
    ...process.env,
    CI: "true",
    CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || "2",
    CARGO_INCREMENTAL: "0",
    CARGO_HOME: process.env.CARGO_HOME || join(home, ".cargo"),
    ...(process.platform === "darwin" ? {
      CFLAGS: [process.env.CFLAGS, nativePrefixMap].filter(Boolean).join(" "),
      CXXFLAGS: [process.env.CXXFLAGS, nativePrefixMap].filter(Boolean).join(" "),
    } : {}),
    CARGO_ENCODED_RUSTFLAGS: [
      process.env.CARGO_ENCODED_RUSTFLAGS,
      `--remap-path-prefix=${home}=/build-user`,
    ]
      .filter(Boolean)
      .join("\x1f"),
  };
}
function buildDmg(
  application: string,
  destination: string,
  { temporarySigning, releaseSigning }: {
    temporarySigning: boolean;
    releaseSigning?: ReturnType<typeof macReleaseSigning>;
  },
) {
  const temporary = mkdtempSync(join(dirname(destination), ".dmg-"));
  try {
    const contents = join(temporary, "contents");
    mkdirSync(contents);
    const copy = (from: string, to: string) => {
      mkdirSync(to);
      for (const entry of readdirSync(from, { withFileTypes: true })) {
        if (entry.isDirectory())
          copy(join(from, entry.name), join(to, entry.name));
        else linkSync(join(from, entry.name), join(to, entry.name));
      }
    };
    copy(application, join(contents, "AImanity.app"));
    symlinkSync("/Applications", join(contents, "Applications"));
    const image = join(temporary, "client.dmg");
    run([
      "hdiutil",
      "create",
      "-srcfolder",
      contents,
      "-volname",
      "AImanity",
      "-format",
      "UDZO", "-imagekey", "zlib-level=1",
      image,
    ]);
    if (releaseSigning || temporarySigning) {
      signMacArtifact(image, releaseSigning?.identity);
      if (releaseSigning) notarizeMacImage(image, releaseSigning.profile);
      verifyMacArtifact(image);
    }
    run(["hdiutil", "verify", image]);
    if (releaseSigning || temporarySigning) {
      const mount = join(temporary, "readback");
      mkdirSync(mount);
      run([
        "hdiutil", "attach", "-readonly", "-nobrowse", "-noverify",
        "-owners", "off", "-mountpoint", mount, image,
      ]);
      try {
        verifyMacArtifact(join(mount, "AImanity.app"));
      } finally {
        run(["hdiutil", "detach", mount]);
      }
    }
    renameSync(image, destination);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}
async function recordInstaller(
  file: string,
  platform: Platform,
  arch: string,
  inspection: unknown,
  root = ROOT,
  signature: InstallerSignature = { signing: "unsigned" },
) {
  const githubHash = createHash("sha256"),
    updateHash = createHash("sha512");
  for await (const chunk of createReadStream(file)) {
    githubHash.update(chunk);
    updateHash.update(chunk);
  }
  const host = platform + "-" + arch;
  const delivery = deliveryMetadata(await payloadComponents(root), host);
  const manifest = {
    version: json(join(root, "config/release-policy.json")).version,
    kind: "native-client",
    interface: "iced-wgpu",
    os: platform,
    arch,
    sha256: githubHash.digest("hex"),
    sha512: updateHash.digest("base64"),
    ...signature,
    runtime_acceptance: "pending",
    dependencies_license_review_complete: false,
    remote_upload: false,
    third_party_notices: noticeManifest(root),
    source_commit: output(["git", "rev-parse", "HEAD"], { cwd: root }),
    builder_commit: output(["git", "rev-parse", "HEAD"], { cwd: ROOT }),
    source_locks: Object.fromEntries(
      await Promise.all(
        ["bun.lock", "apps/native/Cargo.lock"].map(async (name) => [
          name,
          await digest(join(root, name)),
        ]),
      ),
    ),
    source_payload_included: true,
    local_bootstrap: {
      implemented_on: [
        ...Object.keys(
          json(join(root, "config/host-bootstrap.json")).hosts,
        ).filter((host) => host === "macos-arm64"),
        ...(platform === "windows" && delivery.complete_device_bundle
          ? ["windows-x64"]
          : []),
      ].sort(),
      fresh_install_acceptance: "pending",
      user_state_bundled: false,
    },
    payload: inspection,
    ...delivery,
    installer_delivery: installerMetadata(file, host),
    ...(platform === "macos" && signature.signing !== "developer-id"
      ? { developer_id_signed: false, notarized: false }
      : {}),
  };
  save(file + ".manifest.json", manifest);
  return manifest;
}
export async function buildClient(
  platform: Platform,
  root = ROOT,
  { temporarySigning = false, releaseSigning = false } = {},
) {
  if (releaseSigning && temporarySigning)
    throw Error("Select release signing or a local test build");
  await prepareWorkbenchNotices(root, platform);
  const version = json(join(root, "config/release-policy.json")).version,
    dist = join(root, "dist");
  mkdirSync(dist, { recursive: true });
  if (platform === "macos") {
    if (process.platform !== "darwin" || process.arch !== "arm64")
      throw Error("macOS packaging requires Apple Silicon");
    const credentials = releaseSigning ? macReleaseSigning() : undefined;
    const binary = nativeBinary(root, platform);
    const stage = mkdtempSync(join(dist, ".native-app-"));
    try {
      const source = macNativeBundle(root, stage, binary, version);
      if (credentials || temporarySigning) signMacArtifact(source, credentials?.identity);
      const inspection = await inspectMacBundle(source, root);
      const destination = join(dist, `AImanity-${version}-macos-arm64.dmg`);
      buildDmg(source, destination, { temporarySigning, releaseSigning: credentials });
      return { artifact: destination, ...(await recordInstaller(destination, platform, "arm64", inspection, root,
        credentials ? releaseMacSignature : temporarySigning ? temporaryMacSignature : undefined)) };
    } finally {
      rmSync(stage, { recursive: true, force: true });
    }
  }
  if (process.platform !== "win32")
    throw Error("Build Windows clients on Windows");
  const credentials = releaseSigning ? windowsReleaseSigning() : undefined;
  const source = nativeBinary(root, platform);
  const destination = join(dist, `AImanity-${version}-windows-x64-setup.exe`);
  if (credentials) signWindowsRelease(source, credentials);
  await buildWindowsInstaller(root, source, destination);
  if (credentials) signWindowsRelease(destination, credentials);
  const signature = credentials
    ? releaseWindowsSignature
    : temporarySigning ? signWindowsInstaller(destination) : undefined;
  return {
    artifact: destination,
    ...(await recordInstaller(
      destination,
      platform,
      "x64",
      await inspectPackage(destination, root),
      root,
      signature,
    )),
  };
}
