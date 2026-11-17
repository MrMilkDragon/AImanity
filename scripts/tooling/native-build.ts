import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ROOT, output, hash, hostKey } from "./process";
import type { SourceFile } from "./source";
import { collect, json, records, publicBytes } from "./inventory";
import { buildReleaseTools } from "./archive";
import { fetchPin } from "./factory";
import { gunzipSync } from "node:zlib";

export async function prepareServices(root = ROOT, linuxOcr?: string) {
  const release = buildReleaseTools(root);
  const receipt = JSON.parse(output([release, "native-services", "receipt"], { cwd: root }));
  const { assistant, runner, vision, vision_version, ...contracts } = receipt.checks;
  const files: SourceFile[] = [];
  for (const role of Object.keys(receipt.artifacts)) {
    const path = receipt.artifacts[role].path;
    files.push({
      path: "runtime/services/bin/" + role,
      data: readFileSync(join(root, path)),
      mode: 0o755,
    });
  }
  const mihomo = json(join(root, "config/mihomo-release.json"));
  const core = mihomo.assets[hostKey()];
  if (core) {
    const folder = join(root, ".tools/mihomo-harness");
    const archive = await fetchPin(
      folder,
      core.url.split("/").at(-1)!,
      core,
    );
    const license = await fetchPin(folder, "LICENSE", mihomo.verification_download);
    const data = gunzipSync(readFileSync(archive));
    if (hash(data) !== core.binary_sha256) throw Error("Mihomo build differs from its pin");
    files.push({path: "runtime/services/bin/mihomo", data, mode: 0o755});
    files.push({path: "runtime/services/licenses/mihomo-LICENSE.txt", data: readFileSync(license), mode: 0o644});
  }
  const bridge = process.env.AI_PHONE_RELEASE_DESKTOP_BRIDGE;
  if (bridge) {
    const data = publicBytes(readFileSync(bridge), "desktop-bridge");
    const machines: Record<string, number> = { "macos-arm64": 183, "linux-x64": 62 };
    const machine = machines[hostKey()];
    if (
      !machine ||
      data.length < 64 ||
      data.subarray(0, 6).toString("hex") !== "7f454c460201" ||
      data.readUInt16LE(18) !== machine
    )
      throw Error("Desktop bridge must be a matching Linux ELF");
    files.push({
      path: "runtime/services/bin/desktop-bridge",
      data,
      mode: 0o755,
    });
  }
  files.push({
    path: "runtime/services/fixtures/ocr.png",
    data: readFileSync(join(root, "crates/phone-runner/fixtures/ocr-probe.png")),
    mode: 0o644,
  });
  if (linuxOcr) {
    output([release, "linux-ocr", "verify", linuxOcr], { cwd: root });
    files.push(...collect(linuxOcr, "runtime/services/ocr"));
  }
  files.push({
    path: "runtime/services/manifest.json",
    data: Buffer.from(
      JSON.stringify({
        format: 1,
        host: hostKey(),
        source_sha256: receipt.source_sha256,
        compiler: receipt.compiler,
        versions: { assistant: assistant.version, runner: runner.version },
        vision: linuxOcr ? "linux-tesseract-bundled" : "container-tesseract",
        files: records(files),
        ...contracts,
      }),
    ),
    mode: 0o644,
  });
  return files;
}
