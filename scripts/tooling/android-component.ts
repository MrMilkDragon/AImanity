import { rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { ROOT, hostKey, nativeJson, output } from "./process";
import { json, type FileRecord } from "./inventory";
import { buildReleaseTools } from "./archive";

type AndroidManifest = {
  host: string;
  abi: string;
  engine: string;
  files: FileRecord[];
};

function androidReceipt(manifest: AndroidManifest, archiveIdentity: string) {
  return {
    host: manifest.host,
    abi: manifest.abi,
    engine: manifest.engine,
    files: manifest.files.length,
    bytes: manifest.files.reduce((total, row) => total + row.size, 0),
    archive_sha256: archiveIdentity,
    redistribution: "review-pending",
  };
}

type FactoryAndroidInput =
  | { kind: "sdk"; path: string }
  | { kind: "sealed"; file: string; identity: string };

export function factoryAndroidInput(): FactoryAndroidInput {
  const sdk = process.env.AIMANITY_ANDROID_SDK_INPUT;
  const file = process.env.AIMANITY_ANDROID_PAYLOAD;
  const identity = process.env.AIMANITY_ANDROID_PAYLOAD_SHA256;
  if (sdk && !file && !identity) return { kind: "sdk", path: resolve(sdk) };
  if (!sdk && file && identity && /^[a-f0-9]{64}$/.test(identity))
    return { kind: "sealed", file: resolve(file), identity };
  throw Error("Provide a staged Android SDK or a sealed Android payload with its SHA-256");
}

export async function prepareFactoryAndroid(
  input: FactoryAndroidInput,
  work: string,
  preset: number,
  root: string,
) {
  const file = input.kind === "sdk" ? join(work, "android.payload") : input.file;
  const identity = input.kind === "sdk"
    ? JSON.parse(output(
        [buildReleaseTools(root), "android-component", input.path, file, String(preset)],
        { cwd: root },
      )).archive_sha256
    : input.identity;
  const qualification = join(work, "android-qualification");
  const prepared = prepareAndroid(file, qualification, identity, root);
  const manifest = json<AndroidManifest>(join(prepared.sdkRoot, "..", "ANDROID-MANIFEST.json"));
  rmSync(qualification, { recursive: true });
  if (manifest.engine !== (hostKey() === "macos-arm64" ? "avo" : "whpx"))
    throw Error("The Android payload requires the current native engine");
  return { file, receipt: androidReceipt(manifest, identity) };
}

export function prepareAndroid(
  payload: string,
  home: string,
  payloadSha256: string,
  root = ROOT,
): {
  version: number;
  sdkRoot: string;
  avdHome: string;
  avdName: string;
  reusedSdk: boolean;
  reusedDevice: boolean;
} {
  return nativeJson(["--prepare-android", home, payload, payloadSha256], root);
}
