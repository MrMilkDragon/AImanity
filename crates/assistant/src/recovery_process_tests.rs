use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    process::{Child, Command as ChildCommand},
    sync::Notify,
    time::{sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

const ROOT_OWNER_PASSWORD: &str = "isolated-fixture-password";
const CHILD: &str = "worker::process_recovery_tests::worker_child";

fn private_json(path: &Path, value: &Value) {
    let file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    serde_json::to_writer(file, value).unwrap();
}

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Notify,
}

impl Gate {
    async fn hold(&self) {
        self.entered.store(true, Ordering::Release);
        self.released.notified().await;
    }
}

#[derive(Default)]
struct Model {
    calls: Mutex<HashMap<String, usize>>,
    outage: Gate,
    long: Gate,
}

impl Model {
    fn calls(&self, name: &str) -> usize {
        self.calls.lock().unwrap()[name]
    }
}

async fn respond(State(model): State<Arc<Model>>, Json(body): Json<Value>) -> impl IntoResponse {
    let name = body["model"].as_str().unwrap();
    let index = {
        let mut calls = model.calls.lock().unwrap();
        let count = calls.entry(name.into()).or_default();
        *count += 1;
        *count
    };
    let history = body["input"].to_string();
    let tool = |name_of_tool, args: Value| {
        json!({
            "type":"function_call", "call_id":format!("{name}-{index}"),
            "name":name_of_tool, "arguments":args.to_string()
        })
    };
    let pause = |summary| {
        tool(
            "task_finish",
            json!({"status":"needs_input","summary":summary}),
        )
    };
    let item = match name {
        "wait" if index == 1 => tool(
            "task_wait",
            json!({"seconds":12,"reason":"等待本地恢复演练计时"}),
        ),
        "wait" => pause("计时验证已返回，等待维护者核对校验值。"),
        "unknown" => {
            assert!(
                history.contains("result_unknown")
                    && history.contains("superseded_after_interruption")
            );
            pause("需核对原工作文件的写入结果。")
        }
        "long" => {
            assert!(history.contains("保存本轮实测进度"));
            if index == 17 {
                model.long.hold().await;
            }
            let input = body["input"].as_array().unwrap();
            let ids: HashSet<_> = input
                .iter()
                .filter(|item| item["name"] == "memory_save")
                .filter_map(|item| item["call_id"].as_str())
                .collect();
            let completed: HashSet<_> = input
                .iter()
                .filter_map(|item| {
                    if item["type"] == "function_call_output"
                        && item["output"].to_string().contains("已保存记忆")
                    {
                        return item["call_id"].as_str().filter(|id| ids.contains(id));
                    }
                    let parts = item["content"].as_array()?;
                    let header = parts.first()?["text"].as_str()?;
                    (header.contains("tool: memory_save\n")
                        && parts[1..].iter().any(|part| part["text"] == "已保存记忆"))
                    .then_some(header)
                })
                .collect();
            let ordinal = completed.len() + 1;
            if ordinal <= 56 {
                tool(
                    "memory_save",
                    json!({"content":format!("isolated memory {ordinal}")}),
                )
            } else {
                pause("56轮检查点已走完，等待维护者核对校验值。")
            }
        }
        "database-outage" => {
            model.outage.hold().await;
            tool(
                "memory_save",
                json!({"content":"must-not-run-during-checkpoint-failure"}),
            )
        }
        "stream" if index == 1 => {
            tool("memory_save", json!({"content":"FORBIDDEN_PARTIAL_STREAM"}))
        }
        "transient" if index < 3 => {
            let code = if index == 1 {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::TOO_MANY_REQUESTS
            };
            return (code, Json(json!({"error":{"code":"rate_limit"}}))).into_response();
        }
        "quota" => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":{"code":"insufficient_quota"}})),
            )
                .into_response();
        }
        _ => {
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"fixture reply"}]})
        }
    };
    let event = if name == "stream" && index == 1 {
        json!({"type":"response.output_item.done","item":item})
    } else {
        json!({"type":"response.completed","response":{"output":[item]}})
    };
    (
        [("content-type", "text/event-stream")],
        format!("data: {event}\n\n"),
    )
        .into_response()
}

struct Case {
    root: PathBuf,
    db: PgPool,
    run: Uuid,
    message: Uuid,
}

impl Case {
    async fn row(&self) -> (String, i32, Value, Option<f64>, i64) {
        sqlx::query_as("SELECT status, step, loop_state, (extract(epoch from next_wake_at)*1000)::float8, control_epoch FROM runs WHERE id=$1")
            .bind(self.run).fetch_one(&self.db).await.unwrap()
    }

