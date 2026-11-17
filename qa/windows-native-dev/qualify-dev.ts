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
equal(manifest.source_commit, process.env.QA_GUEST_SOURCE ?? source);
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
function gui(action: "fresh" | "capture" | "capture-updated" | "diagnostic" | "keyboard" | "close") {
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
const failures: string[] = [];
async function verify(name: string, test: () => Promise<void>) {
  try {
    await test();
    return true;
  } catch (error) {
    const detail = `${name}: ${error instanceof Error ? error.message : String(error)}`;
    failures.push(detail);
    console.error(detail);
    return false;
  }
}
const logPaths = ["user/qemu/owner.log", "user/qemu/boot.log", "user/qemu/qemu.log",
  "user/data/android/native/sdk.log", "user/data/android/whpx-avd/cuttlefish/kernel.log", "request.json", "deployment.json"];
let observe = true;
let observation: Promise<void> | undefined;

async function inspectAdb(command: string) {
  const socket = createConnection({host:"127.0.0.1", port:15037});
  socket.setTimeout(3500, () => socket.destroy(Error("ADB shell read timed out")));
  const stream = socket[Symbol.asyncIterator]();
  let buffer: Buffer<ArrayBufferLike> = Buffer.alloc(0);
  async function read(size: number) {
    while (buffer.length < size) {
      const part = await stream.next();
      if (part.done) throw Error("ADB shell stream closed");
      buffer = Buffer.concat([buffer, part.value]);
    }
    const value = buffer.subarray(0, size);
    buffer = buffer.subarray(size);
    return value;
  }
  async function send(request: string) {
    socket.write(Buffer.byteLength(request).toString(16).padStart(4,"0") + request);
    const reply = (await read(4)).toString();
    if (reply === "FAIL") {
      throw Error((await read(Number.parseInt((await read(4)).toString(),16))).toString());
    }
    equal(reply,"OKAY","ADB status frame");
  }
  try {
    await send("host:transport:127.0.0.1:5565");
    await send("shell,v2,raw:" + command);
    let stdout = "", stderr = "";
    for (;;) {
      const header = await read(5), size = header.readUInt32LE(1);
      ok(size <= 65536, "bounded Android diagnostic frame");
      const data = await read(size);
      if (header[0] === 1) stdout += data.toString();
      if (header[0] === 2) stderr += data.toString();
      if (header[0] === 3) return {stdout:stdout.trim(),stderr:stderr.trim(),code:data[0]};
    }
  } finally { socket.destroy(); }
}
async function sampleAndroidInput() {
  const state = [
    "printf 'boot='; getprop sys.boot_completed; printf 'uid='; id -u",
    "printf 'user0='; am get-started-user-state 0",
    "printf 'selector='; settings --user 0 get secure enabled_accessibility_services",
    "printf 'flag='; settings --user 0 get secure accessibility_enabled",
    "printf 'package='; cmd package query-services --user 0 --components -a android.accessibilityservice.AccessibilityService -p dev.aiphone.input",
    "dumpsys -t 2 accessibility | grep -Ei 'enabled services|binding services|bound services|PhoneAttention'",
    "dumpsys -t 2 activity services dev.aiphone.input/.PhoneAttention | grep -E 'ServiceRecord|PhoneAttention|No services|requested='",
  ].join("; ");
  let previous = "";
  while (observe) {
    const sample = await inspectAdb(state).then(value => value, (error: Error) => ({ unavailable:error.message }));
    const current = JSON.stringify(sample);
    if (current !== previous) {
      console.log(JSON.stringify({ androidAccessibilityObservationMs: Math.round(performance.now() - started), ...sample }));
      previous = current;
    }
    await Bun.sleep(900);
  }
}

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

async function qualify() {
try {
  equal(await gui("fresh"), 0, "advancing the unpackaged native language and device setup");
  const request = join(home, "request.json");
  const requestDeadline = performance.now() + 7500;
  while (!existsSync(request) && performance.now() < requestDeadline) await Bun.sleep(250);
  if (!existsSync(request)) {
    equal(await gui("keyboard"), 0, "observe the installed window after keyboard input");
    console.log(`Setup request after Tab, Enter and Space: ${existsSync(request)}`);
  }
  ok(existsSync(request), "Start installation must persist the native device setup request");
  console.log(`Native device setup request persisted after the Win32 installation click: ${Math.round(performance.now()-started)} ms`);
  observation = sampleAndroidInput();
  const readyDeadline = performance.now() + 600000;
  let previousPhase = "";
  while (performance.now() < readyDeadline) {
    ok(app.exitCode == null, "installed client must remain running");
    const lines = existsSync(log) ? readFileSync(log, "utf8").split("\n") : [];
    const phase = lines.findLast(line => line.startsWith("native_metric startup_phase="));
    if (phase && phase !== previousPhase) {
      previousPhase = phase;
      console.log(phase);
    }
    const failure = lines.find(line => line.startsWith("AI_PHONE_QEMU_CONTROL_ERROR "));
    if (failure) throw Error(failure);
    if (existsSync(checkpoint)) {
      const status = JSON.parse(readFileSync(checkpoint, "utf8"));
      if (status.progress?.state === "ready" && status.installed_payload === payload.sha256) break;
      ok(!["failed", "blocked"].includes(status.progress?.state), "native deployment must advance");
    }
    await Bun.sleep(500);
  }
  observe = false;
  await observation;
  ok(existsSync(checkpoint), "Windows workbench did not finish initialization within the existing first-run deadline");
  const status = JSON.parse(readFileSync(checkpoint, "utf8"));
  equal(status.progress.state, "ready");
  equal(status.installed_payload, payload.sha256);
  if (process.env.AIMANITY_QA_ONLY_LINUX_INPUT === "1") {
    await probe("desktop-cjk-audio");
    return;
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
  await verify("WHPX Android root and deployed accessibility registry JAR", async () => {
    equal(android("shell", "id", "-u").toString().trim(), "0");
    const registered = android("shell", "CLASSPATH=/data/local/tmp/ai-phone-input-notification-probe.jar app_process / dev.aiphone.input.probe.NotificationAccess attention-installed").toString().trim();
    equal(registered, "true", "the exact deployed JAR sees PhoneAttention in AccessibilityManager");
    const binding = android("shell", "dumpsys -t 2 activity services dev.aiphone.input/.PhoneAttention").toString();
    match(binding, /^\s*requested=true received=true hasBound=true /m, "PhoneAttention registered and bound inside WHPX");
    console.log(JSON.stringify({ accessibilityProbeRegistered: registered, accessibilityServiceBound: true, source }));
  });
  await verify("Foreground frames from both devices", async () => {
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
    ok(androidFrame && desktopFrame, "the native app decoded complete frames from both actual devices");
    equal(await gui("capture"), 0, "installed native workbench window visible");
    const rendered = readFileSync(log, "utf8");
    const upload = (index: number) => {
      const match = rendered.match(new RegExp(`native_metric frame device=${index} sequence=\\d+ receive_to_upload_ms=([\\d.]+)`));
      return match ? Number(match[1]) : null;
    };
    const frame = android("exec-out", "screencap", "-p");
    equal(frame.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    deepEqual([frame.readUInt32BE(16), frame.readUInt32BE(20)], [720, 1440]);
    console.log(JSON.stringify({ androidFirstFrameMs: Number(androidFrame[1]), desktopFirstFrameMs: Number(desktopFrame[1]),
      androidReceiveToUploadMs: upload(0), desktopReceiveToUploadMs: upload(1), independentAndroidPng:[720,1440], source }));
  });

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

  await verify("Android touch and Chinese input", async () => {
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
    equal(await gui("capture-updated"), 0, "live native phone display after entry of Chinese text");
    await tap(entered.match(clear));
    await androidEditor(xml => editable.test(xml) && !editable.exec(xml)?.[0].includes(sample) && !clear.test(xml));
    console.log(JSON.stringify({ androidWorkbenchTouch: true, androidWorkbenchChinese: true }));
  });

  if (process.env.AIMANITY_QA_ANDROID_AUDIO === "1") await verify("Android PCM at the Windows relay", async () => {
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
  });
  console.log(JSON.stringify({ nativeBootAndTransportMs: Math.round(performance.now() - started), source }));
  await verify("ACL, manual device receipts and Linux observation", () => probe("devices"));
  await verify("Linux XTerm CJK and PCM at the Windows relay", () => probe("desktop-cjk-audio"));
  const manualControlReleasedAt = performance.now() + 15_500;
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
    await verify("First real phone Agent action", () => probe("model-action", "phone"));
    await Bun.sleep(Math.max(0, manualControlReleasedAt - performance.now()));
    await verify("First real desktop Agent action", () => probe("model-action", "desktop"));
    await verify("Liberty real device receipts", () => probe("liberty"));
    const waiting = await verify("Task & Goal checkpoint on a real phone", () => probe("goal-begin"));
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
    if (!twoFrames) failures.push("both independent device frames must recover after native relaunch");
    if (waiting) await verify("Same Task & Goal resumes without repeating the action", () => probe("goal-resumed"));
  } finally {
    model.kill();
    await model.exited;
  }
  ok(!failures.length, failures.join("\n"));
} catch (error) {
  if (!existsSync(checkpoint) && app.exitCode == null) await gui("diagnostic");
  if (existsSync(log)) console.error("Native client startup log:", readFileSync(log,"utf8").slice(-6000));
  for (const folder of [home, join(home,"components"), join(home,"components","android"), join(home,"user")]) {
    if (existsSync(folder)) console.error(`Owned startup directory ${folder}:`, readdirSync(folder));
  }
  for (const log of logPaths) {
    const file = join(home, log);
    if (existsSync(file)) {
      const output = readFileSync(file, "utf8");
      const milestones = output.split("\n").filter(line => /Android QEMU screen:|adbd|boot_completed|keyguard|SurfaceFlinger|panic|WHPX|SystemServer|FATAL EXCEPTION/i.test(line)).slice(-32);
      console.error(log, milestones.join("\n"), output.slice(-3600));
    }
  }
  throw error;
} finally {
  observe = false;
  await observation;
  await stopApp();
}
}
await qualify();
