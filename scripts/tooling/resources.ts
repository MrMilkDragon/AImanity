import {
  createReadStream,
  readFileSync,
  existsSync,
  readdirSync,
  statSync,
} from "node:fs";
import { join, basename } from "node:path";
import { createHash } from "node:crypto";
import { ROOT, hash } from "./process";
import { json, secret, canonical } from "./inventory";

const WINDOW = 4096;
type Vector = { size: number; sha256: string };
export function maskVectors(data: Buffer, vectors: Vector[]) {
  const result = Buffer.from(data);
  for (const prefix of ["-----BEGIN ", "sk-"]) {
    let cursor = 0;
    while ((cursor = data.indexOf(prefix, cursor)) >= 0) {
      const start = cursor;
      cursor += prefix.length;
      for (const row of vectors) {
        const end = start + row.size;
        if (
          end <= data.length &&
          data[end - 1] === 0 &&
          hash(data.subarray(start, end)) === row.sha256
        ) {
          result.fill(32, start, end);
          break;
        }
      }
    }
  }
  return result;
}
export async function inspectStream(
  source: AsyncIterable<Uint8Array>,
  file: string,
  vectors: Vector[],
  publicBuildDigest?: string,
) {
  const measured = createHash("sha256");
  let size = 0,
    tail = Buffer.alloc(0),
    buildPath = false;
  function inspect(bytes: Buffer, boundary: number) {
    const masked = maskVectors(bytes, vectors);
    const compact = Buffer.from(masked.filter((byte) => byte !== 0));
    const compactBoundary =
      boundary -
      masked.subarray(0, boundary).filter((byte) => byte === 0).length;
    for (const [sample, end] of [
      [masked, boundary],
      [compact, compactBoundary],
    ] as const) {
      const match = secret.exec(sample.toString("latin1"));
      if (match && match.index < end)
        throw Error("Private material in bundle: " + basename(file));
    }
    buildPath ||= /\/Users\/[A-Za-z0-9._-]+\//.test(bytes.toString());
  }
  for await (const chunk of source) {
    const bytes = Buffer.from(chunk),
      sample = Buffer.concat([tail, bytes]);
    inspect(sample, Math.max(0, sample.length - WINDOW));
    measured.update(bytes);
    size += bytes.length;
    tail = Buffer.from(sample.subarray(-WINDOW));
  }
  inspect(tail, tail.length);
  const sha256 = measured.digest("hex");
  if (
    (publicBuildDigest && sha256 !== publicBuildDigest) ||
    (buildPath && sha256 !== publicBuildDigest)
  )
    throw Error("Unapproved build path in bundle: " + basename(file));
  return { size, sha256 };
}
export function fileReceipt(
  file: string,
  root = ROOT,
  publicBuildDigest?: string,
) {
  return inspectStream(
    createReadStream(file),
    file,
    json(join(root, "config/public-crypto-vectors.json")).vectors,
    publicBuildDigest,
  );
}
export const workbenchNotices = ".tools/iced-workbench-licenses.txt";
export const hostResources = "apps/native/gen/host";

export function resourceMap(root = ROOT) {
  const integrity = join(root, hostResources, "integrity.json");
  const resources: Record<string, string> = {
    LICENSE: "legal/LICENSE",
    NOTICE: "legal/NOTICE",
    "PRIVACY.md": "legal/PRIVACY.md",
    "DISCLAIMER.md": "legal/DISCLAIMER.md",
    "THIRD_PARTY_NOTICES.md": "legal/THIRD_PARTY_NOTICES.md",
    "docs/public/licenses/guest-apps/GPL-3.0.txt": "legal/GPL-3.0.txt",
    [workbenchNotices]: "legal/native-client-licenses.txt",
    "docs/public/licenses/whisper/LICENSE": "legal/whisper-LICENSE.txt",
    "docs/public/licenses/native/whisper-rs-LICENSE": "legal/whisper-rs-LICENSE.txt",
    "docs/public/licenses/native/whisper.cpp-LICENSE": "legal/whisper.cpp-LICENSE.txt",
    [hostResources + "/speech/ggml-tiny-q5_1.bin"]: "host/speech/ggml-tiny-q5_1.bin",
    [hostResources + "/source.payload"]: "host/source.payload",
  };
  if (existsSync(integrity)) {
    const expected = json(integrity);
    for (const name of ["android.payload", "images.payload"])
      if (name in expected) resources[hostResources + "/" + name] = "host/" + name;
    if ("qemu/manifest.json" in expected) {
        const manifestFile = join(root, hostResources, "qemu/manifest.json"),
          manifest = json(manifestFile);
        if (
          hash(readFileSync(manifestFile)) !== expected["qemu/manifest.json"]
        )
          throw Error("Resource manifest changed");
        const names = ["manifest.json", ...manifest.files.map((row: { path: string }) => row.path)];
        for (const name of names) {
          canonical(name);
          resources[`${hostResources}/qemu/${name}`] = `host/qemu/${name}`;
        }
    }
  }
  return resources;
}
export async function verifyCompiledInventory(
  binary: string,
  expected: Record<string, string>,
) {
  const missing = new Set(Object.values(expected));
  let tail = Buffer.alloc(0);
  for await (const chunk of createReadStream(binary)) {
    const sample = Buffer.concat([tail, Buffer.from(chunk)]);
    for (const value of missing)
      if (sample.includes(Buffer.from(value))) missing.delete(value);
    tail = Buffer.from(sample.subarray(-64));
  }
  if (missing.size)
    throw Error("Compiled resources differ from prepared payload");
}
