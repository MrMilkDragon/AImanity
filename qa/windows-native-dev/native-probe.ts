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
import { FileListingSchema, FileLocationSchema, FileManageRequestSchema, FileManageResultSchema, FileOperation, HarnessEventListSchema, HarnessEventRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/workbench_pb";
import { ApiErrorSchema } from "../packages/protocol/src/gen/ai_phone/v1/system_pb";
import { ModelTestResultSchema } from "../packages/protocol/src/gen/ai_phone/v1/settings_pb";
import type { DescMessage, MessageInitShape } from "@bufbuild/protobuf";

const root = await realpath(process.env.AIMANITY_QA_ROOT ?? "");
const home = await realpath(process.env.AIMANITY_QA_HOST ?? "");
const part = relative(root, home);
ok(/^(Dysprosium-QA-|aimanity-dysprosium-qa-)/i.test(root.split(sep).at(-1)!), "Use only a disposable Dysprosium QA directory");
ok(part && !isAbsolute(part) && !part.startsWith(".."), "The host folder must be contained in the disposable root");
const command = process.argv[2] ?? "devices";
ok(["devices", "acl", "desktop-cjk-audio", "legacy-disk", "quiet", "state", "run", "cancel-run", "snapshot", "retention", "model-configure", "model-action", "liberty", "goal-begin", "goal-resumed"].includes(command), "Unknown probe command");

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

const credentials = await vmCredentials();
const assistant = "Bearer " + credentials.assistantToken.trim();
const phoneToken = "Bearer " + credentials.phoneToken.trim();
async function rpc<I extends DescMessage, O extends DescMessage>(path: string, req: I, args: MessageInitShape<I>, res: O, device = false) {
  const port = device ? 17862 : 17863;
  const response = await fetch("http://127.0.0.1:" + port + "/v1/" + path, {
    method: "POST", headers: { "content-type": "application/x-protobuf", authorization: device ? phoneToken : assistant },
    body: toBinary(req, create(req, args)), signal: AbortSignal.timeout(15000),
  });
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (!response.ok) {
    const reason = fromBinary(ApiErrorSchema, bytes);
    throw Error(`${path} returned HTTP ${response.status} (${reason.code}): ${reason.message}`);
  }
  return fromBinary(res, bytes);
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
if (command === "quiet") {
  const current = await proactive();
  if (current.exploring) await rpc("proactive/update", ProactiveUpdateRequestSchema, {...current, expectedRevision:current.revision, exploring:false}, ProactiveStateSchema);
  console.log(JSON.stringify({exploring:(await proactive()).exploring}));
  process.exit(0);
}
if (command === "cancel-run") {
  const runId = process.argv[3];
  ok(runId && /^[0-9a-f-]{36}$/i.test(runId), "Supply the exact QA run ID");
  const agent = registry.agents.find(agent => agent.id === registry.activeAgentId);
  ok(agent?.name === modelName, "The active agent must be the isolated model");
  const prior = await durableRun(runId);
  equal(prior.runId, runId);
  equal(prior.agentId, agent.id);
  const result = ["running", "queued", "paused"].includes(prior.runStatus)
    ? await rpc("cancel", ChatRunRequestSchema, {runId}, ChatStateSchema) : prior;
  console.log(JSON.stringify({runId, previous:prior.runStatus, now:result.runStatus}));
  process.exit(0);
}
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
  ok(tested.ready, JSON.stringify(tested.checks.map(check => ({name:check.name, status:check.status, detail:check.detail}))));
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
  const target = process.argv[3] === "desktop" ? desktop : phone;
  const mode = command === "liberty" ? "liberty" : command.startsWith("goal-") ? "goal" : target.kind === "desktop" ? "desktop" : "phone";
  const phoneInput = "使用真实安卓手机，按一次 HOME 键回到主屏幕，并确认操作。";
  const desktopInput = "使用真实电脑桌面，按一下 Escape 键，然后确认操作。";
  let priorRunId = "";
  if (command === "model-action") {
    const [modes, history] = await Promise.all([
      proactive(), rpc("history", ChatHistoryRequestSchema, {limit:80}, ChatStateSchema)
    ]);
    ok(!modes.exploring && !modes.goalActive, "Disable background modes before timing a new instruction");
    priorRunId = history.runId;
    if (history.runStatus === "paused") {
      const latest = history.messages.findLast(message => message.role === "user")?.text;
      ok(latest === phoneInput || latest === desktopInput, "Only stop an earlier paused QA key instruction");
      const owned = await durableRun(priorRunId);
      equal(owned.agentId, active.id);
      equal(owned.runStatus, "paused");
      await rpc("cancel", ChatRunRequestSchema, {runId:priorRunId}, ChatStateSchema);
      equal((await durableRun(priorRunId)).runStatus, "canceled");
      console.log(JSON.stringify({controlledProvider:true, retiredPausedQaRunId:priorRunId}));
    } else {
      ok(!["running","queued"].includes(history.runStatus), "Wait for the distinct earlier QA run to finish");
    }
  }
  if (command !== "goal-resumed") {
    await Bun.write(join(root, "qualified-model-mode.json"), JSON.stringify({
      scenario:mode, desktop:desktop.id, nonce:randomUUID().replaceAll("-", "")
    }));
  }
  const now = Date.now();
  const trialStart = BigInt(now);
  if (command === "model-action") {
    const actionTitle = target.kind === "desktop" ? "电脑按键" : "手机按键";
    const input = target.kind === "desktop" ? desktopInput : phoneInput;
    const receipt = await rpc("send", ChatSendRequestSchema, {deviceId:target.id,text:input,requestId:randomUUID(),receiptOnly:true}, ChatStateSchema);
    ok(receipt.runId && receipt.runId !== priorRunId, "The request must admit a new run");
    const admissionMs = Date.now() - now;
    console.log(JSON.stringify({controlledProvider:true, admittedRunId:receipt.runId, device:target.kind, admissionMs}));
    let firstTextMs: number | null = null;
    const final = await until(async () => {
      const state = await durableRun(receipt.runId);
      if (firstTextMs === null && state.messages.some(message => message.role === "assistant" && message.text))
        firstTextMs = Date.now() - now;
      return ["queued","running"].includes(state.runStatus) ? undefined : state;
    }, "controlled run didn't settle");
    const events = (await runEvents(receipt.runId)).filter(event => event.createdAtMs >= trialStart);
    const executed = events.find(event => event.title === actionTitle && event.kind === "tool" && event.status === "completed" && event.finishedAtMs >= trialStart);
    const modelTimings = events.flatMap(event => event.modelTiming ? [Object.fromEntries(
      Object.entries(event.modelTiming).filter(([, value]) => typeof value === "bigint").map(([key, value]) => [key, Number(value)])
    )] : []);
    const observation = events.find(event => event.kind === "tool" && event.status === "completed" && event.finishedAtMs >= trialStart && event.title === (target.kind === "desktop" ? "观察电脑" : "观察手机"));
    console.log(JSON.stringify({
      controlledProvider:true, device:target.kind, runId:receipt.runId, firstTextObservedMs:firstTextMs,
      admissionMs, firstObservationMs:observation ? Number(observation.finishedAtMs) - now : null,
      firstActionStartMs:executed ? Number(executed.createdAtMs) - now : null,
      actionWallMs:executed ? Number(executed.finishedAtMs - executed.createdAtMs) : null,
      firstConfirmedDeviceActionMs:executed ? Number(executed.finishedAtMs) - now : null,
      actionReceiptId:executed?.id ?? null, runStatus:final.runStatus,
      audits: events.filter(event => event.kind === "audit").map(event => event.status),
      totalObservedMs:Date.now() - now, modelCalls:modelTimings.length, modelTimings,
    }));
    ok(executed, "real device tool completion receipt");
    equal(final.runStatus, "succeeded", "real tool run must pass final verification");
  } else if (command === "liberty") {
    const before = await proactive();
    ok(!before.goalActive && !before.exploring, "disposable inactive mode");
    const setting = { notificationsEnabled:before.notificationsEnabled,forwardNotifications:before.forwardNotifications,autonomousReply:before.autonomousReply,autonomousInstall:before.autonomousInstall,intervalSeconds:before.intervalSeconds,wechatWatch:before.wechatWatch };
    const toggled = await rpc("proactive/update", ProactiveUpdateRequestSchema, {...setting, notificationsEnabled:false, expectedRevision:before.revision, exploring:true}, ProactiveStateSchema);
    ok(toggled.exploring);
    try {
      const runId = await until(async () => {
        const state = await proactive();
        return state.status === "exploring" && state.activeRunId || undefined;
      }, "Liberty did not dispatch an exploration run", 75000);
      const actions = await until(async () => {
        const events = await runEvents(runId);
        const physicalActions = events.filter(e => e.kind === "tool" && e.status === "completed" && e.createdAtMs >= trialStart && e.finishedAtMs >= trialStart && ["手机按键","电脑按键"].includes(e.title));
        if (new Set(physicalActions.map(e => e.title)).size === 2) return physicalActions;
        const run = await durableRun(runId);
        if (!["running", "queued"].includes(run.runStatus)) throw Error("Liberty ended before both actions: " + run.runStatus + " " + run.error + " " + events.map(e => e.title + "=" + e.status).join(","));
      }, "Liberty never obtained completed side-effect receipts on both devices");
      console.log(JSON.stringify({ controlledProvider:true, liberty:true, runId, actions:actions.map(receipt => ({
        title:receipt.title, id:receipt.id, firstConfirmedReceiptMs:Number(receipt.finishedAtMs)-now
      })) }));
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
    console.log(JSON.stringify({controlledProvider:true, resumed:true, noActionReplay:true, runId:checkpoint.runId,
      newObservationAtMs:Number(events.find(e => e.title.startsWith("观察") && e.kind==="tool" && e.status==="completed" && Number(e.finishedAtMs)>=checkpoint.nextWakeAtMs)!.finishedAtMs)
    }));
  }
  process.exit(0);
}

if (command === "legacy-disk") {
  const name = "calcium-user-file.txt";
  const location = await rpc("files/list", FileLocationSchema, {deviceId:desktop.id,path:"/home/ai/Files"}, FileListingSchema);
  const marker = location.entries.find(entry => entry.name === name);
  ok(marker, "file seeded before native startup exists in the actual Linux desktop");
  const copied = await rpc("files/manage", FileManageRequestSchema, {
    operation:FileOperation.COPY, sources:[{deviceId:desktop.id,path:marker.path}], target:{path:"/"},
  }, FileManageResultSchema);
  const [file] = copied.results;
  ok(file?.target?.path && !file.error, "read marker through the authorized guest file bridge");
  const query = new URLSearchParams({path:file.target.path.replace(/^\/+/, "")});
  const downloaded = await fetch(`http://127.0.0.1:17863/v1/workbench/file?${query}`, {
    headers:{authorization:assistant}, signal:AbortSignal.timeout(8000),
  });
  equal(downloaded.status, 200);
  equal(await downloaded.text(), await readFile(join(root, "layer", "qa-disk", "marker.txt"), "utf8"));
  console.log(JSON.stringify({calciumDiskMarker:true, exactUtf8Bytes:true, nativeRedisReachedDatabasePrepare:true}));
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
    historyRunId: history.runId,
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
async function aclCheck() {
  const [anonymous, badCredential, badOrigin, authenticated] = await Promise.all([
    wsStatus([]), wsStatus(["Authorization: Bearer invalid-fixture"]),
    wsStatus(["Authorization: " + assistant, "Origin: https://qa.invalid"]),
    wsStatus(["Authorization: " + assistant]),
  ]);
  ok(anonymous === 401 || anonymous === 403, "missing service credential rejected");
  ok(badCredential === 401 || badCredential === 403, "invalid service credential rejected");
  ok(badOrigin === 401 || badOrigin === 403, "browser origin cannot use service-only authentication");
  equal(authenticated, 101, "authorized native WS connected without browser origin");
  return { anonymous, badCredential, badOrigin, authenticated };
}
if (command === "acl") {
  console.log(JSON.stringify({ acl: await aclCheck() }));
  process.exit(0);
}
if (command === "desktop-cjk-audio") {
  const deviceId = desktop.id;
  const epoch = (await rpc("desktop/control", DesktopRequestSchema, {deviceId}, DesktopObservationSchema)).epoch;
  async function act(input: MessageInitShape<typeof DesktopActionRequestSchema>) {
    const view = await rpc("desktop/observe", DesktopRequestSchema, {deviceId}, DesktopObservationSchema);
    equal(view.epoch, epoch, "same manual desktop control");
    console.log(JSON.stringify({ desktopAction:input.kind, key:input.key, application:view.applicationId, observation:view.observationId }));
    const receipt = await rpc("desktop/action", DesktopActionRequestSchema, {
      deviceId, actionId:randomUUID(), epoch, observationId:view.observationId, ...input,
    }, DesktopObservationSchema);
    equal(receipt.deviceId, deviceId);
    equal(receipt.epoch, epoch);
  }
  const key = (key: string) => act({kind:"key", key});
  // The packaged Openbox desktop menu begins Browser, Files, Terminal.
  await act({kind:"click", x:640, y:360, button:3});
  await key("Home");
  await key("Down");
  await key("Down");
  await key("Return");
  await until(async () => {
    const current = await rpc("desktop/observe", DesktopRequestSchema, {deviceId}, DesktopObservationSchema);
    return /xterm/i.test(current.applicationId) ? current : undefined;
  }, "desktop Openbox did not open its terminal", 6000);

  const unique = "原生中文回执-" + randomUUID().slice(0,8);
  const textStarted = performance.now();
  await act({kind:"text", text:`printf %s '${unique}' > "$HOME/Files/${unique}.txt"`});
  const pasteMs = Number((performance.now() - textStarted).toFixed(1));
  await key("Return");
  const saved = await until(async () => {
    const list = await rpc("files/list", FileLocationSchema, {deviceId,path:"/home/ai/Files"}, FileListingSchema);
    return list.entries.find(file => file.name === `${unique}.txt`);
  }, "Windows-relayed Linux desktop did not create the expected UTF-8 filename", 6000);
  equal(Number(saved.sizeBytes), Buffer.byteLength(unique), "Linux terminal pasted the full UTF-8 text");
  const copied = await rpc("files/manage", FileManageRequestSchema, {
    operation:FileOperation.COPY, sources:[{deviceId,path:saved.path}], target:{path:"/"},
  }, FileManageResultSchema);
  const [file] = copied.results;
  ok(file?.target?.path && !file.error, "the Linux result was copied into the isolated workspace");
  const query = new URLSearchParams({path:file.target.path.replace(/^\/+/, "")});
  const downloaded = await fetch(`http://127.0.0.1:17863/v1/workbench/file?${query}`, {
    headers:{authorization:assistant}, signal:AbortSignal.timeout(8000),
  });
  equal(downloaded.status, 200);
  equal(await downloaded.text(), unique, "actual XTerm Unicode input bytes persisted in the Linux file");
  console.log(JSON.stringify({linuxCjkInput:true,pasteMs,bytes:Number(saved.sizeBytes),exactFileReadback:true}));

  const audio = await fetch("http://127.0.0.1:17865/desktop/audio", {
    headers:{authorization:phoneToken}, signal:AbortSignal.timeout(16000),
  });
  equal(audio.status, 200);
  ok(/^audio\/L16;rate=48000;channels=2$/i.test(audio.headers.get("content-type") ?? ""));
  const pcm = audio.body!.getReader();
  let high = 0, low = 0, transferred = 0;
  let carry: Buffer<ArrayBufferLike> = Buffer.alloc(0);
  const soundStarted = performance.now();
  try {
    await act({kind:"text", text:"tone=$(mktemp --suffix=.wav); ffmpeg -loglevel error -y -f lavfi -i sine=frequency=660:duration=1.5 \"$tone\" && paplay --device=aiphone \"$tone\"; rm -f \"$tone\""});
    await key("Return");
    while (transferred < 1_700_000 && (high < 8 || low > -8)) {
      const {value,done} = await pcm.read();
      if (done) break;
      const bytes = Buffer.concat([carry, Buffer.from(value)]);
      transferred += value.byteLength;
      const end = bytes.length - bytes.length % 2;
      for (let offset = 0; offset < end; offset += 2) {
        const sample = bytes.readInt16LE(offset);
        high = Math.max(high,sample);
        low = Math.min(low,sample);
      }
      carry = bytes.subarray(end);
    }
  } finally { await pcm.cancel(); }
  ok(high >= 8 && low <= -8, "Linux desktop emitted non-silent stereo PCM through the Windows native relay");
  console.log(JSON.stringify({linuxAudio: "real guest PCM to Windows", firstSignalMs:Number((performance.now()-soundStarted).toFixed(1)),low,high}));
  process.exit(0);
}
const acl = await aclCheck();
console.log(JSON.stringify({ acl }));

const android = await rpc("observe", DeviceRequestSchema, { phoneId: phone.id, ownerId: "local-user", metadataOnly: true }, DeviceObservationSchema, true);
equal(android.rootIdentity, "0", "Android process root identity");
const phoneStarted = performance.now();
const control = await rpc("control", TakeControlRequestSchema, { phoneId: phone.id, ownerId: "local-user", controller: "manual" }, ControlStateSchema, true);
const phoneControlMs = Number((performance.now() - phoneStarted).toFixed(1));
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
const desktopControlMs = Number((performance.now() - desktopStarted).toFixed(1));
const acted = await rpc("desktop/action", DesktopActionRequestSchema, {
  deviceId: desktop.id, actionId: randomUUID(), observationId: window.observationId,
  epoch: window.epoch, kind: "key", key: "Escape",
}, DesktopObservationSchema);
equal(acted.deviceId, desktop.id);
equal(acted.epoch, window.epoch);
const linuxActionMs = Number((performance.now() - desktopStarted).toFixed(1));
const painted = await rpc("desktop/observe", DesktopRequestSchema, { deviceId: desktop.id }, DesktopObservationSchema);
equal(painted.deviceId, desktop.id);
ok(painted.width > 0 && painted.height > 0 && Buffer.from(painted.png).subarray(0,8).toString("hex") === "89504e470d0a1a0a",
  "independent authenticated Linux PNG after the console action");
console.log(JSON.stringify({
  androidActionMs, linuxActionMs, androidRoot: true,
  phoneControlMs, desktopControlMs, desktopFrameReceiptBytes:painted.png.length,
}));
