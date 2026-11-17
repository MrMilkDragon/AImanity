use ai_phone_adb::{Client as Adb, Limits};
use ai_phone_api::wire::*;
use ai_phone_host_runtime::vm_control;
use prost::Message as _;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{env, fs, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Instant, sleep, timeout},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const OLD: &str = "http://127.0.0.1:17860";
const NEW: &str = "http://127.0.0.1:17863";
const MODEL: &str = "http://10.0.2.2:17998/v1";
const MODEL_NAME: &str = "Calcium WHPX upgrade QA";
const KEY: &str = "calcium-windows-qa-fixture-key";
const ASK: &str = "Reply with the phrase CALCIUM_QA_REPLY.";
const ANSWER: &str = "CALCIUM_QA_REPLY";
const PROTO: &str = "application/x-protobuf";
const SEC: Duration = Duration::from_secs(1);

#[derive(Serialize, Deserialize)]
struct State {
    agent: String,
    model: String,
    model_revision: u64,
    phone: String,
    linux: String,
    conversation: String,
    request: String,
    sent: String,
    reply: String,
    android_file: String,
    workspace_path: String,
    linux_path: String,
    file_content: String,
    disk: String,
}

fn uuid() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let id = hex::encode(bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &id[..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..]
    ))
}

struct Workbench {
    http: Client,
    base: &'static str,
    token: String,
}

impl Workbench {
    async fn open(home: &Path, old: bool) -> Result<Self> {
        let http = Client::builder().no_proxy().timeout(120 * SEC).build()?;
        if old {
            let token = http
                .post(format!("{OLD}/api/auth/enter"))
                .header("origin", OLD)
                .header("content-type", PROTO)
                .body(Vec::new())
                .send()
                .await?
                .error_for_status()?
                .headers()
                .get("set-cookie")
                .ok_or("Calcium did not return an authenticated session")?
                .to_str()?
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            Ok(Self {
                http,
                base: OLD,
                token,
            })
        } else {
            let token = vm_control::native_credentials(&home.join("user/qemu/control.key"))
                .await?
                .assistant_token;
            Ok(Self {
                http,
                base: NEW,
                token,
            })
        }
    }

    async fn send(&self, path: &str, media: &str, body: Vec<u8>) -> Result<Response> {
        let request = self
            .http
            .post(format!("{}{path}", self.base))
            .header("content-type", media)
            .body(body);
        let request = if self.base == OLD {
            request.header("origin", OLD).header("cookie", &self.token)
        } else {
            request.bearer_auth(self.token.trim())
        };
        Ok(request.send().await?.error_for_status()?)
    }

    async fn rpc<I: prost::Message, O: prost::Message + Default>(
        &self,
        method: &str,
        req: I,
    ) -> Result<O> {
        let path = if self.base != OLD {
            format!("/v1/{method}")
        } else if method.contains('/') {
            format!("/api/{method}")
        } else {
            format!("/api/assistant/{method}")
        };
        Ok(O::decode(
            self.send(&path, PROTO, req.encode_to_vec())
                .await?
                .bytes()
                .await?,
        )?)
    }

