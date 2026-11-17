import { randomUUID } from "node:crypto";
import { basename, join } from "node:path";

const root = process.env.AIMANITY_QA_ROOT!;
if (!/^Dysprosium-QA-[0-9]+$/.test(basename(root))) throw Error("isolated QA only");
const selection = join(root, "qualified-model-mode.json");
type FunctionCall = {name: string; arguments: string};
type Message = {role:string; content?:string|unknown; tool_calls?:{id:string; function:FunctionCall}[]; tool_call_id?:string};
type Evidence = {tool:string; evidence_id:string; status:string; input?:string; result?:unknown};
function record(value: unknown): Record<string, unknown> | undefined {
  if (typeof value === "string") {
    try { return record(JSON.parse(value)); } catch { return; }
  }
  if (value && typeof value === "object" && !Array.isArray(value)) return value as Record<string, unknown>;
}
function evidenceResult(value: unknown) {
  return Array.isArray(value)
    ? value.flatMap(item => record(record(item)?.text) ?? []).at(0)
    : record(value);
}
function qaAudit(messages: Message[], scenario: string) {
  const user = messages.findLast(message => message.role === "user")?.content;
  const terms = Array.isArray(user) ? user.map(part => record(part)?.text) : [user];
  const request = terms.map(record).find(value => Array.isArray(value?.tool_evidence));
  if (request?.proposed_completion !== "真实设备工具验收步骤完成。" || request.needs_input !== false) return false;
  const evidence = (request.tool_evidence as Evidence[]).filter(row =>
    row.status === "completed" && /^[a-f0-9-]{36}$/i.test(row.evidence_id)
  );
  const actionAndObserve = (action:string, key:string, observe:string) => {
    const n = evidence.findIndex(row => {
      const result = evidenceResult(row.result);
      return row.tool === action && record(row.input)?.key === key
        && result?.success === true && result.result_unknown !== true && result.not_executed !== true;
    });
    if (n < 0) return false;
    const actionResult = evidenceResult(evidence[n].result);
    return evidence.slice(n + 1).some(row => {
      const result = evidenceResult(row.result);
      return row.tool === observe && typeof result?.observation_id === "string" && Boolean(result.observation_id)
        && (actionResult?.device_id === undefined || result.device_id === actionResult.device_id);
    });
  };
  if (scenario === "desktop") return actionAndObserve("computer_key", "Escape", "computer_observe");
  if (!actionAndObserve("phone_key", "HOME", "phone_observe")) return false;
  if (scenario === "liberty") return actionAndObserve("computer_key", "Escape", "computer_observe");
  if (scenario === "goal") {
    const wait = evidence.findIndex(row => row.tool === "task_wait");
    return wait >= 0 && evidence.slice(wait + 1).some(row =>
      row.tool === "phone_observe" && typeof evidenceResult(row.result)?.observation_id === "string");
  }
  return scenario === "phone";
}
function observationReceipt(content: unknown) {
  const blocks = Array.isArray(content) ? content.map(block => record(block)?.text) : [content];
  return blocks
    .flatMap(block => typeof block === "string" ? block.split("\n") : [block])
    .map(record)
    .find(value => typeof value?.success === "boolean");
}
function sse(chunks: object[]) {
  return new Response(chunks.map(data => "data: " + JSON.stringify(data) + "\n\n").join("") + "data: [DONE]\n\n",
    {headers:{"content-type":"text/event-stream"}});
}
const server = Bun.serve({
  hostname: process.env.AIMANITY_QA_MODEL_BIND ?? "127.0.0.1",
  port: 17998,
  async fetch(request) {
    if (request.method === "GET" && new URL(request.url).pathname === "/health") return new Response("ok");
    if (request.method !== "POST" || new URL(request.url).pathname !== "/v1/chat/completions") return new Response(null, {status:404});
    const body = await request.json() as { model:string; stream:boolean; messages: Message[]; tools?:{function:FunctionCall}[] };
    if (body.model !== "qa-dysprosium-local") return new Response(null, {status:400});
    const names = new Set(body.tools?.map(tool => tool.function.name) ?? []);
    const messageContent = JSON.stringify(body.messages.filter(message => message.role === "user").at(-1)?.content ?? "");
    const isProviderCheck = !names.has("phone_observe") && !names.has("computer_observe") && !names.has("task_finish");
    let payload: {content?:string; tool_calls?:object[]} = {};
    if (names.size === 1 && names.has("audit_result")) {
      const { scenario } = await Bun.file(selection).json();
      const approved = qaAudit(body.messages,scenario);
      console.log(JSON.stringify({scenario, audit:approved}));
      payload.tool_calls = [{
        index:0, id:randomUUID(), type:"function",
        function:{name:"audit_result", arguments:JSON.stringify({
          approved, feedback:approved ? "持久账本含所请求的真实按键回执及后续独立观察。" : "账本缺少验收所需的确认按键、后续观察或等待证据。"
        })}
      }];
    } else if (isProviderCheck) {
      const output = body.messages.findLast(message => message.role === "tool")?.content;
      if (names.has("diagnostic_echo") || output) {
        if (output) {
          const receipt = /"receipt"\s*:\s*"([^"]+)"/.exec(JSON.stringify(output).replaceAll('\\"','"'))?.[1];
          payload.content = receipt ?? "AIMANITY_QA_READY";
        } else {
          const code = /Call diagnostic_echo with code ([^\\.\s"]+)/.exec(messageContent)?.[1];
          if (!code) return new Response("diagnostic code missing", {status:400});
          payload.tool_calls = [{index:0,id:randomUUID(),type:"function",function:{name:"diagnostic_echo",arguments:JSON.stringify({code})}}];
        }
      } else {
        payload.content = /Reply exactly with this code:\s*([^\s"\\]+)/.exec(messageContent)?.[1]
          ?? (messageContent.includes("Read the four digits") ? "7319" : "AIMANITY_QA_READY");
      }
    } else {
      const mode = await Bun.file(selection).json() as {scenario:"phone"|"desktop"|"goal"|"liberty"; desktop:string; nonce:string};
      if (!/^[a-f0-9]{32}$/.test(mode.nonce ?? "")) return new Response("isolated QA mode nonce missing", {status:409});
      const prefix = "qa_" + mode.nonce + "_";
      const outputs = new Map(body.messages.flatMap((message,index) =>
        message.role === "tool" && message.tool_call_id?.startsWith(prefix)
          ? [[message.tool_call_id, {content:message.content, at:index}] as const] : []
      ));
      let count = 0;
      const returned = new Map<string, {id:string; content:unknown; at:number; arguments:string}[]>();
      for (const message of body.messages) for (const call of message.tool_calls ?? []) {
        const output = outputs.get(call.id);
        if (!output) continue;
        const items = returned.get(call.function.name) ?? [];
        items.push({id:call.id,content:output.content,at:output.at,arguments:call.function.arguments});
        returned.set(call.function.name,items);
        count++;
      }
      const observed = (name: string) => returned.get(name)?.at(-1)?.content;
      const done = (name: string) => returned.has(name);
      const last = (name: string) => returned.get(name)?.at(-1)?.at ?? -1;
      const use = (name: string, args: object) => {
        if (!names.has(name)) throw Error("expected tool missing: " + name);
        console.log(JSON.stringify({ scenario:mode.scenario, tool:name, providerRequest:count + 1 }));
        payload.tool_calls = [{index:0, id:prefix + randomUUID().replaceAll("-", "").slice(0,16),type:"function",function:{name,arguments:JSON.stringify(args)}}];
      };
      const onDesktop = () => {
        if (!done("computer_observe")) {
          use("computer_observe", {device_id:mode.desktop});
          return true;
        }
        if (!done("computer_key")) {
          const receipt = observationReceipt(observed("computer_observe"));
          if (receipt?.success === false) {
            const cause = receipt.error ?? receipt.reason ?? "电脑观察未执行";
            const summary = String(cause);
            console.log(JSON.stringify({scenario:mode.scenario, observation:"failed", code:receipt.code, error:summary}));
            if (!done("task_finish")) use("task_finish", {status:"needs_input",summary});
            else payload.content = summary;
            return true;
          }
          const observation_id = receipt?.observation_id;
          if (receipt?.device_id !== mode.desktop || typeof observation_id !== "string" || !/^[a-f0-9-]{36}$/i.test(observation_id)) {
            throw Error("desktop tool output lacks a valid observation for the configured guest");
          }
          use("computer_key", {device_id:mode.desktop,observation_id,key:"Escape"});
          return true;
        }
        if (last("computer_observe") < last("computer_key")) {
          use("computer_observe", {device_id:mode.desktop});
          return true;
        }
        return false;
      };
      const onPhone = () => {
        if (!done("phone_observe") || last("phone_observe") < last("phone_key")) {
          use("phone_observe", {});
          return true;
        }
        if (!done("phone_key")) {
          use("phone_key", {key:"HOME"});
          return true;
        }
        return false;
      };
      const hasSkill = (name: string) => returned.get("skill_read")?.some(tool =>
        tool.arguments.includes(name)
      ) ?? false;
      if (count > 20) return new Response("bounded controlled model sequence", {status:409});
      if (!count) payload.content = "我先查看真实设备。";
      if (mode.scenario === "liberty" && !done("device_list") && names.has("device_list")) {
        use("device_list", {});
      } else if (mode.scenario !== "desktop" && names.has("phone_observe") && !hasSkill("phone-use") && !done("phone_observe")) {
        use("skill_read", {name:"phone-use"});
      } else if (mode.scenario !== "desktop" && names.has("phone_observe") && onPhone()) {
        // The phone completed its first observation or action turn.
      } else if ((mode.scenario === "desktop" || mode.scenario === "liberty") && names.has("computer_observe") && !hasSkill("computer-use") && !done("computer_observe")) {
        use("skill_read", {name:"computer-use"});
      } else if ((mode.scenario === "desktop" || mode.scenario === "liberty") && names.has("computer_observe") && onDesktop()) {
        // The desktop completed its first observation or action turn.
      } else if (mode.scenario === "goal" && !done("task_wait")) {
        use("task_wait", {seconds:45, reason:"等待验收人员重启原生工作台后核实手机现状"});
      } else if (mode.scenario === "goal" && last("phone_observe") < last("task_wait")) {
        use("phone_observe", {});
      } else if (!done("task_finish") && names.has("task_finish")) {
        use("task_finish", {status:"completed",summary:"真实设备工具验收步骤完成。"});
      } else {
        payload.content = "设备操作回执已返回。";
      }
    }
    const finish_reason = payload.tool_calls ? "tool_calls" : "stop";
    return body.stream === false
      ? Response.json({choices:[{index:0,message:{role:"assistant",...payload},finish_reason}]})
      : sse([{choices:[{index:0,delta:{role:"assistant",...payload},finish_reason:null}]},
             {choices:[{index:0,delta:{},finish_reason}]}]);
  },
});
console.log("Controlled local model listening on " + server.port);
