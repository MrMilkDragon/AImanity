import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { equal, ok } from "node:assert/strict";
import { createHmac, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import { readFile, realpath } from "node:fs/promises";
import { createConnection } from "node:net";
import { isAbsolute, join, relative, sep } from "node:path";
import { ChatHistoryRequestSchema, ChatRunRequestSchema, ChatSendRequestSchema, ChatStateSchema } from "../packages/protocol/src/gen/ai_phone/v1/assistant_pb";
import { LinuxVmControlReplySchema, LinuxVmControlRequestSchema, LinuxVmNativeCredentialsSchema, LinuxVmOperation } from "../packages/protocol/src/gen/ai_phone/v1/host_bridge_pb";
import { AgentConfigurationSaveRequestSchema, ConnectionTestRequestSchema, RegistryRequestSchema, RegistryStateSchema } from "../packages/protocol/src/gen/ai_phone/v1/registry_pb";
import { DesktopRequestSchema, DesktopObservationSchema, DesktopActionRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/desktop_pb";
import { ControlStateSchema, DeviceActionRequestSchema, DeviceActionResponseSchema, DeviceObservationSchema, DeviceRequestSchema, TakeControlRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/phone_runtime_pb";
import { GoalUpdateRequestSchema, ProactiveRequestSchema, ProactiveStateSchema, ProactiveUpdateRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/proactive_pb";
import { FileListingSchema, FileLocationSchema, HarnessEventListSchema, HarnessEventRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/workbench_pb";
import { ModelTestResultSchema } from "../packages/protocol/src/gen/ai_phone/v1/settings_pb";
import type { DescMessage, MessageInitShape } from "@bufbuild/protobuf";

const root = await realpath(process.env.AIMANITY_QA_ROOT ?? "");
const home = await realpath(process.env.AIMANITY_QA_HOST ?? "");
const part = relative(root, home);
ok(/^(Dysprosium-QA-|aimanity-dysprosium-qa-)/i.test(root.split(sep).at(-1)!), "Use only a disposable Dysprosium QA directory");
ok(part && !isAbsolute(part) && !part.startsWith(".."), "The host folder must be contained in the disposable root");
const command = process.argv[2] ?? "devices";
ok(["devices", "state", "run", "snapshot", "retention", "model-configure", "model-action", "liberty", "goal-begin", "goal-resumed"].includes(command), "Unknown probe command");

const checkpoint = JSON.parse(await readFile(join(home, "deployment.json"), "utf8"));
if (process.env.AIMANITY_QA_SOURCE_PAYLOAD)
  equal(checkpoint.installed_payload, process.env.AIMANITY_QA_SOURCE_PAYLOAD, "exact staged source payload");
equal(checkpoint.progress.state, "ready");

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

const credentials = process.platform === "win32" ? await vmCredentials() : {
  assistantToken: await readFile(join(home, "releases", checkpoint.installed_payload, "data", "assistant", "token"), "utf8"),
  phoneToken: await readFile(join(home, "releases", checkpoint.installed_payload, "data", "phone-runner", "token"), "utf8"),
};
const assistant = "Bearer " + credentials.assistantToken.trim();
const phoneToken = "Bearer " + credentials.phoneToken.trim();
async function rpc<I extends DescMessage, O extends DescMessage>(path: string, req: I, args: MessageInitShape<I>, res: O, device = false) {
  const port = device ? 17862 : 17863;
  const response = await fetch("http://127.0.0.1:" + port + "/v1/" + path, {
    method: "POST", headers: { "content-type": "application/x-protobuf", authorization: device ? phoneToken : assistant },
    body: toBinary(req, create(req, args)), signal: AbortSignal.timeout(15000),
  });
  ok(response.ok, path + " returned status " + response.status);
  return fromBinary(res, new Uint8Array(await response.arrayBuffer()));
}
const registry = await rpc("registry/state", RegistryRequestSchema, {}, RegistryStateSchema);
const phone = registry.devices.find(device => device.kind === "phone" && device.runtimeState === "managed");
const desktop = registry.devices.find(device => device.kind === "desktop" && device.runtimeState === "managed");
ok(phone && desktop, "both real devices are managed and ready");


const modelName = "Dysprosium 0.0.4 isolated QA";
const proactive = () => rpc("proactive/state", ProactiveRequestSchema, {}, ProactiveStateSchema);
const durableRun = (runId: string) => rpc("run", ChatRunRequestSchema, {runId}, ChatStateSchema);
const runEvents = async (runId: string) =>
  (await rpc("workbench/events", HarnessEventRequestSchema, {runId}, HarnessEventListSchema)).events;
async function until<T>(condition: () => Promise<T | undefined>, description: string, limit = 120000) {
  const deadline = performance.now() + limit;
  do {
    const result = await condition();
    if (result !== undefined) return result;
    await Bun.sleep(150);
  } while (performance.now() < deadline);
  throw Error(description);
}
if (command === "model-configure") {
  if (registry.agents.some(agent => agent.name === modelName)) process.exit(0);
  ok(!registry.agents.length && !registry.activeAgentId, "existing models are outside QA");
  const agent = randomUUID(), model = randomUUID();
  const draft = { provider:"deepseek", protocol:"chat-completions", model:"qa-dysprosium-local",
    baseUrl: process.env.AIMANITY_QA_MODEL_URL ?? "http://127.0.0.1:17998/v1" };
  const tested = await rpc("registry/model-test", ConnectionTestRequestSchema, {id:model,draft}, ModelTestResultSchema);
  ok(tested.ready, "controlled local endpoint passed the actual model settings check");
  const next = await rpc("registry/agent-config-save", AgentConfigurationSaveRequestSchema, {
    agent:{id:agent,name:modelName,modelConnectionId:model},
    connection:{id:model,name:modelName,draft,verificationId:tested.verificationId},
  }, RegistryStateSchema);
  equal(next.activeAgentId, agent);
  console.log(JSON.stringify({ controlledProvider: true, modelConfigured: true, agentId: agent, modelId: model }));
  process.exit(0);
}

if (["model-action", "liberty", "goal-begin", "goal-resumed"].includes(command)) {
  const active = registry.agents.find(item => item.id === registry.activeAgentId);
  ok(active?.name === modelName, "isolated controlled model only");
  const now = Date.now();
  const target = process.argv[3] === "desktop" ? desktop : phone;
  const mode = command === "liberty" ? "liberty" : command.startsWith("goal-") ? "goal" : target.kind === "desktop" ? "desktop" : "phone";
  await Bun.write(join(root, "qualified-model-mode.json"), JSON.stringify({scenario:mode, desktop:desktop.id}));
  if (command === "model-action") {
    const actionTitle = target.kind === "desktop" ? "电脑按键" : "手机按键";
    const input = target.kind === "desktop" ? "使用真实电脑桌面，按一下 Escape 键，然后确认操作。"
      : "使用真实安卓手机，按一次 HOME 键回到主屏幕，并确认操作。";
    const receipt = await rpc("send", ChatSendRequestSchema, {deviceId:target.id,text:input,requestId:randomUUID(),receiptOnly:true}, ChatStateSchema);
    let firstTextMs: number | null = null;
    const final = await until(async () => {
      const state = await durableRun(receipt.runId);
      if (firstTextMs === null && state.messages.some(message => message.role === "assistant" && message.text))
        firstTextMs = Date.now() - now;
      return ["queued","running"].includes(state.runStatus) ? undefined : state;
    }, "controlled run didn't settle");
    equal(final.runStatus, "succeeded", "real tool run must complete");
    const events = await runEvents(receipt.runId);
    const executed = events.find(event => event.title === actionTitle && event.kind === "tool" && event.status === "completed" && event.finishedAtMs > 0n);
    ok(executed, "real device tool completion receipt");
    console.log(JSON.stringify({
      controlledProvider:true, device:target.kind, firstTextObservedMs:firstTextMs,
      firstConfirmedDeviceActionMs:Number(executed.finishedAtMs) - now,
      totalObservedMs:Date.now() - now, modelCalls:events.filter(event=>event.modelTiming).length,
    }));
  } else if (command === "liberty") {
    const before = await proactive();
    ok(!before.goalActive && !before.exploring, "disposable inactive mode");
    const setting = { notificationsEnabled:before.notificationsEnabled,forwardNotifications:before.forwardNotifications,autonomousReply:before.autonomousReply,autonomousInstall:before.autonomousInstall,intervalSeconds:before.intervalSeconds,wechatWatch:before.wechatWatch };
    const toggled = await rpc("proactive/update", ProactiveUpdateRequestSchema, {...setting, expectedRevision:before.revision, exploring:true}, ProactiveStateSchema);
    ok(toggled.exploring);
    try {
      const runId = await until(async () => (await proactive()).activeRunId || undefined, "Liberty did not dispatch a run", 45000);
      const receipt = await until(async () => (await runEvents(runId))
        .find(e => e.kind === "tool" && e.status === "completed" && ["观察手机","观察电脑","手机按键","电脑按键"].includes(e.title)), "Liberty never obtained a real device receipt");
      console.log(JSON.stringify({ controlledProvider:true, liberty:true, runId, receipt:receipt.title, firstConfirmedReceiptMs:Number(receipt.finishedAtMs)-now }));
    } finally {
      const current = await proactive();
      await rpc("proactive/update", ProactiveUpdateRequestSchema, {...setting, expectedRevision:current.revision, exploring:false}, ProactiveStateSchema);
    }
  } else if (command === "goal-begin") {
    const before = await proactive();
    ok(!before.goalActive && !before.exploring);
    const admitted = await rpc("proactive/goal", GoalUpdateRequestSchema, {
      expectedRevision:before.revision, deviceId:phone.id,
      goal:"通过真实手机按下一次 HOME 键；需要短暂等待，随后再次观察并完成。此任务用于验证退出后继续。",
    }, ProactiveStateSchema);
    const admittedRun = admitted.activeRunId || await until(async () => (await proactive()).activeRunId || undefined, "Task & Goal run must be admitted");
    const checkpoint = await until(async () => {
      const state = await durableRun(admittedRun);
      if (state.runStatus !== "queued" || state.nextWakeAtMs <= BigInt(Date.now())) return;
      const events = await runEvents(admittedRun);
      const keys = events.filter(e=>e.title==="手机按键" && e.kind==="tool" && e.status==="completed");
      if (keys.length === 1) return { runId:admittedRun, conversation:state.conversationId, actionId:keys[0].id, nextWakeAtMs:Number(state.nextWakeAtMs) };
    }, "No persisted task wait after the real Android action");
    await Bun.write(join(root,"goal-checkpoint.json"), JSON.stringify(checkpoint));
    console.log(JSON.stringify({controlledProvider:true, waiting:true, ...checkpoint}));
  } else {
    const checkpoint = await Bun.file(join(root,"goal-checkpoint.json")).json();
    const resumed = await until(async () => {
      const state = await durableRun(checkpoint.runId);
      return !["queued","running","paused"].includes(state.runStatus) ? state : undefined;
    }, "same checkpoint run did not resume after host relaunch");
    equal(resumed.runStatus,"succeeded");
    equal(resumed.conversationId,checkpoint.conversation);
    const events = await runEvents(checkpoint.runId);
    equal(events.filter(e=>e.title==="手机按键" && e.kind==="tool" && e.status==="completed").map(e=>e.id).join(""), checkpoint.actionId, "desktop shutdown must not replay the completed Android action");
    ok(events.some(e=>e.title.startsWith("观察") && e.kind==="tool" && e.status==="completed" && Number(e.finishedAtMs)>=checkpoint.nextWakeAtMs), "new real observation after resume");
    console.log(JSON.stringify({controlledProvider:true, resumed:true, noActionReplay:true, runId:checkpoint.runId}));
  }
  process.exit(0);
}

if (command === "retention") {
  const marker = process.argv[3];
  ok(marker && /^update-ownership-[0-9a-f-]+\.txt$/i.test(marker), "Only the staged QA marker can be read");
  const lists = await Promise.all([
    rpc("files/list", FileLocationSchema, {path:"/"}, FileListingSchema),
    rpc("files/list", FileLocationSchema, {deviceId:desktop.id,path:"/home/ai/Files"}, FileListingSchema),
    rpc("files/list", FileLocationSchema, {deviceId:phone.id,path:"/sdcard/Download"}, FileListingSchema),
  ]);
  for (const list of lists) {
    ok(list.entries.some(entry => entry.name === marker && entry.sizeBytes === BigInt(marker.length)), "pre-upgrade workspace or device marker survives");
  }
  console.log(JSON.stringify({workspace:true, linux:true, android:true}));
  process.exit(0);
}

if (command === "state" || command === "snapshot") {
  const [state, history] = await Promise.all([
    rpc("proactive/state", ProactiveRequestSchema, {}, ProactiveStateSchema),
    rpc("history", ChatHistoryRequestSchema, { limit: 80 }, ChatStateSchema),
  ]);
  console.log(JSON.stringify({
    agentId: registry.activeAgentId,
    modelIds: registry.models.map(item => item.id).sort(),
    phoneId: phone.id,
    desktopId: desktop.id,
    conversationId: history.conversationId,
    messageCount: history.messages.length,
    activeRunId: state.activeRunId,
    runStatus: history.runStatus,
    exploring: state.exploring,
    goalStatus: state.goalStatus,
  }));
  process.exit(0);
}

if (command === "run") {
  const runId = process.argv[3];
  ok(runId && /^[0-9a-f-]{36}$/i.test(runId), "Supply the isolated run ID");
  const events = await rpc("workbench/events", HarnessEventRequestSchema, { runId }, HarnessEventListSchema);
  console.log(JSON.stringify({ runId, events: events.events.map(event => ({
    kind: event.kind, title: event.title, status: event.status,
    createdAtMs: Number(event.createdAtMs), finishedAtMs: Number(event.finishedAtMs),
  })) }));
  process.exit(0);
}

async function firstFrame(route: string) {
  const start = performance.now();
  const response = await fetch("http://127.0.0.1:17865/" + route, {
    headers: { authorization: phoneToken }, signal: AbortSignal.timeout(15000),
  });
  ok(response.ok);
  ok(response.headers.get("content-type")?.startsWith("multipart/x-mixed-replace"));
  const reader = response.body!.getReader();
  let bytes = Buffer.alloc(0);
  try {
    while (bytes.length < 2_500_000) {
      const item = await reader.read();
      if (item.done) break;
      bytes = Buffer.concat([bytes, Buffer.from(item.value)]);
      const header = bytes.indexOf("\r\n\r\n");
      if (header < 0) continue;
      const count = Number(/(?:^|\r\n)Content-Length: (\d+)/i.exec(bytes.toString("ascii", 0, header))?.[1]);
      if (!(count > 4 && count < 2_500_000)) throw Error("Invalid native JPEG frame length");
      const data = bytes.subarray(header + 4, header + 4 + count);
      if (data.length !== count) continue;
      equal(data.readUInt16BE(), 0xffd8);
      equal(data.readUInt16BE(data.length - 2), 0xffd9);
      return Number((performance.now() - start).toFixed(1));
    }
  } finally { await reader.cancel(); }
  throw Error("No complete JPEG frame on " + route);
}
async function wsStatus(headers: string[]) {
  const socket = createConnection({ host: "127.0.0.1", port: 17863 });
  socket.setTimeout(5000, () => socket.destroy(Error("QA WebSocket handshake timeout")));
  socket.write([
    "GET /v1/chat/events HTTP/1.1", "Host: 127.0.0.1:17863", "Connection: Upgrade",
    "Upgrade: websocket", "Sec-WebSocket-Version: 13", "Sec-WebSocket-Key: " + randomBytes(16).toString("base64"),
    ...headers, "", "",
  ].join("\r\n"));
  try {
    let result = "";
    for await (const chunk of socket) {
      result += Buffer.from(chunk).toString("ascii");
      if (result.includes("\r\n\r\n")) return Number(result.split(" ", 3)[1]);
    }
  } finally { socket.destroy(); }
  throw Error("Missing WebSocket handshake response");
}
const [androidFrameMs, linuxFrameMs, anonymous, badOrigin, authenticated] = await Promise.all([
  firstFrame("screen"), firstFrame("desktop/screen"), wsStatus([]),
  wsStatus(["Authorization: " + assistant, "Origin: https://qa.invalid"]),
  wsStatus(["Authorization: " + assistant]),
]);
ok(anonymous === 401 || anonymous === 403, "missing service credential rejected");
ok(badOrigin === 401 || badOrigin === 403, "browser origin cannot use service-only authentication");
equal(authenticated, 101, "authorized native WS connected without browser origin");

const android = await rpc("observe", DeviceRequestSchema, { phoneId: phone.id, ownerId: "local-user", metadataOnly: true }, DeviceObservationSchema, true);
equal(android.rootIdentity, "0", "Android process root identity");
const phoneStarted = performance.now();
const control = await rpc("control", TakeControlRequestSchema, { phoneId: phone.id, ownerId: "local-user", controller: "manual" }, ControlStateSchema, true);
const actionId = randomUUID();
const receipt = await rpc("action", DeviceActionRequestSchema, {
  phoneId: phone.id, ownerId: "local-user", controller: "manual", runId: "manual", controlEpoch: control.controlEpoch,
  actionId, action: { case: "key", value: { key: "HOME" } },
}, DeviceActionResponseSchema, true);
ok(receipt.success && !receipt.resultUnknown && !receipt.replayed);
equal(receipt.actionId, actionId);
const androidActionMs = Number((performance.now() - phoneStarted).toFixed(1));
const desktopStarted = performance.now();
const window = await rpc("desktop/control", DesktopRequestSchema, { deviceId: desktop.id }, DesktopObservationSchema);
const painted = await rpc("desktop/action", DesktopActionRequestSchema, {
  deviceId: desktop.id, actionId: randomUUID(), observationId: window.observationId,
  epoch: window.epoch, kind: "key", key: "Escape", waitForPaint: true,
}, DesktopObservationSchema);
equal(painted.deviceId, desktop.id);
ok(painted.width > 0 && painted.height > 0);
const linuxActionMs = Number((performance.now() - desktopStarted).toFixed(1));
console.log(JSON.stringify({
  androidFrameMs, linuxFrameMs, androidActionMs, linuxActionMs, androidRoot: true,
  acl: { anonymous, badOrigin, authenticated }, semantics: "authenticated runtime frame and manual device action receipts; not initial GUI frame or model first action",
}));