    async fn count(&self, query: &str) -> i64 {
        sqlx::query_scalar(query).fetch_one(&self.db).await.unwrap()
    }

    async fn pending(
        &self,
        status: &str,
        epoch: i64,
        calls: &[Value],
        receipt: (&str, &str, Value),
    ) {
        let user = json!({"role":"user","content":[{"type":"input_text","text":"你好"}]});
        let context: Vec<_> = std::iter::once(user).chain(calls.iter().cloned()).collect();
        sqlx::query(
            "UPDATE runs SET status=$2,control_epoch=$3,context=$4,pending_tools=$5 WHERE id=$1",
        )
        .bind(self.run)
        .bind(status)
        .bind(epoch)
        .bind(json!(context))
        .bind(json!(calls))
        .execute(&self.db)
        .await
        .unwrap();
        let action = Uuid::parse_str(calls[0]["local_action_id"].as_str().unwrap()).unwrap();
        sqlx::query("INSERT INTO harness_events(id,run_id,message_id,kind,title,status,result) VALUES($1,$2,$3,'tool',$4,$5,$6)")
            .bind(action).bind(self.run).bind(self.message).bind(receipt.0).bind(receipt.1)
            .bind(if receipt.2.is_null() { None } else { Some(receipt.2) }).execute(&self.db).await.unwrap();
    }
}

struct Fixture {
    root: tempfile::TempDir,
    pgdata: PathBuf,
    connection: PgConnectOptions,
    admin: PgPool,
    port: String,
    address: std::net::SocketAddr,
    model: Arc<Model>,
    assistant: PathBuf,
    pg_ctl: PathBuf,
}

impl Fixture {
    fn ctl(&self, action: &str) {
        let result = Command::new(&self.pg_ctl)
            .arg("-D")
            .arg(&self.pgdata)
            .arg("-l")
            .arg(self.pgdata.join("qualification.log"))
            .args(["-w", "-m", "fast", action])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "pg_ctl {action}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    async fn case(&self, name: &str) -> Case {
        let database = format!("aiphone_recovery_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&self.admin)
            .await
            .unwrap();
        let root = self.root.path().join(name);
        fs::create_dir_all(root.join("data/model")).unwrap();
        fs::write(root.join(".isolated-recovery-test"), "").unwrap();
        fs::write(root.join(".env"), format!("POSTGRES_DB={database}\nPOSTGRES_USER=ai_phone\nPOSTGRES_PASSWORD={ROOT_OWNER_PASSWORD}\n")).unwrap();
        let shared = self.root.path().join("credentials.json");
        let credentials = root.join("data/database/credentials.json");
        if shared.exists() {
            let mut value: Value = serde_json::from_reader(File::open(&shared).unwrap()).unwrap();
            value["database"] = json!(database);
            fs::create_dir_all(credentials.parent().unwrap()).unwrap();
            private_json(&credentials, &value);
        }
        for flag in ["--prepare-database", "--migrate"] {
            let result = ChildCommand::new(&self.assistant)
                .arg(flag)
                .current_dir(&root)
                .env("AI_PHONE_PROJECT_ROOT", &root)
                .env("AI_PHONE_PG_PORT", &self.port)
                .output()
                .await
                .unwrap();
            assert!(
                result.status.success(),
                "{flag}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if !shared.exists() {
                fs::copy(&credentials, &shared).unwrap();
            }
        }
        private_json(
            &root.join("data/model/credentials.json"),
            &json!({
                "api_key":"", "base_url":format!("http://{}/v1", self.address), "model":name, "provider":"compatible", "protocol":"responses"
            }),
        );
        let db = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(self.connection.clone().database(&database))
            .await
            .unwrap();
        let conversation: Uuid = sqlx::query_scalar("SELECT id FROM conversations LIMIT 1")
            .fetch_one(&db)
            .await
            .unwrap();
        let run = Uuid::new_v4();
        let message = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO messages(id,conversation_id,role,text) VALUES($1,$2,'assistant','')",
        )
        .bind(message)
        .bind(conversation)
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO runs(id,conversation_id,assistant_message_id,request_id,request_fingerprint,status,model_revision,context) VALUES($1,$2,$3,$1,'fixture','queued',1,$4)")
            .bind(run).bind(conversation).bind(message)
            .bind(json!([{"role":"user","content":[{"type":"input_text","text":"你好，这是一轮本地校验。"}]}])).execute(&db).await.unwrap();
        Case {
            root,
            db,
            run,
            message,
        }
    }

