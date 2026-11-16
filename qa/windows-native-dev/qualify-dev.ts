import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { deepEqual, equal, match, ok } from "node:assert/strict";
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { join } from "node:path";
import { LinuxVmControlReplySchema, LinuxVmControlRequestSchema, LinuxVmNativeCredentialsSchema, LinuxVmOperation } from "../packages/protocol/src/gen/ai_phone/v1/host_bridge_pb";
import { ControlStateSchema, DeviceActionRequestSchema, DeviceActionResponseSchema, TakeControlRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/phone_runtime_pb";
import type { MessageInitShape } from "@bufbuild/protobuf";
import { RegistryRequestSchema, RegistryStateSchema } from "../packages/protocol/src/gen/ai_phone/v1/registry_pb";

const root = process.env.AIMANITY_QA_ROOT;
const source = process.env.PRODUCT_SOURCE;
ok(root && process.platform === "win32" && process.env.RUNNER_ENVIRONMENT === "github-hosted" && /^[A-Z]:\\Dysprosium-QA-/.test(root), "Run on the disposable Windows acceptance host");
ok(source && /^[0-9a-f]{40}$/.test(source), "Supply the reviewed source commit");
const started = performance.now();
const dev = join(root, "dev");
const guestProfile = join(root, "profile");
const home = join(guestProfile, "host");
const checkpoint = join(home, "deployment.json");
ok(!existsSync(home) && existsSync(join(dev, "aimanity-native.exe")), "fresh isolated development runtime");
const integrity = await Bun.file(join(dev, "host", "integrity.json")).json();
const manifest = await Bun.file(join(dev, "host", "qemu", "manifest.json")).json();
equal(manifest.source_commit, source);
const payload = { sha256: integrity["source.payload"] };
ok(/^[0-9a-f]{64}$/.test(payload.sha256), "exact product source payload");
const env = { ...process.env, AIMANITY_DATA_DIR: guestProfile, AIMANITY_QA_HOST: home,
  AIMANITY_QA_SOURCE_PAYLOAD: payload.sha256, AIMANITY_QA_MODEL_URL: "http://10.0.2.2:17998/v1" };
const program = join(dev, "aimanity-native.exe");
const log = join(root, "native.log");
const prepare = Bun.spawn([program, "--prepare-windows-host"], {cwd:dev, env, stdout:"inherit", stderr:"inherit"});
equal(await prepare.exited, 0, "WHPX available on this disposable QA runner without pending restart");
const launch = (file: string) => Bun.spawn([program], { cwd: dev, env, stdout: "ignore", stderr: Bun.file(file) });
let app = launch(log);
function gui(action: "fresh" | "capture" | "close") {
  return Bun.spawn(["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
    "../qa/windows-native-dev/window.ps1", "-AppId", String(app.pid), "-Action", action],
  { env, stdout: "inherit", stderr: "inherit" }).exited;
}
async function stopApp() {
  if (app.exitCode == null) {
    equal(await gui("close"), 0, "close the owned native client");
    const code = await Promise.race([app.exited, Bun.sleep(120000).then(() => undefined)]);
    equal(code, 0, "native client normal shutdown");
  }
}
async function probe(command: string, argument?: string) {
  const args = [process.execPath, "scripts/windows-device-qa-probe.ts", command];
  if (argument) args.push(argument);
  const process_ = Bun.spawn(args, { env, stdout: "inherit", stderr: "inherit" });
  equal(await process_.exited, 0, command);
}
const androidRoot = join(home, "user", "data", "android");
const logPaths = ["user/qemu/owner.log", "user/qemu/boot.log", "user/qemu/qemu.log",
  "user/data/android/native/sdk.log", "user/data/android/whpx-avd/cuttlefish/kernel.log", "deployment.json"];
let androidStarted = 0;
let reportedAndroid = false;
let reportedListeners = false;
const watchAndroid = setInterval(() => {
  if (!existsSync(checkpoint) || !existsSync(join(androidRoot, "native", "sdk.log"))) return;
  if (!androidStarted) androidStarted = Date.now();
  const reportBoot = !reportedAndroid && Date.now() - androidStarted >= 85000;
  const reportListeners = reportedAndroid && !reportedListeners && Date.now() - androidStarted >= 114000;
  if (!reportBoot && !reportListeners) return;
  const components = join(home, "components", "android");
  if (!existsSync(components)) return;
  const [identity] = readdirSync(components);
  const adb = join(components, identity!, "sdk", "platform-tools", "adb.exe");
  const adbInfo = (...args: string[]) => Bun.spawnSync([adb, "-P", "15037", ...args], { env, timeout: 4000 }).stdout.toString().trim();
  if (reportBoot) {
    reportedAndroid = true;
    console.log(`Android boot transport: ${adbInfo("devices", "-l")}\n${adbInfo("-s", "127.0.0.1:5565", "shell", "getprop sys.boot_completed; id -u")}`);
  }
  if (reportListeners) {
    reportedListeners = true;
    const shell = (command: string) => adbInfo("-s", "127.0.0.1:5565", "shell", command);
    console.log(`Android first-run user state: ${shell("am get-started-user-state 0; date +%s")}; host clock ${Date.now()}`);
    for (const name of ["notifications", "attention"]) {
      const raw = shell(`cat /data/user/0/dev.aiphone.input/files/${name}.json 2>/dev/null`);
      if (!raw) { console.log(`Android ${name} listener has not written a receipt`); continue; }
      const receipt = JSON.parse(raw);
      console.log(`Android ${name} listener: connected=${receipt.connected}, checked_at_ms=${receipt.checked_at_ms}`);
    }
  }
}, 10000);

async function vmCredentials() {
  const key = await readFile(join(home, "user", "qemu", "control.key"));
  equal(key.length, 32, "isolated VM control key size");
  const socket = createConnection({ host: "127.0.0.1", port: 17861 });
  socket.setTimeout(10_000, () => socket.destroy(Error("QA native VM credential handshake timeout")));
  const chunks = socket[Symbol.asyncIterator]();
  let rest = Buffer.alloc(0);
  async function read() {
    let size = 0;
    for (;;) {
      if (rest.length >= 4) {
        size = rest.readUInt32BE();
        ok(size <= 512, "native VM response bounded frame");
        if (rest.length >= size + 4) break;
      }
      const item = await chunks.next();
      ok(!item.done);
      rest = Buffer.concat([rest, item.value]);
    }
    const reply = fromBinary(LinuxVmControlReplySchema, rest.subarray(4, size + 4));
    rest = rest.subarray(size + 4);
    ok(!reply.error, "native VM returned a control error");
    return reply;
  }
  function write(input: MessageInitShape<typeof LinuxVmControlRequestSchema>) {
    const wire = toBinary(LinuxVmControlRequestSchema, create(LinuxVmControlRequestSchema, input));
    const length = Buffer.alloc(4);
    length.writeUInt32BE(wire.length);
    socket.write(Buffer.concat([length, wire]));
  }
  const sign = (...parts: (string | Uint8Array)[]) => {
    const mac = createHmac("sha256", key);
    for (const item of parts) mac.update(item);
    return mac.digest();
  };
  try {
    const nonce = randomBytes(32);
    write({ payload: nonce });
    const challenge = await read();
    equal(challenge.payload.length, 32);
    ok(timingSafeEqual(challenge.proof, sign("control-challenge", nonce, challenge.payload)), "VM authenticated itself");
    const op = LinuxVmOperation.NATIVE_CREDENTIALS;
    const opBytes = Buffer.alloc(4);
    opBytes.writeInt32BE(op);
    write({ operation: op, proof: sign(opBytes, challenge.payload) });
    const ready = await read();
    ok(timingSafeEqual(ready.proof, sign("control-reply", challenge.payload, ready.payload)), "VM credential reply authenticated");
    return fromBinary(LinuxVmNativeCredentialsSchema, ready.payload);
  } finally {
    socket.destroy();
  }
}

try {
  equal(await gui("fresh"), 0, "advancing the unpackaged native language and device setup");
  const readyDeadline = performance.now() + 600000;
  while (performance.now() < readyDeadline) {
    ok(app.exitCode == null, "installed client must remain running");
    if (existsSync(checkpoint)) {
      const status = JSON.parse(readFileSync(checkpoint, "utf8"));
      if (status.progress?.state === "ready" && status.installed_payload === payload.sha256) break;
      ok(!["failed", "blocked"].includes(status.progress?.state), "native deployment must advance");
    }
    await Bun.sleep(500);
  }
  const status = JSON.parse(readFileSync(checkpoint, "utf8"));
  equal(status.progress.state, "ready");
  equal(status.installed_payload, payload.sha256);
  const frameDeadline = performance.now() + 30000;
  let androidFrame: RegExpMatchArray | null = null;
  let desktopFrame: RegExpMatchArray | null = null;
  do {
    const rendered = readFileSync(log, "utf8");
    androidFrame = rendered.match(/native_metric first_frame device=0 ms=([\d.]+)/);
    desktopFrame = rendered.match(/native_metric first_frame device=1 ms=([\d.]+)/);
    if (androidFrame && desktopFrame) break;
    await Bun.sleep(250);
  } while (performance.now() < frameDeadline);
  ok(androidFrame && desktopFrame, "the owned native window received both device frames");
  equal(await gui("capture"), 0, "installed native workbench window visible");
  console.log(JSON.stringify({ androidFirstFrameMs: Number(androidFrame[1]), desktopFirstFrameMs: Number(desktopFrame[1]), source }));

  const credentials = await vmCredentials();
  const assistantToken = "Bearer " + credentials.assistantToken.trim();
  const phoneToken = "Bearer " + credentials.phoneToken.trim();
  const protocol = "application/x-protobuf";
  const registry = await fetch("http://127.0.0.1:17863/v1/registry/state", { method: "POST",
    headers: { authorization: assistantToken, "content-type": protocol },
    body: toBinary(RegistryRequestSchema, create(RegistryRequestSchema)), signal: AbortSignal.timeout(10000) });
  equal(registry.status, 200, "installed service registry");
  const devices = fromBinary(RegistryStateSchema, new Uint8Array(await registry.arrayBuffer())).devices;
  const desktop = devices.find((device) => device.kind === "desktop" && device.runtimeState === "managed");
  const phone = devices.find((device) => device.kind === "phone" && device.runtimeState === "managed");
  ok(desktop, "installed desktop is registered");
  ok(phone, "installed Android is registered");
  async function verifyFrame(endpoint: string, device: string) {
    const screen = await fetch(`http://127.0.0.1:17865${endpoint}`, { headers: { authorization: phoneToken }, signal: AbortSignal.timeout(12000) });
    equal(screen.status, 200, `${device} media HTTP status`);
    match(screen.headers.get("content-type") ?? "", /^multipart\/x-mixed-replace/i);
    const reader = screen.body!.getReader();
    let frames = Buffer.alloc(0);
    const start = Buffer.from([0xff, 0xd8, 0xff]), end = Buffer.from([0xff, 0xd9]);
    try {
      while (frames.length < 2_000_000) {
        const { value, done } = await reader.read();
        if (done) break;
        frames = Buffer.concat([frames, Buffer.from(value)]);
        const offset = frames.indexOf(start);
        if (offset >= 0 && frames.indexOf(end, offset + 3) > offset) {
          console.log(`${device} sent a complete JPEG through the native relay`);
          return;
        }
      }
    } finally { await reader.cancel(); }
    throw Error(`${device} did not send a complete JPEG`);
  }
  await Promise.all([
    verifyFrame("/desktop/screen", "Linux"),
    verifyFrame("/screen", "Android"),
  ]);
  const scope = { phoneId: phone.id, ownerId: "local-user", controller: "manual" };
  async function phoneRequest(operation: string, body: Uint8Array) {
    const response = await fetch(`http://127.0.0.1:17862/v1/${operation}`, { method: "POST",
      headers: { authorization: phoneToken, "content-type": protocol }, body: new Uint8Array(body), signal: AbortSignal.timeout(16000) });
    equal(response.status, 200, `Android ${operation} HTTP status`);
    return new Uint8Array(await response.arrayBuffer());
  }
  const control = fromBinary(ControlStateSchema, await phoneRequest("control",
    toBinary(TakeControlRequestSchema, create(TakeControlRequestSchema, scope))));
  async function phoneAction(action: MessageInitShape<typeof DeviceActionRequestSchema>["action"]) {
    const actionId = crypto.randomUUID();
    const request = create(DeviceActionRequestSchema, { ...scope, actionId, runId: "manual", controlEpoch: control.controlEpoch, action });
    const result = fromBinary(DeviceActionResponseSchema, await phoneRequest("action", toBinary(DeviceActionRequestSchema, request)));
    equal(result.actionId, actionId);
    equal(result.success, true, `Android ${action?.case} did not execute: ${result.output}`);
    return result;
  }

  const components = join(home, "components", "android");
  const [identity] = readdirSync(components);
  ok(identity, "installed Android capsule");
  const adb = join(components, identity, "sdk", "platform-tools", "adb.exe");
  function android(...args: string[]) {
    const result = Bun.spawnSync([adb, "-P", "15037", "-s", "127.0.0.1:5565", ...args], { env });
    equal(result.exitCode, 0, "installed Android ADB command");
    return result.stdout;
  }
  equal(android("shell", "id", "-u").toString().trim(), "0");
  const frame = android("exec-out", "screencap", "-p");
  equal(frame.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  deepEqual([frame.readUInt32BE(16), frame.readUInt32BE(20)], [720, 1440]);
  const policy = android("shell", "dumpsys", "window", "policy").toString().split("\n")
    .filter(line => /keyguard|showing|interactive/i.test(line)).slice(0, 14).join("\n");
  const beforeFocus = android("shell", "dumpsys", "window", "displays").toString().split("\n")
    .filter(line => /mCurrentFocus|mFocusedApp/.test(line)).join("\n");
  console.log(`Android pristine window state:\n${beforeFocus}\n${policy}`);
  match(android("shell", "locksettings", "set-disabled", "--user", "0", "true").toString(), /Lock screen disabled set to true/);
  equal(android("shell", "locksettings", "get-disabled", "--user", "0").toString().trim(), "true");
  android("shell", "input", "keyevent", "KEYCODE_SLEEP");
  await Bun.sleep(350);
  android("shell", "input keyevent KEYCODE_WAKEUP; svc power stayon true; cmd statusbar collapse; wm dismiss-keyguard; input keyevent KEYCODE_HOME");
  const search = "com.android.settings.intelligence";
  const opened = android("shell", "am", "start", "-W", "-a", "android.settings.APP_SEARCH_SETTINGS", "-p", search).toString();
  match(opened, /Status: ok\b/);
  equal(android("shell", "settings", "get", "secure", "default_input_method").toString().trim(), "dev.aiphone.input/.UnicodeIme");
  const uiFile = "/sdcard/dysprosium-install.xml";
  async function androidEditor(predicate: (xml: string) => boolean) {
    const deadline = Date.now() + 15000;
    let last = "";
    do {
      last = android("shell", `rm -f ${uiFile}; timeout 7 uiautomator dump ${uiFile} 2>&1; [ ! -s ${uiFile} ] || cat ${uiFile}`).toString();
      if (predicate(last)) return last;
      await Bun.sleep(250);
    } while (Date.now() < deadline);
    const focus = android("shell", "dumpsys", "window", "displays").toString().split("\n")
      .filter(line => /mCurrentFocus|mFocusedApp/.test(line)).join("\n");
    throw Error(`Android Settings editor not found: ${focus}\n${last.slice(0, 450)}`);
  }
  const editable = /<node[^>]*resource-id="android:id\/search_src_text"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/;
  const clear = /<node[^>]*resource-id="android:id\/search_close_btn"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/;
  async function tap(field: RegExpMatchArray | null) {
    ok(field, "visible Android control coordinates");
    await phoneAction({case: "tap", value: { x: (Number(field[1]) + Number(field[3])) >> 1,
      y: (Number(field[2]) + Number(field[4])) >> 1 }});
  }
  const hierarchy = await androidEditor(xml => editable.test(xml));
  await tap(hierarchy.match(editable));
  await androidEditor(xml => editable.test(xml) && editable.exec(xml)?.[0].includes('focused="true"') === true);
  const sample = "Dysprosium 中文";
  const input = await phoneAction({case: "input", value: {text: sample}});
  ok(input.output.includes(`committed:${input.actionId}`), "installed Unicode IME returned the action receipt");
  const entered = await androidEditor(xml => editable.exec(xml)?.[0].includes(`text="${sample}"`) === true && clear.test(xml));
  await tap(entered.match(clear));
  await androidEditor(xml => editable.test(xml) && !editable.exec(xml)?.[0].includes(sample) && !clear.test(xml));
  console.log(JSON.stringify({ androidRoot: true, androidFrame: [720, 1440], androidWorkbenchTouch: true, androidWorkbenchChinese: true, linuxJpeg: true }));

  if (process.env.AIMANITY_QA_ANDROID_AUDIO === "1") {
    const music = android("shell", "pm", "list", "packages", "--user", "0", "com.android.music").toString().trim();
    if (music !== "package:com.android.music") {
      match(android("shell", "cmd", "package", "install-existing", "--user", "0", "com.android.music").toString(), /installed for user: 0/);
    }
    const audio = await fetch(`http://127.0.0.1:17865/audio`, {
      headers: { authorization: phoneToken }, signal: AbortSignal.timeout(16000),
    });
    equal(audio.status, 200, "installed Android audio");
    match(audio.headers.get("content-type") ?? "", /^audio\/L16;rate=48000;channels=2$/i);
    const pcm = audio.body!.getReader();
    let min = 0, max = 0, samples = 0;
    let partial: Buffer<ArrayBufferLike> = Buffer.alloc(0);
    try {
      const playback = android("shell", "am", "start", "-W", "-a", "android.intent.action.VIEW", "-d",
        "file:///product/media/audio/alarms/Neon.ogg", "-t", "audio/ogg", "-n", "com.android.music/.AudioPreview");
      match(playback.toString(), /Status: ok\b/);
      while (min === 0 || max === 0) {
        const { value, done } = await pcm.read();
        if (done) break;
        const bytes = Buffer.concat([partial, Buffer.from(value)]);
        const complete = bytes.length & ~1;
        for (let index = 0; index < complete; index += 2) {
          const sample = bytes.readInt16LE(index);
          min = Math.min(min, sample);
          max = Math.max(max, sample);
          samples++;
        }
        partial = bytes.subarray(complete);
      }
    } finally {
      await pcm.cancel();
      android("shell", "am", "force-stop", "com.android.music");
      if (music !== "package:com.android.music") android("shell", "pm", "uninstall", "-k", "--user", "0", "com.android.music");
    }
    ok(min < 0 && max > 0, "installed Android audio emitted nonzero stereo PCM");
    console.log(JSON.stringify({ androidAudio: true, rate: 48000, channels: 2, samples, min, max }));
  }
  console.log(JSON.stringify({ nativeBootAndTransportMs: Math.round(performance.now() - started), source }));
  await probe("devices");
  const model = Bun.spawn([process.execPath, "../qa/windows-native-dev/qualified-model.ts"],
    { env, stdout:"inherit", stderr:"inherit" });
  try {
    const deadline = performance.now() + 12000;
    for (;;) {
      try {
        const health = await fetch("http://127.0.0.1:17998/health", {signal:AbortSignal.timeout(1000)});
        if (health.ok) break;
      } catch {}
      if (performance.now() >= deadline || model.exitCode != null) throw Error("isolated model did not open");
      await Bun.sleep(200);
    }
    await probe("model-configure");
    await probe("model-action", "phone");
    await probe("model-action", "desktop");
    await probe("liberty");
    await probe("goal-begin");
    await stopApp();
    const restartLog = join(root, "native-restart.log");
    app = launch(restartLog);
    const restartDeadline = performance.now() + 600000;
    let twoFrames = false;
    while (performance.now() < restartDeadline) {
      ok(app.exitCode == null, "same native client running after restart");
      const lines = existsSync(restartLog) ? readFileSync(restartLog, "utf8") : "";
      twoFrames = /native_metric first_frame device=0/.test(lines) && /native_metric first_frame device=1/.test(lines);
      if (twoFrames) break;
      await Bun.sleep(500);
    }
    ok(twoFrames, "same independent device disks both gave frames again");
    await probe("goal-resumed");
  } finally {
    model.kill();
    await model.exited;
  }
} catch (error) {
  for (const log of logPaths) {
    const file = join(home, log);
    if (existsSync(file)) {
      const output = readFileSync(file, "utf8");
      const milestones = output.split("\n").filter(line => /adbd|boot_completed|keyguard|SurfaceFlinger|panic|WHPX|SystemServer|FATAL EXCEPTION/i.test(line)).slice(-32);
      console.error(log, milestones.join("\n"), output.slice(-3600));
    }
  }
  throw error;
} finally {
  clearInterval(watchAndroid);
  await stopApp();
}
