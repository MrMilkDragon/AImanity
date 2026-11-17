import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { ROOT, digest, run } from "../../exact/scripts/tooling/process";
import { fetchPin } from "../../exact/scripts/tooling/factory";
import { json } from "../../exact/scripts/tooling/inventory";
import { unpackWindowsInstaller } from "../../exact/scripts/tooling/windows-installer";

const root = process.env.AIMANITY_QA_ROOT;
if (process.platform !== "win32" || process.env.RUNNER_ENVIRONMENT !== "github-hosted" ||
    !root || !/^[A-Z]:\\Dysprosium-QA-[0-9]+$/i.test(root)) throw Error("Expected an isolated Windows runner");
const pin = json(join(ROOT, "config/windows-qemu.json")).calcium_installer;
const input = join(root,"portable"), installer = await fetchPin(input, pin.file, pin);
await unpackWindowsInstaller(ROOT, installer, input, [
  "{app}\\ai-phone-client.exe", "{app}\\host\\android.payload", "{app}\\host\\qemu\\manifest.json",
]);
rmSync(installer);
const capsule = join(input,"host","android.payload");
if (await digest(capsule) !== process.env.CALCIUM_ANDROID_SHA) throw Error("The AOSP capsule differs from the original QA input");
const worker = join(input,"ai-phone-client.exe");
run([worker,"--prepare-windows-host"]);
const target = join(ROOT,"target","release");
mkdirSync(target,{recursive:true});
copyFileSync(worker,join(target,"host-resources.exe"));
