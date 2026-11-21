//! Application tools are written by the idle model and run as pure data transformations.
mod author;
mod inventory;
mod program;
use crate::*;
pub(crate) use author::work;
use program::Program;
use serde_json::{Value, json};

const IDLE: &str = "NOT EXISTS (SELECT 1 FROM runs WHERE status='running' OR
    (status='queued' AND (next_wake_at IS NULL OR next_wake_at<=now())))";

pub(crate) fn verify_document(content: &str) -> Result<(), String> {
    Program::read(content)?.verify()
}

async fn enabled(db: &PgPool, app_id: &str) -> Result<Option<Program>, String> {
    let content: Option<String> = sqlx::query_scalar(
        "SELECT s.content FROM app_mcp_jobs j JOIN runtime_skills s ON s.id=j.skill_id
         WHERE j.app_id=$1 AND j.state='ready' AND s.enabled AND s.deleted_at IS NULL",
    )
    .bind(app_id)
    .fetch_optional(db)
    .await
    .map_err(|e| e.to_string())?;
    content.as_deref().map(Program::read).transpose()
}

pub(crate) async fn model_tool(db: &PgPool, app_id: Option<&str>) -> Option<Value> {
    let program = enabled(db, app_id?).await.ok()??;
    Some(
        json!({"type":"function","name":format!("app_mcp_{}",program.name),
        "description":program.description,"parameters":program.schema()}),
    )
}

pub(crate) async fn call(
    db: &PgPool,
    app_id: &str,
    name: &str,
    args: &Value,
    run: Option<Uuid>,
) -> Result<Value, String> {
    let program = enabled(db, app_id)
        .await?
        .filter(|p| p.name == name)
        .ok_or("application tool unavailable")?;
    execute(db, app_id, &program, args, run).await
}

async fn execute(
    db: &PgPool,
    app_id: &str,
    program: &Program,
    args: &Value,
    run: Option<Uuid>,
) -> Result<Value, String> {
    if !program.uses_view {
        return program.call(args, None);
    }
    let kind = if app_id.starts_with("linux:") {
        "desktop"
    } else {
        "phone"
    };
    let row:Option<(String,i64,Value)>=sqlx::query_as(
        "WITH latest AS (SELECT o.* FROM app_learning_observations o WHERE o.device_id=(
           SELECT id FROM devices WHERE owner_id='local-user' AND kind=$1 AND runtime_state='managed' ORDER BY created_at,id LIMIT 1)
           AND ($2::uuid IS NULL OR o.run_id=$2) ORDER BY o.observed_at DESC,o.event_id DESC LIMIT 1)
         SELECT o.observation_id,(extract(epoch FROM o.observed_at)*1000)::bigint,
           jsonb_path_query_array(e.result,'$[*] ? (@.type == \"input_text\").text')
         FROM latest o JOIN harness_events e ON e.id=o.event_id WHERE o.app_id=$3 AND e.status='completed'")
        .bind(kind).bind(run).bind(app_id).fetch_optional(db).await.map_err(|e|e.to_string())?;
    let (id, time, parts) = row.ok_or("observe this application first")?;
    let frame = parts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|s| serde_json::from_str::<Value>(s).ok())
        .find(|v| v["observation_id"] == id)
        .ok_or("application observation is unavailable")?;
    let view = json!({"application":app_id,"elements":frame["elements"].as_array().map(Vec::as_slice).unwrap_or_default(),
        "window_id":frame["window_id"],"width":frame["width"],"height":frame["height"]});
    let result = program.call(args, Some(&view))?;
    Ok(json!({"observation_id":id,"observed_at_ms":time,"data":result}))
}

#[derive(Default)]
enum Session {
    #[default]
    New,
    Initializing,
    Ready,
}

impl Session {
    async fn respond(&mut self, db: &PgPool, app_id: &str, req: &Value) -> Option<Value> {
        let id = &req["id"];
        let error = |code: i32, message: &str| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}});
        let Some(method) = req["method"].as_str().filter(|_| req["jsonrpc"] == "2.0") else {
            return Some(error(-32600, "Invalid Request"));
        };
        if req.get("id").is_none() {
            if method == "notifications/initialized" && matches!(self, Self::Initializing) {
                *self = Self::Ready;
            }
            return None;
        }
        let params = &req["params"];
        let result = match method {
            "initialize" if matches!(self, Self::New) => {
                *self = Self::Initializing;
                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":app_id,"version":"1"}})
            }
            "ping" => json!({}),
            _ if !matches!(self, Self::Ready) => {
                return Some(error(-32002, "Server not initialized"));
            }
            "tools/list" | "tools/call" => {
                let p = match enabled(db, app_id).await {
                    Ok(p) => p,
                    Err(e) => return Some(error(-32603, &e)),
                };
                if method == "tools/list" {
                    json!({"tools":p.into_iter().map(|p|json!({"name":p.name,"description":p.description,"inputSchema":p.schema()})).collect::<Vec<_>>()})
                } else {
                    let Some(p) = p.filter(|p| params["name"].as_str() == Some(&p.name)) else {
                        return Some(error(-32602, "Unknown tool"));
                    };
                    let result = execute(db, app_id, &p, &params["arguments"], None).await;
                    let failed = result.is_err();
                    let text = match result {
                        Ok(Value::String(s)) => s,
                        Ok(v) => v.to_string(),
                        Err(e) => e,
                    };
                    json!({"content":[{"type":"text","text":text}],"isError":failed})
                }
            }
            _ => return Some(error(-32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}

pub(crate) async fn stdio(db: &PgPool, app_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, Write};
    if !crate::application_identity::valid(app_id) {
        return Err("invalid application ID".into());
    }
    let mut session = Session::default();
    for line in std::io::stdin().lock().lines() {
        let reply = match serde_json::from_str(&line?) {
            Ok(req) => session.respond(db, app_id, &req).await,
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(reply) = reply {
            let mut out = std::io::stdout().lock();
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}
