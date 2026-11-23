use super::{IDLE, program::Program};
use crate::*;
use serde_json::{Value, json};
use std::time::Duration;

struct Job {
    app_id: String,
    run: Option<Uuid>,
    lease: Uuid,
}

async fn claim(db: &PgPool) -> Result<Option<Job>, sqlx::Error> {
    let lease = Uuid::now_v7();
    let query = format!("UPDATE app_mcp_jobs SET lease=$1,eligible_at=now()+interval '90 seconds'
        WHERE app_id=(SELECT app_id FROM app_mcp_jobs WHERE state='pending'
        AND eligible_at<=now() AND {IDLE} ORDER BY eligible_at,app_id LIMIT 1 FOR UPDATE SKIP LOCKED)
        RETURNING app_id, run_id");
    Ok(sqlx::query_as::<_, (String, Option<Uuid>)>(&query)
        .bind(lease)
        .fetch_optional(db)
        .await?
        .map(|(app_id, run)| Job { app_id, run, lease }))
}

fn author_request(app_id: &str, resource_ids: &[String]) -> Value {
    let string = json!({"type":"string"});
    let tool = crate::tool_catalog::function(
        "write_app_tool",
        "Write one reusable data tool for this application, or skip it.",
        json!({"skip":{"type":"boolean"},"name":string,"description":string,
            "input_type":{"type":"string","enum":["string","object","array","integer","number","boolean"]},
            "input_description":string,"uses_view":{"type":"boolean"},"source":string,
            "examples":{"type":"array","items":{"type":"object","properties":{"input_json":string,"expected_json":string,"view_json":string},
                "required":["input_json","expected_json"],"additionalProperties":false}}}),
        vec!["skip"],
    );
    json!({"input":[
        {"role":"system","content":[{"type":"input_text","text":"Write a useful MCP tool for this application in pure Rhai 1.x. The constant input holds its argument. The last expression is the result. If observed resource IDs support a useful lookup, prefer it: set uses_view=true and use view.elements (id, text, resource_id, clickable, scrollable, editable, bounds) to return matching real controls. Include a synthetic view_json per example. Otherwise use input alone for an app-specific data or document operation. Use a short snake_case name, one-line description and two distinct JSON-encoded input/output examples. Skip when there is no useful supported operation. Do not invent app APIs or report actions."}]},
        {"role":"user","content":[{"type":"input_text","text":json!({"application":app_id,"observed_resource_ids":resource_ids}).to_string()}]}
    ],"tools":[tool],"tool_choice":{"type":"function","name":"write_app_tool"},"parallel_tool_calls":false,
       "reasoning":{"effort":"none"},"max_output_tokens":1800})
}

async fn author(app: &App, job: &Job) -> Result<Option<Program>, String> {
    // Use machine identifiers only. Screen text, window titles and chat are not sent to the author.
    let parts: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_path_query_array(e.result,'$[*] ? (@.type == \"input_text\").text')
         FROM app_learning_observations o JOIN harness_events e ON e.id=o.event_id
         WHERE o.run_id=$1 AND o.app_id=$2 ORDER BY o.observed_at DESC LIMIT 1",
    )
    .bind(job.run)
    .bind(&job.app_id)
    .fetch_optional(&app.db)
    .await
    .map_err(|e| e.to_string())?;
    let mut ids = std::collections::BTreeSet::new();
    for text in parts
        .as_ref()
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if let Ok(frame) = serde_json::from_str::<Value>(text) {
            for node in frame["elements"].as_array().into_iter().flatten() {
                if let Some(id) = node["resource_id"].as_str().filter(|s| {
                    !s.is_empty()
                        && s.len() <= 160
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"._-:/".contains(&c))
                }) {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    let config = match job.run {
        Some(run) => crate::settings::for_run(app, run).await?,
        None => {
            let revision: i64 = sqlx::query_scalar("SELECT m.model_revision FROM registry_meta w
                JOIN agents a ON a.id=w.active_agent_id JOIN model_connections m ON m.id=a.model_connection_id
                WHERE w.owner_id='local-user' AND a.retired_at IS NULL AND m.retired_at IS NULL")
                .fetch_one(&app.db).await.map_err(|e|e.to_string())?;
            app.settings.get(revision as u64)?
        }
    };
    let client = app.settings.http_client(&config.proxy_url)?;
    let output = crate::model_adapter::request_tracked(
        &config,
        &client,
        author_request(&job.app_id, &ids.into_iter().take(32).collect::<Vec<_>>()),
        &CancellationToken::new(),
        &mut crate::model_timing::Timing::default(),
    )
    .await?;
    let call = output
        .iter()
        .find(|v| v["type"] == "function_call" && v["name"] == "write_app_tool")
        .and_then(|v| v["arguments"].as_str())
        .ok_or("missing app tool source")?;
    let args: Value = serde_json::from_str(call).map_err(|e| e.to_string())?;
    if args["skip"] == true {
        return Ok(None);
    }
    let program: Program = serde_json::from_value(args).map_err(|e| e.to_string())?;
    program.verify()?;
    Ok(Some(program))
}

async fn publish(app: &App, job: &Job, program: &Program) -> Result<(), String> {
    let mut tx = app.db.begin().await.map_err(|e| e.to_string())?;
    let id = Uuid::now_v7().to_string();
    let name = format!("mcp-{}-{}", job.app_id.replace(':', "-"), program.name);
    let content = program.document(&name)?;
    sqlx::query("INSERT INTO runtime_skills(id,name,description,source,content) VALUES($1,$2,$3,'agent-mcp',$4)")
        .bind(&id).bind(&name).bind(&program.description).bind(content).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    let saved = sqlx::query(
        "UPDATE app_mcp_jobs SET state='ready',skill_id=$3,lease=NULL WHERE app_id=$1 AND lease=$2",
    )
    .bind(&job.app_id)
    .bind(job.lease)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if saved.rows_affected() == 1 {
        crate::workbench::commit_skill(tx, &app.db, &app.root, &id).await?;
    }
    Ok(())
}

async fn run_job(
    app: &App,
    job: &Job,
    activity: &mut tokio::sync::watch::Receiver<()>,
) -> Result<(), String> {
    let result = tokio::select! {
        biased;
        _=activity.changed()=>None,
        made=tokio::time::timeout(Duration::from_secs(55),author(app,job))=>
            Some(made.map_err(|_|"model timed out".to_owned()).and_then(|result|result))
    };
    let query = match result {
        Some(Ok(Some(program))) => return publish(app, job, &program).await,
        Some(Ok(None)) => {
            "UPDATE app_mcp_jobs SET state='skipped',lease=NULL WHERE app_id=$1 AND lease=$2"
        }
        Some(Err(error)) => {
            // Keep diagnostics out of the shared conversation.
            eprintln!("Background MCP authoring for {}: {error}", job.app_id);
            "UPDATE app_mcp_jobs SET attempts=attempts+1,state=CASE WHEN attempts>=1 THEN 'failed' ELSE 'pending' END,
             eligible_at=now()+interval '30 minutes',lease=NULL WHERE app_id=$1 AND lease=$2"
        }
        None => "UPDATE app_mcp_jobs SET eligible_at=now(),lease=NULL WHERE app_id=$1 AND lease=$2",
    };
    sqlx::query(query)
        .bind(&job.app_id)
        .bind(job.lease)
        .execute(&app.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn work(app: Shared) {
    let mut activity = app.run_signals.app_tools_activity.subscribe();
    let mut scan_at = tokio::time::Instant::now();
    loop {
        activity.borrow_and_update();
        if tokio::time::Instant::now() >= scan_at {
            scan_at = tokio::time::Instant::now() + Duration::from_secs(60);
            if let Err(error) = super::inventory::scan(&app).await {
                eprintln!("Background app inventory: {error}");
            }
        }
        if app.settings.current() > 0
            && let Ok(Some(job)) = claim(&app.db).await
        {
            if let Err(error) = run_job(&app, &job, &mut activity).await {
                eprintln!("Background app tool: {error}");
            }
            continue;
        }
        tokio::select! {_=tokio::time::sleep_until(scan_at)=>{},_=activity.changed()=>{}}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::steering_regressions::database_case;
    use axum::extract::State;
    #[tokio::test]
    #[ignore = "requires an isolated PostgreSQL database"]
    async fn incoming_message_preempts_background_author_and_only_foreground_is_visible() {
        database_case(|app| async move {
            let (started,mut waiting)=tokio::sync::mpsc::channel(1);
            let server=Router::new().route("/responses",post(move || {
                let started=started.clone();
                async move {started.send(()).await.unwrap(); std::future::pending::<String>().await}
            }));
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address=listener.local_addr().unwrap();
            let server=tokio::spawn(async move {axum::serve(listener,server).await.unwrap()});
            app.settings.test_revision(crate::settings::ModelConfig{api_key:String::new(),
                base_url:format!("http://{address}"),model:"app-author-test".into(),provider:"compatible".into(),
                protocol:"responses".into(),proxy_url:String::new(),verified_at_ms:1});
            crate::registry::initialize(&app).await.unwrap();
            let installed=vec!["org.example.reader".to_owned()];
            super::super::inventory::record(&app.db,crate::registry::PHONE,&[]).await.unwrap();
            super::super::inventory::record(&app.db,crate::registry::PHONE,&installed).await.unwrap();
            let job=claim(&app.db).await.unwrap().unwrap();
            assert!(claim(&app.db).await.unwrap().is_none());
            let mut activity=app.run_signals.app_tools_activity.subscribe();
            let author={let app=app.clone();tokio::spawn(async move {run_job(&app,&job,&mut activity).await})};
            tokio::time::timeout(Duration::from_secs(4),waiting.recv()).await.unwrap().unwrap();
            let visible=||sqlx::query_as::<_,(i64,i64,i64)>("SELECT (SELECT count(*) FROM messages),(SELECT count(*) FROM harness_todos),(SELECT count(*) FROM harness_events)");
            assert_eq!(visible().fetch_one(&app.db).await.unwrap(),(0,0,0));
            let mut headers=HeaderMap::new();
            headers.insert("authorization",format!("Bearer {}",app.token).parse().unwrap());
            headers.insert("content-type",MEDIA.parse().unwrap());
            let input=wire::ChatSendRequest{request_id:Uuid::now_v7().to_string(),text:"foreground input".into(),device_id:crate::registry::PHONE.into(),..Default::default()};
            let started=std::time::Instant::now();
            assert_eq!(crate::http::send(State(app.clone()),headers,Ok(Bytes::from(input.encode_to_vec()))).await.status(),StatusCode::ACCEPTED);
            tokio::time::timeout(Duration::from_secs(1),author).await.unwrap().unwrap().unwrap();
            eprintln!("message admission through background release: {:?}", started.elapsed());
            assert_eq!(sqlx::query_as::<_,(String,i16,bool)>("SELECT state,attempts,lease IS NULL FROM app_mcp_jobs")
                .fetch_one(&app.db).await.unwrap(),("pending".into(),0,true));
            assert!(claim(&app.db).await.unwrap().is_none());
            assert_eq!(visible().fetch_one(&app.db).await.unwrap().1,1);
            sqlx::query("UPDATE runs SET status='canceled' WHERE status='queued'").execute(&app.db).await.unwrap();
            assert_eq!(claim(&app.db).await.unwrap().unwrap().app_id,installed[0]);
            server.abort();
            let _=server.await;
        }).await;
    }
}
