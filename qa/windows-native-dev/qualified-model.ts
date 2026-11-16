import { randomUUID } from "node:crypto";
import { join } from "node:path";

const root = process.env.AIMANITY_QA_ROOT!;
if (!/Dysprosium-QA-|aimanity-dysprosium-qa-/i.test(root)) throw Error("isolated QA only");
const selection = join(root, "qualified-model-mode.json");
type FunctionCall = {name: string; arguments: string};
type Message = {role:string; content?:string|unknown; tool_calls?:{id:string; function:FunctionCall}[]; tool_call_id?:string};
function observationId(value: unknown): string | undefined {
  if (typeof value === "string") {
    try { return observationId(JSON.parse(value)); } catch { return; }
  }
  if (!value || typeof value !== "object") return;
  if ("observation_id" in value && typeof value.observation_id === "string") return value.observation_id;
  for (const child of Object.values(value)) {
    const result = observationId(child);
    if (result) return result;
  }
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
    if (isProviderCheck) {
      if (names.has("diagnostic_echo")) {
        const output = body.messages.findLast(message => message.role === "tool")?.content;
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
      const mode = await Bun.file(selection).json() as {scenario:"phone"|"desktop"|"goal"|"liberty"; desktop:string};
      const lastUser = body.messages.findLastIndex(message => message.role === "user");
      const turn = body.messages.slice(lastUser + 1);
      let count = 0;
      const returned = new Map<string, {id:string; content:unknown}[]>();
      for (const message of turn) for (const call of message.tool_calls ?? []) {
        const output = turn.find(item => item.role === "tool" && item.tool_call_id === call.id);
        if (!output) continue;
        const items = returned.get(call.function.name) ?? [];
        items.push({id:call.id,content:output.content});
        returned.set(call.function.name,items);
        count++;
      }
      const observed = (name: string) => returned.get(name)?.at(-1)?.content;
      const done = (name: string) => returned.has(name);
      const use = (name: string, args: object) => {
        if (!names.has(name)) throw Error("expected tool missing: " + name);
        console.log(JSON.stringify({ scenario:mode.scenario, tool:name, providerRequest:count + 1 }));
        payload.tool_calls = [{index:0, id:randomUUID(),type:"function",function:{name,arguments:JSON.stringify(args)}}];
      };
      const desktop = mode.scenario === "desktop" || (mode.scenario === "liberty" && !names.has("phone_observe"));
      const observe = desktop ? "computer_observe" : "phone_observe";
      const argumentsForObservation = desktop ? {device_id:mode.desktop} : {};
      if (count > 8) return new Response("bounded controlled model sequence", {status:409});
      if (!done(observe)) {
        payload.content = "我先查看真实设备。";
        use(observe, argumentsForObservation);
      } else if (mode.scenario === "liberty" && !done("device_list") && names.has("device_list")) {
        use("device_list", {});
      } else {
        const action = desktop ? "computer_key" : "phone_key";
        if (!done(action) && names.has(action)) {
          if (desktop) {
            const observation_id = observationId(observed(observe));
            if (!observation_id) return new Response("desktop observation receipt missing", {status:400});
            use(action, { device_id:mode.desktop, observation_id, key:"Escape" });
          } else {
            use(action, { key:"HOME" });
          }
        } else if (mode.scenario === "goal" && !done("task_wait") && names.has("task_wait")) {
          use("task_wait", { seconds:30, reason:"验收临时等待，退出重启后继续" });
        } else if (mode.scenario === "goal" && done("task_wait") && (returned.get(observe)?.length ?? 0) < 2) {
          use(observe, argumentsForObservation);
        } else if (!done("task_finish") && names.has("task_finish")) {
          use("task_finish", {status:"completed",summary:"真实设备工具验收步骤完成。"});
        } else {
          payload.content = "设备操作回执已返回。";
        }
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