    async fn file(&self, path: &str) -> Result<Vec<u8>> {
        let prefix = if self.base == OLD { "/api" } else { "/v1" };
        let request = self
            .http
            .get(format!("{}{prefix}/workbench/file", self.base))
            .query(&[("path", path.trim_start_matches('/'))]);
        let request = if self.base == OLD {
            request.header("cookie", &self.token)
        } else {
            request.bearer_auth(self.token.trim())
        };
        Ok(request
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }
}

async fn model_response(socket: TcpStream) -> Result<()> {
    let mut reader = BufReader::new(socket);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    assert!(line.starts_with("POST /v1/chat/completions "));
    let mut size = 0;
    loop {
        line.clear();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            size = value.trim().parse()?;
        }
    }
    assert!((1..1_000_000).contains(&size));
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    let input: Value = serde_json::from_slice(&bytes)?;
    let messages = input["messages"]
        .as_array()
        .ok_or("model fixture messages")?;
    let prompt = messages
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message["content"].to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let token = |prefix: &str| {
        prompt
            .split_once(prefix)
            .and_then(|(_, text)| text.split(|c: char| !c.is_ascii_alphanumeric()).next())
    };
    let diagnostic = "qa_calcium_diagnostic";
    let choice = if let Some(tool) = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == diagnostic)
    {
        let raw = tool["content"]
            .as_str()
            .ok_or("model diagnostic tool receipt")?;
        let receipt: Value = serde_json::from_str(raw)?;
        json!({"content":receipt["receipt"]})
    } else if let Some(code) = token("Call diagnostic_echo with code ") {
        json!({"tool_calls":[{"index":0,"id":diagnostic,"type":"function",
            "function":{"name":"diagnostic_echo","arguments":json!({"code":code}).to_string()}}]})
    } else if let Some(code) = token("Reply exactly with this code: ") {
        json!({"content":code})
    } else if prompt.contains("Read the four digits") {
        json!({"content":"7319"})
    } else {
        let tool_count = input["tools"].as_array().map_or(0, Vec::len);
        println!(
            "{}",
            json!({"phase":"calcium_fixture_chat","contains_seed_marker":prompt.contains(ANSWER),"tool_count":tool_count})
        );
        json!({"content":ANSWER})
    };
    let mut choice = choice;
    let finish = if choice["tool_calls"].is_array() {
        "tool_calls"
    } else {
        "stop"
    };
    choice["role"] = json!("assistant");
    let (mime, response) = if input["stream"] == false {
        (
            "application/json",
            json!({"choices":[{"index":0,"message":choice,"finish_reason":finish}]}).to_string(),
        )
    } else {
        let delta = json!({"choices":[{"index":0,"delta":choice,"finish_reason":null}]});
        let done = json!({"choices":[{"index":0,"delta":{},"finish_reason":finish}]});
        (
            "text/event-stream",
            format!("data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n"),
        )
    };
    let mut socket = reader.into_inner();
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).as_bytes()).await?;
    socket.write_all(response.as_bytes()).await?;
    Ok(())
}

fn managed<'a>(registry: &'a RegistryState, kind: &str) -> &'a str {
    &registry
        .devices
        .iter()
        .find(|device| device.kind == kind && device.runtime_state == "managed")
        .unwrap_or_else(|| panic!("missing managed {kind}"))
        .id
}