    fn start(&self, case: &Case) -> Worker {
        let log = case.root.join(format!("worker-{}.log", Uuid::new_v4()));
        let output = File::create(&log).unwrap();
        let child = ChildCommand::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD, "--ignored", "--nocapture"])
            .current_dir(&case.root)
            .env("AI_PHONE_RECOVERY_CHILD", "1")
            .env("AI_PHONE_PROJECT_ROOT", &case.root)
            .env("AI_PHONE_PG_PORT", &self.port)
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Worker { child, log }
    }
}

struct Worker {
    child: Child,
    log: PathBuf,
}

impl Worker {
    async fn wait<T>(&mut self, mut poll: impl AsyncFnMut() -> Option<T>) -> T {
        timeout(Duration::from_secs(55), async {
            loop {
                assert!(
                    self.child.try_wait().unwrap().is_none(),
                    "{}",
                    fs::read_to_string(&self.log).unwrap()
                );
                if let Some(value) = poll().await {
                    return value;
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "Recovery timeout: {}",
                fs::read_to_string(&self.log).unwrap()
            )
        })
    }

    async fn stop(mut self) {
        self.child.kill().await.unwrap();
    }

    fn signal(&self, name: &str) {
        assert!(
            Command::new("/bin/kill")
                .arg(name)
                .arg(self.child.id().unwrap().to_string())
                .status()
                .unwrap()
                .success()
        );
    }
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"type":"function_call","call_id":id,"local_action_id":Uuid::new_v4(),
        "name":name,"arguments":args.to_string()})
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL cluster provisioned by check-source-services.sh"]
async fn isolated_worker_recovery() {
    let pgdata = PathBuf::from(std::env::var_os("AI_PHONE_TEST_PGDATA").unwrap())
        .canonicalize()
        .unwrap();
    assert!(pgdata.join(".aiphone-recovery-fixture").is_file());
    let port: u16 = std::env::var("AI_PHONE_TEST_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    assert!(port > 1024 && port != 55482);
    let connection = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("ai_phone")
        .password(ROOT_OWNER_PASSWORD)
        .database("ai_phone");
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(connection.clone())
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SHOW data_directory")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(Path::new(&actual).canonicalize().unwrap(), pgdata);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let model = Arc::new(Model::default());
    let router = Router::new()
        .route("/v1/responses", post(respond))
        .with_state(model.clone());
    let _server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap()
    }));
    let fixture = Fixture {
        root: tempfile::tempdir().unwrap(),
        pgdata,
        connection,
        admin,
        port: port.to_string(),
        address,
        model,
        assistant: PathBuf::from(std::env::var_os("AI_PHONE_TEST_ASSISTANT_BINARY").unwrap()),
        pg_ctl: PathBuf::from(std::env::var_os("AI_PHONE_TEST_PG_CTL").unwrap()),
    };
    let model = &fixture.model;

    let case = fixture.case("wait").await;
    let cached = call(
        "cached-write",
        "workspace_write",
        json!({"path":"never-rewrite.txt","content":"WRONG_REPLAY"}),
    );
    case.pending(
        "queued",
        0,
        &[cached],
        (
            "cached",
            "completed",
            json!([{"type":"input_text","text":"{\"success\":true}"}]),
        ),
    )
    .await;
    let file = case.root.join("data/workspaces/admin/never-rewrite.txt");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "ORIGINAL_COMPLETED_RESULT").unwrap();
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    let mut worker = fixture.start(&case);
    let wake = worker.wait(async || case.row().await.3).await;
    worker.stop().await;
    sqlx::query("UPDATE runs SET control_epoch=42 WHERE id=$1")
        .bind(case.run)
        .execute(&case.db)
        .await
        .unwrap();
    let mut worker = fixture.start(&case);
    worker
        .wait(async || (case.row().await.4 == 0).then_some(()))
        .await;
    assert_eq!(case.row().await.3, Some(wake));
    assert_eq!(model.calls("wait"), 1);
    fixture.ctl("restart");
    assert_eq!(case.row().await.3, Some(wake));
    worker.signal("-STOP");
    sleep(Duration::from_secs(1)).await;
    worker.signal("-CONT");
    worker
        .wait(async || (case.row().await.0 == "paused").then_some(()))
        .await;
    worker.stop().await;
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "ORIGINAL_COMPLETED_RESULT"
    );
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    assert_eq!(
        case.count("SELECT count(*) FROM harness_events WHERE title='等待结束'")
            .await,
        1
    );
    assert_eq!(model.calls("wait"), 2);

    let case = fixture.case("unknown").await;
    let old = call(
        "unknown-write",
        "workspace_write",
        json!({"path":"forbidden.txt","content":"DO_NOT_RUN"}),
    );
    let unstarted = call(
        "unstarted-write",
        "memory_save",
        json!({"content":"DO_NOT_RUN_UNSTARTED"}),
    );
    case.pending(
        "running",
        42,
        &[old, unstarted],
        ("unknown", "running", Value::Null),
    )
    .await;
    let mut worker = fixture.start(&case);
    worker
        .wait(async || {
            let (status, _, state, _, epoch) = case.row().await;
            (status == "paused" && epoch == 0 && state["notice_kind"] == "needs_input")
                .then_some(())
        })
        .await;
    worker.stop().await;
    assert_eq!(model.calls("unknown"), 1);
    assert_eq!(case.count("SELECT count(*) FROM memories").await, 0);
    assert!(
        !case
            .root
            .join("data/workspaces/admin/forbidden.txt")
            .exists()
    );

    for (name, calls, end) in [
        ("transient", 3, "succeeded"),
        ("stream", 2, "succeeded"),
        ("quota", 1, "paused"),
    ] {
        let case = fixture.case(name).await;
        let mut worker = fixture.start(&case);
        if name == "transient" {
            let wake = worker
                .wait(async || {
                    let row = case.row().await;
                    (row.2["model_retries"] == 1).then_some(row.3).flatten()
                })
                .await;
            worker.stop().await;
            worker = fixture.start(&case);
            assert_eq!(case.row().await.3, Some(wake));
        }
        worker
            .wait(async || {
                matches!(case.row().await.0.as_str(), "succeeded" | "paused").then_some(())
            })
            .await;
        worker.stop().await;
        assert_eq!(case.row().await.0, end);
        assert_eq!(case.count("SELECT count(*) FROM memories").await, 0);
        assert_eq!(model.calls(name), calls);
    }

    let case = fixture.case("database-outage").await;
    let mut worker = fixture.start(&case);
    worker
        .wait(async || model.outage.entered.load(Ordering::Acquire).then_some(()))
        .await;
    fixture.ctl("stop");
    model.outage.released.notify_one();
    let log = worker.log.clone();
    worker
        .wait(async || {
            fs::read_to_string(&log)
                .unwrap()
                .contains("holding the checkpoint")
                .then_some(())
        })
        .await;
    fixture.ctl("start");
    worker
        .wait(async || matches!(case.row().await.0.as_str(), "failed" | "paused").then_some(()))
        .await;
    worker.stop().await;
    assert_eq!(case.count("SELECT count(*) FROM memories").await, 0);

    let case = fixture.case("long").await;
    let goal = "保存本轮实测进度";
    sqlx::query("UPDATE runs SET trigger_input='task-goal',context=$2 WHERE id=$1")
        .bind(case.run)
        .bind(json!([{"role":"user","content":[{"type":"input_text","text":goal}]}]))
        .execute(&case.db)
        .await
        .unwrap();
    sqlx::query("UPDATE proactive_state SET goal=$1,goal_run_id=$2")
        .bind(goal)
        .bind(case.run)
        .execute(&case.db)
        .await
        .unwrap();
    let mut worker = fixture.start(&case);
    worker
        .wait(async || model.long.entered.load(Ordering::Acquire).then_some(()))
        .await;
    assert_eq!(case.count("SELECT count(*) FROM memories").await, 16);
    worker.stop().await;
    model.long.released.notify_one();
    let pending = case.root.join("data/deployment/device-setup.pending");
    fs::create_dir_all(pending.parent().unwrap()).unwrap();
    fs::write(&pending, "").unwrap();
    let mut worker = fixture.start(&case);
    worker
        .wait(async || (case.row().await.0 == "queued").then_some(()))
        .await;
    sleep(Duration::from_millis(450)).await;
    assert_eq!(case.row().await.0, "queued");
    assert_eq!(model.calls("long"), 17);
    fs::remove_file(pending).unwrap();
    worker
        .wait(async || (case.row().await.0 == "paused").then_some(()))
        .await;
    worker.stop().await;
    assert!(case.row().await.1 >= 56);
    assert_eq!(case.count("SELECT count(*) FROM memories").await, 56);
    assert_eq!(
        case.count("SELECT count(DISTINCT content) FROM memories")
            .await,
        56
    );
    let run: Uuid = sqlx::query_scalar("SELECT goal_run_id FROM proactive_state")
        .fetch_one(&case.db)
        .await
        .unwrap();
    assert_eq!(run, case.run);
    println!(
        "Recovery passed: retained action, unknown receipts, provider retries, database outage, and 56 effects in resumed task"
    );
}
