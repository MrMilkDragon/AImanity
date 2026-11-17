import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { spawn, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { connect } from "node:net";
import { join, resolve } from "node:path";
import {
  NativeAndroidSessionControlSchema,
  NativeAndroidSessionReplySchema,
  NativeAndroidStartSchema,
} from "../packages/protocol/src/gen/ai_phone/v1/native_android_pb";
import { digest } from "./tooling/process";
import { prepareAndroid } from "./tooling/android-component";

const [input, destination] = process.argv.slice(2);
if (!input || !destination) throw Error("Provide the new Android capsule and clean test home");
const capsule = resolve(input), home = resolve(destination);
if (process.env.RUNNER_ENVIRONMENT !== "github-hosted" || !/^[A-Z]:\Dysprosium-QA-[0-9]+[\/]/i.test(home)) throw Error("Standalone diagnostic requires a disposable Windows runner");
const unpackStarted = performance.now();
const phone = await prepareAndroid(capsule, home, await digest(capsule));
console.log("Local Android SDK prepared in", Math.round(performance.now() - unpackStarted), "ms");
const worker = spawn(join("target", "release", "host-resources.exe"), ["--android-session"], {
  stdio: ["pipe", "pipe", "inherit"],
});
function send(body: Uint8Array) {
  const header = Buffer.alloc(4);
  header.writeUInt32BE(body.length);
  worker.stdin.write(Buffer.concat([header, Buffer.from(body)]));
}
const stdout = worker.stdout[Symbol.asyncIterator]();
const finished = new Promise<number | null>((success, fail) => {
  worker.once("error", fail);
  worker.once("exit", (code) => success(code));
});
let verified = false;
try {
  const start = performance.now();
  send(toBinary(NativeAndroidStartSchema, create(NativeAndroidStartSchema, { phone })));
  let frame = Buffer.alloc(0);
  while (frame.length < 4 || frame.length < frame.readUInt32BE() + 4) {
    const chunk = await stdout.next();
    if (chunk.done) throw Error("Android session exited without a boot receipt");
    frame = Buffer.concat([frame, chunk.value]);
  }
  const { result } = fromBinary(NativeAndroidSessionReplySchema, frame.subarray(4, frame.readUInt32BE() + 4));
  if (result.case !== "booted") throw Error("Android boot failed: " + JSON.stringify(result));
  const { serial, rootVerified, width, height } = result.value;
  if (!rootVerified || serial !== "127.0.0.1:5565" || width < 1 || height < 1)
    throw Error("Native boot receipt did not prove the expected root device");
  const adbPath = join(phone.sdkRoot, "platform-tools/adb.exe");
  function adb(...args: string[]) {
    const reply = spawnSync(adbPath, ["-P", "15037", "-s", serial, ...args], { maxBuffer: 8 * 1024 * 1024 });
    if (reply.error) throw reply.error;
    if (reply.status !== 0) throw Error(`adb ${args[0]} failed: ${reply.stderr.toString().trim() || reply.stdout.toString().trim()}`);
    return reply.stdout;
  }
  if (adb("shell", "id", "-u").toString().trim() !== "0") throw Error("Independent ADB root readback failed");
const packageName = "dev.aiphone.input";
const attention = packageName + "/.PhoneAttention";
const enabled = "enabled_accessibility_services";
const apk = process.env.AIMANITY_QA_INPUT_APK;
if (!apk || process.env.RUNNER_ENVIRONMENT !== "github-hosted" || !/^[A-Z]:\\Dysprosium-QA-[0-9]+[\\/]/i.test(home)) {
  throw Error("Expected a fresh owned Windows Android diagnostic with the sealed APK");
}
function adbOutput(...args: string[]) {
  const result = spawnSync(adbPath, ["-P", "15037", "-s", serial, ...args], { maxBuffer: 1024 * 1024 });
  if (result.error) throw result.error;
  return {status:result.status, stdout:result.stdout.toString().trim(), stderr:result.stderr.toString().trim()};
}
const packageResult = adbOutput("install", "--no-incremental", "-r", "--user", "0", apk);
console.log(JSON.stringify({inputApk:packageResult}));
if (packageResult.status !== 0 || packageResult.stdout !== "Success") throw Error("The sealed input package was not installed");
function states() {
  return {
    user:adbOutput("shell", "am get-started-user-state 0"),
    selector:adbOutput("shell", "settings --user 0 get secure " + enabled),
    global:adbOutput("shell", "settings --user 0 get secure accessibility_enabled"),
    component:adbOutput("shell", "cmd package query-services --user 0 --components -a android.accessibilityservice.AccessibilityService -p " + packageName),
    binding:adbOutput("shell", "dumpsys activity services " + attention + " | grep -E 'ServiceRecord|PhoneAttention|No services'"),
  };
}
const before = states();
if (before.selector.status !== 0) throw Error("Secure selector could not be read before replay");
const selected = before.selector.stdout;
const rows = selected === "null" || !selected ? [] : selected.split(":");
if (rows.some(item => !/^[A-Za-z0-9_.$]+\/[A-Za-z0-9_.$]+$/.test(item))) throw Error("Unexpected selector readback");
if (rows.some(item => item === attention || item === packageName + "/" + packageName + ".PhoneAttention")) {
  console.log(JSON.stringify({diagnosticOnly:true, settingAlreadyEnabled:true, before}));
} else {
  const write = adbOutput("shell", "settings --user 0 put secure " + enabled + " '" + [...rows,attention].join(":") + "'");
  const immediate = adbOutput("shell", "settings --user 0 get secure " + enabled);
  await Bun.sleep(1000);
  const after = states();
  const log = adbOutput("shell", "logcat -d -t 160 -s SettingsProvider AccessibilityManagerService ActivityManager PackageManager");
  console.log(JSON.stringify({diagnosticOnly:true, before,write,immediate,after,log}));
}
verified = true;
} catch (error) {
  for (const path of [join(phone.avdHome, "..", "native", "sdk.log"), join(phone.avdHome, "cuttlefish", "kernel.log")]) {
    try {
      const lines = readFileSync(path, "utf8").split("\n");
      if (path.endsWith("kernel.log"))
        console.error(lines.filter(line => /service 'console'|\badbd\b|sys\.boot_completed|Starting kernel/.test(line)).slice(0, 30).join("\n"));
      console.error(lines.slice(-80).join("\n"));
    }
    catch {}
  }
  throw error;
} finally {
  if (worker.exitCode === null) {
    send(toBinary(NativeAndroidSessionControlSchema, create(NativeAndroidSessionControlSchema, { cancelStartup: true })));
    worker.stdin.end();
  }
  const exited = await finished;
  if (verified && exited !== 0) throw Error("Android owner did not stop cleanly: " + exited);
}