async fn android_file(command: &str) -> Result<String> {
    let output = Adb::new(15037, "127.0.0.1:5565".into())?
        .shell(
            command,
            Limits {
                stdout: 4096,
                stderr: 4096,
            },
            10 * SEC,
        )
        .await?;
    assert_eq!(
        output.code,
        0,
        "Android marker command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

async fn history(workbench: &Workbench, conversation: &str) -> Result<ChatState> {
    workbench
        .rpc(
            "history",
            ChatHistoryRequest {
                conversation_id: conversation.into(),
                limit: 40,
                ..Default::default()
            },
        )
        .await
}

async fn seed(workbench: &Workbench, home: &Path) -> Result<State> {
    let provider = TcpListener::bind("127.0.0.1:17998").await?;
    let listener = tokio::spawn(async move {
        while let Ok((socket, _)) = provider.accept().await {
            tokio::spawn(async move {
                if let Err(error) = model_response(socket).await {
                    eprintln!("QA model fixture: {error}");
                }
            });
        }
    });
    let registry: RegistryState = workbench.rpc("registry/state", RegistryRequest {}).await?;
    assert!(
        registry.agents.is_empty(),
        "one wholly isolated Calcium deployment"
    );
    let phone = managed(&registry, "phone").to_owned();
    let linux = managed(&registry, "desktop").to_owned();
    let (agent, model, request) = (uuid()?, uuid()?, uuid()?);
    let draft = ModelDraft {
        provider: "deepseek".into(),
        protocol: "chat-completions".into(),
        base_url: MODEL.into(),
        model: "qa-dysprosium-local".into(),
        api_key: KEY.into(),
        ..Default::default()
    };
    let test: ModelTestResult = workbench
        .rpc(
            "registry/model-test",
            ConnectionTestRequest {
                id: model.clone(),
                draft: Some(draft.clone()),
                ..Default::default()
            },
        )
        .await?;
    assert!(
        test.ready,
        "Calcium actually tested the configured provider: {:?}",
        test.checks
    );
    println!("Calcium QA provider capabilities verified");
    let registry: RegistryState = workbench
        .rpc(
            "registry/agent-config-save",
            AgentConfigurationSaveRequest {
                agent: Some(AgentSaveRequest {
                    id: agent.clone(),
                    name: MODEL_NAME.into(),
                    model_connection_id: model.clone(),
                    ..Default::default()
                }),
                connection: Some(ConnectionSaveRequest {
                    id: model.clone(),
                    name: MODEL_NAME.into(),
                    draft: Some(draft),
                    verification_id: test.verification_id,
                    ..Default::default()
                }),
            },
        )
        .await?;
    assert_eq!(registry.active_agent_id, agent);
    println!("Calcium QA Agent and model committed");
    let model_revision = registry
        .models
        .iter()
        .find(|row| row.id == model)
        .ok_or("saved Calcium model absent")?
        .revision;
    let send: ChatState = workbench
        .rpc(
            "send",
            ChatSendRequest {
                text: ASK.into(),
                request_id: request.clone(),
                agent_id: agent.clone(),
                receipt_only: true,
                ..Default::default()
            },
        )
        .await?;
    let conversation = send.conversation_id;
    assert!(
        !conversation.is_empty(),
        "Calcium committed conversation ID"
    );
    println!("Calcium QA message admitted");
    let (sent, reply) = timeout(60 * SEC, async {
        let mut previous = String::new();
        loop {
            let state = history(workbench, &conversation).await?;
            if state.run_status != previous {
                println!(
                    "{}",
                    json!({"phase":"calcium_chat_history","status":state.run_status,"message_count":state.messages.len()})
                );
                previous = state.run_status.clone();
            }
            assert!(!matches!(state.run_status.as_str(), "failed" | "canceled"));
            if let (Some(sent), Some(reply)) = (
                state
                    .messages
                    .iter()
                    .find(|m| m.role == "user" && m.request_id == request && m.text == ASK),
                state
                    .messages
                    .iter()
                    .find(|m| m.role == "assistant" && m.text.contains(ANSWER)),
            ) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>((
                    sent.id.clone(),
                    reply.id.clone(),
                ));
            }
            sleep(Duration::from_millis(400)).await;
        }
    })
    .await??;
    listener.abort();

    let file = format!("calcium-qa-{request}.txt");
    let file_content = format!("Calcium committed fixture {request}");
    let root = FileLocation {
        path: "/".into(),
        ..Default::default()
    }
    .encode_to_vec();
    let boundary = format!("QA-{request}");
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"location\"\r\nContent-Type: {PROTO}\r\n\r\n").into_bytes();
    body.extend(root);
    body.extend(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file}\"\r\nContent-Type: text/plain\r\n\r\n{file_content}\r\n--{boundary}--\r\n").into_bytes());
    let uploaded = workbench
        .send(
            "/api/files/upload",
            &format!("multipart/form-data; boundary={boundary}"),
            body,
        )
        .await?;
    let location = FileLocation::decode(uploaded.bytes().await?)?;
    let workspace_path = location.path.clone();
    let copied: FileManageResult = workbench
        .rpc(
            "files/manage",
            FileManageRequest {
                operation: FileOperation::Copy.into(),
                sources: vec![location],
                target: Some(FileLocation {
                    device_id: linux.clone(),
                    path: "/home/ai/Files".into(),
                }),
                name: String::new(),
            },
        )
        .await?;
    assert_eq!(copied.results.len(), 1);
    assert_eq!(copied.results[0].error, "");
    let linux_path = copied.results[0]
        .target
        .as_ref()
        .ok_or("Linux copy omitted its destination")?
        .path
        .clone();
    android_file(&format!(
        "printf '%s' '{file_content}' > '/sdcard/Download/{file}'"
    ))
    .await?;
    let disk = fs::read_to_string(home.join("user/qemu/disk/base.sha256"))?
        .trim()
        .to_owned();
    let state = State {
        agent,
        model,
        model_revision,
        phone,
        linux,
        conversation,
        request,
        sent,
        reply,
        android_file: file,
        workspace_path,
        linux_path,
        file_content,
        disk,
    };
    verify(workbench, home, &state).await?;
    Ok(state)
}

