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
const root = process.env.AIMANITY_QA_ROOT;
if (process.env.RUNNER_ENVIRONMENT !== "github-hosted" || !root ||
    !/^[A-Z]:\\Dysprosium-QA-[0-9]+$/i.test(root) || home !== resolve(root, "android-only"))
  throw Error("Standalone diagnostic requires a disposable Windows runner");
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
const apk = join(root, "inputs", "input.apk");
function adbOutput(...args: string[]) {
  const result = spawnSync(adbPath, ["-P", "15037", "-s", serial, ...args], { maxBuffer: 1024 * 1024 });
  if (result.error) throw result.error;
  return {status:result.status, stdout:result.stdout.toString().trim(), stderr:result.stderr.toString().trim()};
}
const manager = () => adbOutput("shell",
  "dumpsys -t 3 accessibility | grep -Ei 'attributes:.*installedServiceCount|enabled services|binding services|PhoneAttention'");
const installedCount = (snapshot: ReturnType<typeof manager>) => {
  const found = /attributes:\{id=0,.*installedServiceCount=(\d+)/.exec(snapshot.stdout);
  if (!found) throw Error("Missing accessibility manager service count for the Android owner: " + JSON.stringify(snapshot));
  return Number(found[1]);
};
const baseline = manager();
const originalCount = installedCount(baseline);
console.log(JSON.stringify({ baseline, originalCount, currentUser:adbOutput("shell", "am get-current-user") }));
const packageResult = adbOutput("install", "--no-incremental", "-r", "--user", "0", apk);
console.log(JSON.stringify({inputApk:packageResult}));
if (packageResult.status !== 0 || packageResult.stdout.split(/\r?\n/).at(-1) !== "Success")
  throw Error("The sealed input package was not installed");
const registeredAt = performance.now();
let current = manager();
const immediateManagerCount = installedCount(current);
while (installedCount(current) === originalCount && performance.now() - registeredAt < 30_000) {
  await Bun.sleep(400);
  current = manager();
}
const confirmation = adbOutput("shell", "cmd appops get --user 0 " + packageName + " ACCESS_RESTRICTED_SETTINGS");
const distinctCount = installedCount(current);
console.log(JSON.stringify({ immediateManagerCount, distinctCount, registrationMs:Math.round(performance.now() - registeredAt), current, confirmation }));
if (distinctCount !== originalCount + 1) throw Error("PhoneAttention did not appear in the accessibility manager after the only APK install");
function states() {
  return {
    user:adbOutput("shell", "am get-started-user-state 0"),
    selector:adbOutput("shell", "settings --user 0 get secure " + enabled),
    global:adbOutput("shell", "settings --user 0 get secure accessibility_enabled"),
    component:adbOutput("shell", "cmd package query-services --user 0 --components -a android.accessibilityservice.AccessibilityService -p " + packageName),
    binding:adbOutput("shell", "dumpsys -t 2 activity services " + attention + " | grep -E 'ServiceRecord|PhoneAttention|No services|requested='"),
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
  const writtenAt = performance.now();
  console.log(JSON.stringify({diagnosticOnly:true, before,write,immediate}));
  for (const ms of [1000,5000,10000]) {
    await Bun.sleep(Math.max(0, writtenAt + ms - performance.now()));
    console.log(JSON.stringify({afterMs:Math.round(performance.now() - writtenAt), state:states()}));
  }
  const log = adbOutput("shell", "logcat -d -t 240 -s SettingsProvider AccessibilityManagerService EnhancedConfirmationService AppOps ActivityManager PackageManager");
  console.log(JSON.stringify({log}));
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