async fn verify(workbench: &Workbench, home: &Path, state: &State) -> Result<()> {
    let registry: RegistryState = workbench.rpc("registry/state", RegistryRequest {}).await?;
    assert_eq!(registry.active_agent_id, state.agent);
    assert_eq!(managed(&registry, "phone"), state.phone);
    assert_eq!(managed(&registry, "desktop"), state.linux);
    let agent = registry
        .agents
        .iter()
        .find(|row| row.id == state.agent)
        .ok_or("persisted active agent missing")?;
    assert_eq!(
        (agent.name.as_str(), agent.model_connection_id.as_str()),
        (MODEL_NAME, state.model.as_str())
    );
    let model = registry
        .models
        .iter()
        .find(|row| row.id == state.model)
        .ok_or("persisted model missing")?;
    assert_eq!(model.revision, state.model_revision);
    let profile = model
        .profile
        .as_ref()
        .ok_or("persisted model profile missing")?;
    assert_eq!(
        (
            profile.provider.as_str(),
            profile.model.as_str(),
            profile.base_url.as_str()
        ),
        ("deepseek", "qa-dysprosium-local", MODEL)
    );
    let credential: ModelCredential = workbench
        .rpc(
            "registry/credential",
            ConnectionCredentialRequest {
                id: state.model.clone(),
                expected_revision: state.model_revision,
            },
        )
        .await?;
    assert!(
        credential.api_key == KEY,
        "synthetic model credential missing after reload"
    );
    let history = history(workbench, &state.conversation).await?;
    assert_eq!(history.conversation_id, state.conversation);
    assert!(
        history
            .messages
            .iter()
            .any(|row| row.id == state.sent && row.request_id == state.request && row.text == ASK)
    );
    assert!(
        history.messages.iter().any(|row| row.id == state.reply
            && row.role == "assistant"
            && row.text.contains(ANSWER))
    );
    for (id, path, expected) in [
        ("", "/", &state.workspace_path),
        (state.linux.as_str(), "/home/ai/Files", &state.linux_path),
    ] {
        let listing: FileListing = workbench
            .rpc(
                "files/list",
                FileLocation {
                    device_id: id.into(),
                    path: path.into(),
                },
            )
            .await?;
        assert!(
            listing.entries.iter().any(|entry| &entry.path == expected
                && entry.size_bytes == state.file_content.len() as u64),
            "persisted file absent from {path}"
        );
    }
    assert_eq!(
        workbench.file(&state.workspace_path).await?,
        state.file_content.as_bytes()
    );
    let runtime = if workbench.base == OLD {
        "calcium"
    } else {
        "native"
    };
    let readback: FileManageResult = workbench
        .rpc(
            "files/manage",
            FileManageRequest {
                operation: FileOperation::Copy.into(),
                sources: vec![FileLocation {
                    device_id: state.linux.clone(),
                    path: state.linux_path.clone(),
                }],
                target: Some(FileLocation {
                    device_id: String::new(),
                    path: "/".into(),
                }),
                name: format!("{runtime}-readback-{}.txt", state.request),
            },
        )
        .await?;
    assert_eq!(readback.results.len(), 1);
    assert_eq!(readback.results[0].error, "");
    let readback = readback.results[0]
        .target
        .as_ref()
        .ok_or("Linux read-back omitted its destination")?;
    assert_eq!(
        workbench.file(&readback.path).await?,
        state.file_content.as_bytes()
    );
    assert_eq!(
        android_file(&format!("cat '/sdcard/Download/{}'", state.android_file)).await?,
        state.file_content
    );
    assert_eq!(
        fs::read_to_string(home.join("user/qemu/disk/base.sha256"))?.trim(),
        state.disk
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let root = env::var("AIMANITY_QA_ROOT")?;
    assert!(cfg!(windows) && env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"));
    assert!(
        root.get(1..)
            .is_some_and(|path| path.starts_with(":\\Dysprosium-QA-"))
    );
    let root = Path::new(&root);
    let home = root.join("profile/dev.aiphone.client/host");
    let state_file = root.join("calcium-state.json");
    let old = match env::args().nth(1).as_deref() {
        Some("seed") => true,
        Some("read") => false,
        _ => return Err("expected seed or read".into()),
    };
    let workbench = Workbench::open(&home, old).await?;
    let started = Instant::now();
    if old {
        let snapshot = seed(&workbench, &home).await?;
        fs::write(&state_file, serde_json::to_vec(&snapshot)?)?;
    } else {
        verify(
            &workbench,
            &home,
            &serde_json::from_slice(&fs::read(&state_file)?)?,
        )
        .await?;
    }
    println!(
        "{}",
        json!({"phase":if old {"calcium_seed"} else {"dysprosium_same_profile"},
        "chat":true,"agent_model":true,"synthetic_credential":true,"managed_device_ids":true,
        "workspace":true,"linux_files":true,"android_file":true,"base_identity":true,"elapsed_ms":started.elapsed().as_millis()})
    );
    Ok(())
}
