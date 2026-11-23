use crate::*;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::{HashMap, HashSet};

pub struct AuditResult {
    pub approved: bool,
    pub feedback: String,
    experience: Option<Value>,
    knowledge: Option<Value>,
}

type LearningEvidence = std::collections::BTreeMap<String, crate::app_learning::Metadata>;

fn learning_schema(evidence: &LearningEvidence) -> Option<Value> {
    if evidence.is_empty() {
        return None;
    }
    let mut schema = crate::tool_catalog::experience_schema();
    let properties = schema["properties"]
        .as_object_mut()
        .expect("experience schema");
    let mut references = properties
        .remove("observation_ids")
        .expect("experience references");
    references["items"]["enum"] = json!(evidence.keys().collect::<Vec<_>>());
    properties.insert("evidence_ids".into(), references);
    properties.insert(
        "app_id".into(),
        json!({"type":"string","enum":evidence.values().map(|m| m.app_id.as_str()).collect::<std::collections::BTreeSet<_>>()}),
    );
    for field in schema["required"].as_array_mut().unwrap() {
        if field == "observation_ids" {
            *field = json!("evidence_ids");
        }
    }
    Some(schema)
}

fn learning_candidate(candidate: &Value, evidence: &LearningEvidence) -> Result<Value, String> {
    let app_id = candidate["app_id"].as_str().ok_or("经验缺少应用标识")?;
    let ids: Vec<String> = serde_json::from_value(candidate["evidence_ids"].clone())
        .map_err(|_| "经验需要本轮核验事件ID")?;
    let mut observations = Vec::with_capacity(ids.len());
    for id in ids {
        let actual = evidence.get(&id).ok_or("经验引用了本轮核验以外的事件")?;
        if actual.app_id != app_id {
            return Err("经验引用的事件不属于该应用".into());
        }
        observations.push(actual.observation_id.clone());
    }
    let mut value = candidate.clone();
    let fields = value.as_object_mut().ok_or("经验格式无效")?;
    fields.remove("evidence_ids");
    fields.insert("observation_ids".into(), json!(observations));
    Ok(value)
}

fn knowledge_schema(evidence: &LearningEvidence, illustrated: &[String]) -> Option<Value> {
    let app = &evidence.values().next()?.app_id;
    if !evidence.values().any(|item| &item.app_id != app) {
        return None;
    }
    let mut schema = json!({"type":"object","additionalProperties":false,
        "properties":{
            "title":{"type":"string","maxLength":100},
            "summary":{"type":"string","maxLength":1200},
            "evidence_ids":{"type":"array","minItems":2,"maxItems":8,"items":{"type":"string","enum":evidence.keys().collect::<Vec<_>>()}}
        },"required":["title","summary","evidence_ids"]});
    if !illustrated.is_empty() {
        schema["properties"]["image_evidence_id"] = json!({"type":"string","enum":illustrated});
    }
    Some(schema)
}

struct KnowledgeCandidate<'a> {
    title: &'a str,
    summary: &'a str,
    observations: Vec<String>,
    image: Option<&'a str>,
}

fn knowledge_candidate<'a>(
    candidate: &'a Value,
    evidence: &'a LearningEvidence,
    illustrated: &[String],
) -> Option<KnowledgeCandidate<'a>> {
    let title = candidate["title"].as_str()?.trim();
    let summary = candidate["summary"].as_str()?.trim();
    let ids = candidate["evidence_ids"].as_array()?;
    if title.is_empty()
        || title.chars().count() > 100
        || summary.is_empty()
        || summary.chars().count() > 1200
        || !(2..=8).contains(&ids.len())
    {
        return None;
    }
    let mut apps = std::collections::BTreeSet::new();
    let mut observations = std::collections::BTreeSet::new();
    for id in ids {
        let item = evidence.get(id.as_str()?)?;
        apps.insert(&item.app_id);
        observations.insert(item.observation_id.clone());
    }
    let image = candidate["image_evidence_id"]
        .as_str()
        .filter(|id| {
            illustrated.iter().any(|known| known == id)
                && ids.iter().any(|value| value.as_str() == Some(*id))
        })
        .and_then(|id| evidence.get(id))
        .map(|frame| frame.observation_id.as_str());
    (apps.len() >= 2 && observations.len() == ids.len()).then(|| KnowledgeCandidate {
        title,
        summary,
        observations: observations.into_iter().collect(),
        image,
    })
}

fn single_desktop_input(evidence: &[Value]) -> bool {
    let mut inputs = 0;
    for receipt in evidence {
        match receipt["tool"].as_str() {
            Some("computer_type") if receipt["status"] == "completed" => {
                let observed = crate::worker::tool_value(&receipt["result"]).is_some_and(|value| {
                    value["success"] == true && value["observation_id"].is_string()
                });
                if !observed {
                    return false;
                }
                inputs += 1;
            }
            Some(
                "skill_read"
                | "computer_observe"
                | "computer_zoom"
                | "computer_find_text"
                | "computer_find_control"
                | "task_plan",
            ) => {}
            _ => return false,
        }
    }
    inputs == 1
}

struct PhoneInputAudit {
    source: Value,
    tap: usize,
    after: usize,
}

fn single_cued_input(
    evidence: &[Value],
    calls: &HashMap<Uuid, &Value>,
    pngs: &HashMap<Uuid, usize>,
) -> Option<PhoneInputAudit> {
    if evidence
        .iter()
        .any(|receipt| receipt["status"] != "completed")
    {
        return None;
    }
    let mut device_rows = evidence.iter().enumerate().filter(|(_, receipt)| {
        !matches!(receipt["tool"].as_str(), Some("skill_read" | "task_plan"))
    });
    let (_, before) = device_rows.next()?;
    let (tap, action) = device_rows.next()?;
    let (after, post) = device_rows.next()?;
    if device_rows.next().is_some()
        || before["tool"] != "phone_observe"
        || !matches!(action["tool"].as_str(), Some("phone_tap" | "phone_swipe"))
        || post["tool"] != "phone_observe"
    {
        return None;
    }

    let result = crate::worker::tool_value(&action["result"])?;
    if result["success"] != true
        || result["not_executed"] == true
        || result["result_unknown"] == true
    {
        return None;
    }
    let action_event = Uuid::parse_str(action["evidence_id"].as_str()?).ok()?;
    let post_event = Uuid::parse_str(post["evidence_id"].as_str()?).ok()?;
    let action_call = calls.get(&action_event)?;
    let action_id = action_call["call_id"]
        .as_str()
        .filter(|id| !id.is_empty())?;
    if !pngs.contains_key(&post_event) || calls.get(&post_event)?["local_after_action"] != action_id
    {
        return None;
    }

    let arguments: Value = serde_json::from_str(action_call["arguments"].as_str()?).ok()?;
    let source = crate::worker::tool_value(&before["result"])?;
    let destination = crate::worker::tool_value(&post["result"])?;
    let source_app = source["foreground_app"].as_str()?;
    if crate::application_identity::phone(source_app)?
        != crate::application_identity::phone(destination["foreground_app"].as_str()?)?
    {
        return None;
    }
    let device = source["device_id"].as_str().filter(|id| !id.is_empty())?;
    let source_observation = source["observation_id"]
        .as_str()
        .filter(|id| !id.is_empty())?;
    let target_observation = destination["observation_id"]
        .as_str()
        .filter(|id| !id.is_empty())?;
    if destination["device_id"] != device || source_observation == target_observation {
        return None;
    }
    let readback = calls.get(&post_event)?["local_readback"] == true;
    let cue = &result["cue"];
    if !readback || cue["matched"] != true || cue["source_observation_id"] != source_observation {
        return None;
    }
    let target = json!({"condition":arguments["when"],"matched_at_ms":cue["confirmed_at_ms"],"motion":cue["motion"]});
    let source = json!({"device_id":device,"observation_id":source_observation,"foreground_app":source_app,"target":target});
    Some(PhoneInputAudit {
        source: json!({"evidence_id":before["evidence_id"],"tool":"phone_observe","status":"completed",
            "result":[{"type":"input_text","text":source.to_string()}]}),
        tap,
        after,
    })
}

// A source image can be deferred only when the persisted control under the tap,
// the input receipt and the immediately following read all identify one chain.
struct PhoneFrames {
    device: String,
    images: Vec<(Uuid, usize)>,
    shown: HashSet<Uuid>,
}

impl PhoneFrames {
    fn navigation(
        evidence: &[Value],
        calls: &HashMap<Uuid, &Value>,
        pngs: &HashMap<Uuid, usize>,
    ) -> Option<Self> {
        let mut rows = evidence.iter().filter(|row| row["tool"] != "skill_read");
        if evidence.iter().any(|row| row["status"] != "completed") {
            return None;
        }
        let event_id = |row: &Value| Uuid::parse_str(row["evidence_id"].as_str()?).ok();
        let mut row = rows.next()?;
        let mut seen = HashSet::new();
        let mut images = Vec::new();
        let mut device = None;
        let mut app_id = None;
        loop {
            let event = event_id(row)?;
            let frame = crate::worker::tool_value(&row["result"])?;
            let observation = frame["observation_id"]
                .as_str()
                .filter(|id| !id.is_empty())?;
            let phone = frame["device_id"].as_str().filter(|id| !id.is_empty())?;
            let package = crate::application_identity::phone(frame["foreground_app"].as_str()?)?;
            if row["tool"] != "phone_observe"
                || device.get_or_insert_with(|| phone.to_owned()) != phone
                || app_id.get_or_insert_with(|| package.to_owned()) != package
                || !seen.insert(observation.to_owned())
            {
                return None;
            }
            images.push((event, *pngs.get(&event)?));
            let Some(action) = rows.next() else {
                if images.len() < 2 || calls.get(&event)?["local_readback"] != true {
                    return None;
                }
                return Some(Self {
                    device: device?,
                    images,
                    shown: HashSet::from([event]),
                });
            };
            let action_call = calls.get(&event_id(action)?)?;
            let args: Value = serde_json::from_str(action_call["arguments"].as_str()?).ok()?;
            let result = crate::worker::tool_value(&action["result"])?;
            let target = args["element_id"].as_str().filter(|id| !id.is_empty())?;
            let call_id = action_call["call_id"]
                .as_str()
                .filter(|id| !id.is_empty())?;
            row = rows.next()?;
            if action["tool"] != "phone_tap_element"
                || result["success"] != true
                || result["result_unknown"] == true
                || result["not_executed"] == true
                || result["error"].is_string()
                || args["observation_id"] != observation
                || calls.get(&event_id(row)?)?["local_after_action"] != call_id
                || !frame["elements"].as_array()?.iter().any(|c| {
                    c["id"] == target
                        && c["source"] == "native"
                        && c["clickable"] == true
                        && c["text"]
                            .as_str()
                            .is_some_and(|text| !text.trim().is_empty())
                })
            {
                return None;
            }
        }
    }

    fn request(&self, template: &Value, visuals: &[Value]) -> Value {
        let mut body = template.clone();
        let content = body["input"][1]["content"].as_array_mut().unwrap();
        let mut selected = Vec::new();
        let mut available = Vec::new();
        for (id, i) in &self.images {
            if self.shown.contains(id) {
                selected.extend_from_slice(&visuals[*i..*i + 2]);
            } else {
                available.push(id);
            }
        }
        append_visuals(content, selected);
        if !available.is_empty() {
            body["tools"].as_array_mut().unwrap().push(json!({"type":"function","name":"audit_images",
                "description":"原生记录和已附原图不足以核实用户要求时，一次选择所需的其余原图。前后外观、颜色、图形等视觉比较须取相应时刻的原图。",
                "parameters":{"type":"object","additionalProperties":false,"properties":{
                    "evidence_ids":{"type":"array","minItems":1,"maxItems":available.len(),"items":{"type":"string","enum":available}}
                },"required":["evidence_ids"]}}));
            body["tool_choice"] = json!("required");
        }
        body
    }

    fn reveal(&mut self, args: &str) -> Result<Vec<Uuid>, String> {
        let value: Value = serde_json::from_str(args).map_err(|_| "截图回看参数无效")?;
        let ids: Vec<Uuid> = serde_json::from_value(value["evidence_ids"].clone())
            .map_err(|_| "截图回看需要本轮证据 ID")?;
        if ids.is_empty()
            || ids.len() > self.images.len()
            || ids.iter().any(|id| {
                !self.images.iter().any(|(event, _)| event == id) || self.shown.contains(id)
            })
        {
            return Err("截图回看只接受本轮尚未发送的原图 ID".into());
        }
        self.shown.extend(ids.iter().copied());
        Ok(ids)
    }
}

pub(crate) struct Proposal<'a> {
    pub summary: &'a str,
    pub needs_input: bool,
    pub attachments: &'a [crate::files::PreparedOutput],
}

// A completion audit has no execution tools. Its input comes from this run's
// persisted tool ledger, never from historical assistant prose.
pub async fn verify(
    app: &App,
    run: Uuid,
    message: Uuid,
    context: &[Value],
    proposal: Proposal<'_>,
    cancel: &CancellationToken,
) -> Result<AuditResult, String> {
    let Proposal {
        summary,
        needs_input,
        attachments,
    } = proposal;
    let mut timing = crate::model_timing::Timing::default();
    let (latest_input, request_text) = crate::worker::audit_request(context);
    let current_calls: HashMap<Uuid, &Value> = context
        .iter()
        .skip(latest_input + 1)
        .filter(|item| item["type"] == "function_call")
        .filter_map(|item| {
            Some((
                Uuid::parse_str(item["local_action_id"].as_str()?).ok()?,
                item,
            ))
        })
        .collect();
    let ids: Vec<_> = current_calls.keys().copied().collect();
    let rows = sqlx::query("SELECT e.id,e.title,e.status,e.result,r.origin='user' AS user_run FROM harness_events e JOIN runs r ON r.id=e.run_id WHERE e.run_id=$1 AND e.id=ANY($2) AND e.kind='tool' AND e.result IS NOT NULL ORDER BY e.created_at DESC,e.id DESC LIMIT 64")
        .bind(run).bind(&ids).fetch_all(&app.db).await.map_err(|e| e.to_string())?;
    if !needs_input
        && !rows.iter().any(|row| {
            row.get::<&str, _>("status") == "completed"
                && current_calls[&row.get::<Uuid, _>("id")]["name"] != "skill_read"
        })
    {
        return Err("本轮没有已确认的执行回执，无法核验完成。".into());
    }
    let mut image_budget = 8usize;
    let mut visual_inputs = Vec::new();
    let mut input_visuals = None;
    let mut pngs = HashMap::new();
    let mut evidence = Vec::new();
    let mut learning_evidence = LearningEvidence::new();
    let mut illustrated = Vec::new();
    let host_session = app.run_signals.host.session(run);
    let complete_ledger = rows.len() < 64
        && current_calls
            .values()
            .filter(|call| call["name"] != "task_finish")
            .count()
            == rows.len();
    let user_run = rows
        .first()
        .is_some_and(|row| row.get::<bool, _>("user_run"));
    for row in rows {
        let row_id = row.get::<Uuid, _>("id");
        let id = row_id.to_string();
        let call = current_calls[&row_id];
        let mut result = row.get::<Option<Value>, _>("result").unwrap_or(Value::Null);
        let name = call["name"].as_str().unwrap_or("");
        if name == "skill_read"
            && let Some(text) = result[0]["text"].as_str()
        {
            result = json!({"excerpt":text.chars().take(512).collect::<String>()});
        }
        let observation = (row.get::<String, _>("status") == "completed"
            && crate::app_learning::observation_tool(name))
        .then(|| crate::app_learning::metadata(&result))
        .flatten();
        if let Some(observation) = observation {
            learning_evidence.insert(id.clone(), observation);
        }
        if (name == "host_desktop"
            && !crate::host_desktop::receipt_in_session(&result, host_session.as_deref()))
            || (name == "device_interact" && !app.run_signals.peers.contains_output(&result))
        {
            limit_result(&mut result, &mut 0);
        } else {
            limit_result(&mut result, &mut image_budget);
        }
        let first_visual = visual_inputs.len();
        extract_images(&mut result, &id, &mut visual_inputs);
        if first_visual < visual_inputs.len() && learning_evidence.contains_key(&id) {
            illustrated.push(id.clone());
        }
        if name == "computer_type" && first_visual < visual_inputs.len() {
            input_visuals = Some(first_visual..visual_inputs.len());
        }
        if name == "phone_observe"
            && visual_inputs.len() == first_visual + 2
            && visual_inputs[first_visual + 1]["image_url"]
                .as_str()
                .and_then(|url| url.strip_prefix("data:image/png;base64,"))
                .is_some_and(|png| !png.is_empty())
        {
            pngs.insert(row_id, first_visual);
        }
        evidence.push(json!({"evidence_id":id,"tool":name,"learning_app":learning_evidence.get(&id).map(|m|m.app_id.as_str()),
            "input":call["arguments"].as_str().unwrap_or(""),
            "title":row.get::<String,_>("title"),"status":row.get::<String,_>("status"),"result":result}));
    }
    evidence.reverse();
    let (handoff_receipts, notifications, learning) = tokio::try_join!(
        sqlx::query_scalar::<_, Value>("SELECT e.result FROM harness_events e JOIN runs r ON r.id=e.run_id WHERE e.run_id=$1 AND e.kind='summary' AND e.title='设备协作' AND e.status='completed' AND e.result IS NOT NULL AND e.created_at>=COALESCE(r.input_at,r.created_at) ORDER BY e.created_at")
            .bind(run).fetch_all(&app.db),
        sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('device_id',n.device_id,'app',n.app_name,'title',n.title,'text',n.body) FROM phone_notifications n JOIN runs r ON r.id=n.run_id WHERE n.run_id=$1 AND NOT r.background_read_only AND n.status IN ('assigned','attending','pending-summary') ORDER BY n.received_at LIMIT 16")
            .bind(run).fetch_all(&app.db),
        sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('app',app_id,'title',title,'procedure',procedure,'outcome',outcome,'limitations',limitations,'observations',observation_ids) FROM app_experiences WHERE run_id=$1 AND superseded_at IS NULL ORDER BY created_at LIMIT 8")
            .bind(run).fetch_all(&app.db),
    ).map_err(|e| e.to_string())?;
    let compact_allowed = !needs_input
        && handoff_receipts.is_empty()
        && notifications.is_empty()
        && learning.is_empty()
        && complete_ledger;
    let readback = context
        .last()
        .is_some_and(|call| call["local_readback"] == true);
    let mut frames = (compact_allowed && user_run && readback)
        .then(|| PhoneFrames::navigation(&evidence, &current_calls, &pngs))
        .flatten();
    let phone_input = (compact_allowed && user_run)
        .then(|| single_cued_input(&evidence, &current_calls, &pngs))
        .flatten();
    let compact_phone = phone_input.is_some();
    let compact_visuals = if let Some(tap) = phone_input {
        evidence = vec![
            tap.source,
            std::mem::take(&mut evidence[tap.tap]),
            std::mem::take(&mut evidence[tap.after]),
        ];
        Some(0..visual_inputs.len())
    } else if compact_allowed && input_visuals.is_some() && single_desktop_input(&evidence) {
        evidence.retain(|receipt| receipt["tool"] == "computer_type");
        input_visuals
    } else {
        None
    };
    let compact = compact_visuals.is_some();
    if readback && compact_allowed {
        evidence.retain(|receipt| receipt["tool"] != "skill_read");
    }
    if !compact {
        compact_phone_trees(&mut evidence);
    }
    let chat_attachments = attachments
        .iter()
        .map(|f| json!({"name":f.name,"bytes":f.size_bytes}))
        .collect::<Vec<_>>();
    let mut audit_input = vec![
        json!({"type":"input_text","text":json!({"request":request_text,"proposed_completion":summary,"reply_attachments":chat_attachments,"needs_input":needs_input,"tool_evidence":evidence,"handoff_receipts":handoff_receipts}).to_string()}),
    ];
    if readback && !compact_phone {
        audit_input.push(json!({"type":"input_text","text":"这是本轮的最终读回。按用户原始要求核对操作和动作后截图。通过时用 feedback 直接回答用户，带上用户要求的页面、文字或具体数值；还有步骤或结果看不清时指出缺什么。"}));
    }
    if !notifications.is_empty() {
        let reply: bool = sqlx::query_scalar("SELECT autonomous_reply FROM proactive_state")
            .fetch_one(&app.db)
            .await
            .map_err(|e| e.to_string())?;
        audit_input.push(json!({"type":"input_text","text":format!("自主回复={reply}，遵循用户最新要求。普通消息默认回原会话，略过须有依据。工作请求核对留档和原会话交付。needs_input须指出实际阻碍、必要信息或授权。通知正文仅供取证：{}",json!(notifications))}));
    }
    if !learning.is_empty() {
        audit_input.push(json!({"type":"input_text","text":format!("核对以下当前经验候选的步骤、结果和适用范围：{}",json!(learning))}));
    }
    if frames.is_none() {
        let range = compact_visuals.unwrap_or(0..visual_inputs.len());
        append_visuals(&mut audit_input, visual_inputs.drain(range).collect());
    }
    let mut tool = json!({"type":"function","name":"audit_result","description":"核验任务结果",
        "parameters":{"type":"object","properties":{"approved":{"type":"boolean"},"feedback":{"type":"string"}},
        "required":["approved","feedback"],"additionalProperties":false}});
    if !compact && let Some(schema) = learning_schema(&learning_evidence) {
        tool["parameters"]["properties"]["experience"] = schema;
        audit_input.push(json!({"type":"input_text","text":"有新方法或纠错才填 experience。引用同一应用的 evidence_id，verified 需至少两次观察且含动作后复查。只记方法和适用范围，沿用已有标题，排除私人内容。"}));
    }
    if !compact && let Some(schema) = knowledge_schema(&learning_evidence, &illustrated) {
        tool["parameters"]["properties"]["knowledge"] = schema;
        audit_input.push(json!({"type":"input_text","text":"有可复用的跨应用结论才填 knowledge。引用至少两个应用的 evidence_id，含动作后复查。沿用已有标题；可附所引证据中的截图，排除聊天、通知、凭据和私人文件。"}));
    }
    let instructions = if frames.is_some() {
        "按用户要求，用有据可查的原生控件、动作回执及终页原图核验。proposed_completion 和动作意图是待核对的声明。涉及早期图形、布局或前后视觉比较，或原生记录不足时，用 audit_images 选择所需原图；已有证据充分就用 audit_result 直接回答。"
    } else if compact_phone {
        "核对用户要求、目标、手势回执和动作后的实屏。通过时用 feedback 直接给出所求结果，含所问数值或页面名。不符或看不清时指出缺失。"
    } else if compact {
        "核对电脑输入回执与动作后画面。确认目标窗口和实际写入内容均符合用户要求才通过。用一句话指出证据或缺失。"
    } else {
        "只按本轮工具回执、截图和用户目标核验。动作、数量、界面事实及交付均有证据才通过；缺证时直接指出下一步。当前对话将随回复发布的文件以 reply_attachments 为准，空列表表示没有附件；其他应用的发送核对原会话回执。通知预览、发送记录与对方确认分别核对。经验的步骤和适用范围由操作前后证据支持。截图归档后缺证就重新观察。反馈只写可核对的结论，不含凭据。"
    };
    let mut body = json!({"input":[
        {"role":"system","content":[{"type":"input_text","text":instructions}]},
        {"role":"user"}
    ],"tools":[tool],"tool_choice":{"type":"function","name":"audit_result"},"parallel_tool_calls":false,
        "reasoning":{"effort":"none"},"max_output_tokens":if compact {192} else {1200},"stream":false,"store":false});
    body["input"][1]["content"] = Value::Array(audit_input);

    enum Turn {
        Result(AuditResult),
        Images(Vec<Uuid>),
    }
    let (event, result) = loop {
        let sent_images = frames.as_ref().map(|f| {
            f.images
                .iter()
                .filter_map(|(id, _)| f.shown.contains(id).then_some(*id))
                .collect::<Vec<_>>()
        });
        let event = Uuid::now_v7();
        sqlx::query("INSERT INTO harness_events(id,run_id,message_id,kind,title,content) VALUES($1,$2,$3,'audit','结果核验',$4)")
            .bind(event).bind(run).bind(message)
            .bind(if frames.as_ref().is_some_and(|f| f.shown.len() > 1) { "按所选证据回看原始截图" } else { "核对本轮工具记录与完成声明" })
            .execute(&app.db).await.map_err(|e| e.to_string())?;
        let audit = async {
            let config = crate::settings::for_run(app, run).await?;
            let client = app.settings.http_client(&config.proxy_url)?;
            let request = if let Some(frames) = &frames {
                if crate::phone_system::for_run(app, run).await? != frames.device {
                    return Err(crate::phone_system::PHONE_BINDING_REVOKED.into());
                }
                let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id=$1 AND assistant_message_id=$2 AND status='running' AND jsonb_array_length(steering)=0)")
                    .bind(run).bind(message).fetch_one(&app.db).await.map_err(|e| e.to_string())?;
                if !active {
                    return Err("canceled".into());
                }
                frames.request(&body, &visual_inputs)
            } else {
                std::mem::take(&mut body)
            };
            let output = tokio::time::timeout(
                Duration::from_secs(60),
                crate::model_adapter::request_tracked(
                    &config,
                    &client,
                    request,
                    cancel,
                    &mut timing,
                ),
            )
            .await
            .map_err(|_| "结果核验超时；尚未确认完成")??;
            let output = json!({"output":output});
            let call = audit_call(&output)?;
            if call["name"] == "audit_images" {
                let frames = frames.as_mut().ok_or("本轮没有可回看的截图")?;
                Ok(Turn::Images(frames.reveal(
                    call["arguments"].as_str().ok_or("截图回看参数无效")?,
                )?))
            } else {
                parse_result(&output).map(Turn::Result)
            }
        };
        let turn = tokio::select! {biased; _ = cancel.cancelled() => Err("canceled".into()), turn = audit => turn};
        let mut metrics = timing.snapshot();
        metrics["compact"] = json!(compact);
        if metrics["request_bytes"].as_u64().is_some_and(|n| n > 0)
            && let Some(ids) = sent_images
        {
            metrics["sent_image_evidence_ids"] = json!(ids);
        }
        let (status, content) = match &turn {
            Ok(Turn::Result(report)) => (
                if report.approved {
                    "completed"
                } else {
                    "failed"
                },
                report.feedback.as_str(),
            ),
            Ok(Turn::Images(ids)) => {
                metrics["requested_evidence_ids"] = json!(ids);
                ("succeeded", "已选择原始截图，继续核验")
            }
            Err(error) => (
                if error == "canceled" {
                    "canceled"
                } else {
                    "failed"
                },
                error.as_str(),
            ),
        };
        if !compact
            && let Ok(Turn::Result(report)) = &turn
            && let Some(candidate) = report.experience.as_ref().filter(|_| !needs_input)
        {
            match learning_candidate(candidate, &learning_evidence) {
                Ok(candidate) => {
                    retain_learning(app, run, false, Some(&candidate)).await;
                }
                Err(error) => eprintln!(
                    "Optional app learning was not retained; user task result preserved: {error}"
                ),
            }
        }
        sqlx::query("UPDATE harness_events SET status=$2,content=$3,result=$4,finished_at=now() WHERE id=$1")
            .bind(event).bind(status).bind(content).bind(metrics).execute(&app.db).await.map_err(|e| e.to_string())?;
        match turn {
            Ok(Turn::Images(_)) => timing = Default::default(),
            Ok(Turn::Result(report)) => break (event, Ok(report)),
            Err(error) => break (event, Err(error)),
        }
    };
    if !compact
        && !needs_input
        && let Ok(report) = &result
        && report.approved
        && let Some(note) = report
            .knowledge
            .as_ref()
            .and_then(|candidate| knowledge_candidate(candidate, &learning_evidence, &illustrated))
        && let Err(error) = crate::workbench::publish_learned_knowledge(
            app,
            run,
            event,
            note.title,
            note.summary,
            &note.observations,
            note.image,
        )
        .await
    {
        eprintln!("Optional knowledge was not retained: {error}");
    }
    result
}

// Learning shares the completed audit's evidence and model call. A rejected
// optional candidate never turns a valid user result into another tool loop.
pub(crate) async fn retain_learning(
    app: &App,
    run: Uuid,
    needs_input: bool,
    candidate: Option<&Value>,
) -> bool {
    let Some(candidate) = candidate.filter(|_| !needs_input) else {
        return false;
    };
    match crate::app_learning::save(app, run, candidate).await {
        Ok(_) => true,
        Err(error) => {
            eprintln!(
                "Optional app learning was not retained; user task result preserved: {}",
                app.settings.redact(&error)
            );
            false
        }
    }
}

// History is evidence about previous work, never proof of a new action.
pub async fn history(app: &App, run: Uuid, limit: usize) -> Result<Value, String> {
    let rows=sqlx::query("SELECT r.id,r.status,r.created_at,m.text FROM runs r JOIN messages m ON m.id=r.assistant_message_id WHERE r.conversation_id=(SELECT conversation_id FROM runs WHERE id=$1) ORDER BY r.created_at DESC,r.id DESC LIMIT $2")
        .bind(run).bind(limit.clamp(1,8) as i64).fetch_all(&app.db).await.map_err(|e|e.to_string())?;
    let mut history = Vec::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let events=sqlx::query("SELECT id,title,status,content FROM harness_events WHERE run_id=$1 AND kind IN ('tool','audit') ORDER BY created_at,id LIMIT 100")
            .bind(id).fetch_all(&app.db).await.map_err(|e|e.to_string())?;
        history.push(json!({"run_id":id.to_string(),"status":row.get::<String,_>("status"),
            "assistant_claim_not_evidence":row.get::<String,_>("text").chars().take(1600).collect::<String>(),
            "actual_ledger":events.iter().map(|event|json!({"id":event.get::<Uuid,_>("id").to_string(),"title":event.get::<String,_>("title"),"status":event.get::<String,_>("status"),"content":app.settings.redact(&event.get::<String,_>("content")).chars().take(4000).collect::<String>()})).collect::<Vec<_>>() }));
    }
    Ok(json!({"note":"这是旧轮次真实账本，仅用于回答质疑，不可用作本轮执行证据","runs":history}))
}

fn audit_call(value: &Value) -> Result<&Value, String> {
    let mut calls = value["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "function_call");
    let call = calls.next().ok_or("结果核验没有返回有效结论")?;
    if calls.next().is_some() {
        return Err("结果核验必须返回唯一结论".into());
    }
    Ok(call)
}

fn parse_result(value: &Value) -> Result<AuditResult, String> {
    let call = audit_call(value)?;
    let args = (call["name"] == "audit_result")
        .then(|| call["arguments"].as_str())
        .flatten()
        .ok_or("结果核验没有返回有效结论")?;
    let report: Value = serde_json::from_str(args).map_err(|_| "结果核验没有返回有效结论")?;
    let approved = report["approved"].as_bool().ok_or("结果核验缺少证据判定")?;
    let feedback = report["feedback"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("结果核验缺少说明")?;
    Ok(AuditResult {
        approved,
        feedback: feedback.chars().take(4000).collect(),
        experience: report
            .get("experience")
            .filter(|v| approved && v.is_object())
            .cloned(),
        knowledge: report
            .get("knowledge")
            .filter(|v| approved && v.is_object())
            .cloned(),
    })
}

fn extract_images(value: &mut Value, id: &str, inputs: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            for item in items {
                extract_images(item, id, inputs);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("input_image") {
                let label = format!("截图 {id}");
                inputs.push(json!({"type":"input_text","text":label}));
                inputs.push(std::mem::replace(
                    value,
                    json!({"type":"input_text","text":label}),
                ));
            } else {
                for item in object.values_mut() {
                    extract_images(item, id, inputs);
                }
            }
        }
        _ => {}
    }
}

// Native observations repeat the same field names and often the same values
// for every control. Store the common fields once and keep every other value
// by column, including IDs, text, bounds and actionability in original order.
fn compact_phone_trees(evidence: &mut [Value]) {
    for receipt in evidence.iter_mut().filter(|r| r["tool"] == "phone_observe") {
        let Some(parts) = receipt["result"].as_array_mut() else {
            continue;
        };
        for part in parts {
            if part["type"] != "input_text" || part.as_object().is_none_or(|p| p.len() != 2) {
                continue;
            }
            let Some(text) = part["text"].as_str().filter(|s| s.contains("\"elements\"")) else {
                continue;
            };
            let Ok(mut frame) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if !frame["device_id"].is_string() || !frame["observation_id"].is_string() {
                continue;
            }
            let Some(controls) = frame["elements"].as_array().filter(|a| a.len() > 1) else {
                continue;
            };
            let Some(first) = controls[0].as_object() else {
                continue;
            };
            if !controls.iter().all(|c| {
                c.as_object().is_some_and(|o| {
                    o.len() == first.len() && first.keys().all(|key| o.contains_key(key))
                })
            }) {
                continue;
            }
            let mut common = serde_json::Map::new();
            let mut columns = Vec::new();
            for (key, value) in first {
                if controls.iter().all(|c| &c[key] == value) {
                    common.insert(key.clone(), value.clone());
                } else {
                    columns.push(key.as_str());
                }
            }
            let rows: Vec<Vec<_>> = controls
                .iter()
                .map(|c| columns.iter().map(|&k| &c[k]).collect())
                .collect();
            let table = json!({"common":common,"columns":columns,"rows":rows});
            frame["elements"] = table;
            if frame.to_string().len() < text.len() {
                *part = json!({"phone_observation":frame});
            }
        }
    }
}

fn append_visuals(input: &mut Vec<Value>, visuals: Vec<Value>) {
    // At most eight pictures. Compare against the images already in the
    // request so distinct sizes are rejected without hashing base64 bodies.
    let mut unique: Vec<usize> = Vec::new();
    let mut pairs = visuals.into_iter();
    while let (Some(mut label), Some(image)) = (pairs.next(), pairs.next()) {
        if let Some(prior) = unique.iter().position(|&index| input[index] == image) {
            let event = label["text"].as_str().unwrap();
            label["text"] = json!(format!("{event}（与前面第 {} 张图完全相同）", prior + 1));
            input.push(label);
        } else {
            input.push(label);
            unique.push(input.len());
            input.push(image);
        }
    }
}

fn limit_result(value: &mut Value, images: &mut usize) {
    match value {
        Value::Array(items) => {
            for item in items.iter_mut().rev() {
                limit_result(item, images);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("input_image") {
                if *images == 0 {
                    *value = json!({"type":"input_text","text":"截图已归档"});
                    return;
                }
                *images -= 1;
            }
            if let Some(Value::String(text)) = object.get_mut("text")
                && text.len() > 24000
            {
                *text = text.chars().take(24000).collect();
            }
            for (key, item) in object {
                if key != "image_url" {
                    limit_result(item, images);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_control_table_round_trips_every_target_in_order() {
        let controls: Vec<_> = (0..4)
            .map(|i| {
                json!({
                    "id":format!("n{i}"), "text":"Repeated label", "source":"native",
                    "resource_id":"app:id/item", "clickable":i != 1, "editable":false,
                    "bounds":[0,i*30,200,(i+1)*30], "confidence":1.0,
                })
            })
            .collect();
        let frame = json!({"device_id":"phone","observation_id":"frame","elements":controls});
        let mut receipt = json!({"tool":"phone_observe","result":[
            {"type":"input_text","text":frame.to_string()}
        ]});
        compact_phone_trees(std::slice::from_mut(&mut receipt));
        let compact = &receipt["result"][0]["phone_observation"];
        assert_eq!(compact["observation_id"], "frame");
        let table = &compact["elements"];
        let columns = table["columns"].as_array().unwrap();
        let restored: Vec<_> = table["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let mut control = table["common"].clone();
                for (name, value) in columns.iter().zip(row.as_array().unwrap()) {
                    control[name.as_str().unwrap()] = value.clone();
                }
                control
            })
            .collect();
        assert_eq!(restored, controls);
    }

    #[test]
    fn identical_images_keep_each_event_and_distinct_render_options() {
        let mut visuals = Vec::new();
        for (event, detail) in [("before", "high"), ("after", "high"), ("crop", "low")] {
            let mut part = json!({"type":"input_image","image_url":"data:image/png;base64,same","detail":detail});
            extract_images(&mut part, event, &mut visuals);
        }
        let mut input = Vec::new();
        append_visuals(&mut input, visuals);
        assert_eq!(input.len(), 5);
        assert_eq!(input[0]["text"], "截图 before");
        assert_eq!(input[1]["detail"], "high");
        assert_eq!(input[2]["text"], "截图 after（与前面第 1 张图完全相同）");
        assert_eq!(input[3]["text"], "截图 crop");
        assert_eq!(input[4]["detail"], "low");
    }

    #[test]
    fn short_review_requires_one_confirmed_input_and_its_observation() {
        let observation = json!({"tool":"computer_observe","status":"completed"});
        let zoom = json!({"tool":"computer_zoom","status":"completed"});
        let mut input = json!({"tool":"computer_type","status":"completed","result":[
            {"type":"input_text","text":json!({"success":true,"observation_id":"fresh"}).to_string()}
        ]});
        assert!(single_desktop_input(&[
            observation.clone(),
            input.clone(),
            zoom
        ]));
        assert!(!single_desktop_input(&[input.clone(), input.clone()]));
        input["status"] = json!("failed");
        assert!(!single_desktop_input(&[observation, input]));
    }

    #[test]
    fn native_navigation_reviews_exact_frames_and_requires_each_verified_link() {
        let [start, tap, middle, next, last] = std::array::from_fn(|_| Uuid::new_v4());
        let frame = |id: Uuid, observation: &str, activity: &str| {
            let metadata = json!({"device_id":"phone","observation_id":observation,
                "foreground_app":activity,"elements":[{"id":"n1","source":"native","clickable":true,"text":"Open"}]});
            json!({"evidence_id":id,"tool":"phone_observe","status":"completed",
                "result":[{"type":"input_text","text":metadata.to_string()}]})
        };
        let tap_row = |id: Uuid| {
            json!({"evidence_id":id,"tool":"phone_tap_element","status":"completed",
            "result":[{"type":"input_text","text":r#"{"success":true}"#}]})
        };
        let evidence = vec![
            frame(start, "a", "org.example.viewer/.Home"),
            tap_row(tap),
            frame(middle, "b", "org.example.viewer/.Item"),
            tap_row(next),
            frame(last, "c", "org.example.viewer/.Item"),
        ];
        let tap_call = |id: &str, observation: &str| {
            json!({"call_id":id,
            "arguments":json!({"element_id":"n1","observation_id":observation}).to_string()})
        };
        let first = tap_call("first", "a");
        let second = tap_call("second", "b");
        let intermediate = json!({"local_after_action":"first"});
        let readback = json!({"local_after_action":"second","local_readback":true});
        let calls = HashMap::from([
            (tap, &first),
            (middle, &intermediate),
            (next, &second),
            (last, &readback),
        ]);
        let pngs = HashMap::from([(last, 0), (middle, 2), (start, 4)]);
        let mut images = Vec::new();
        for id in [last, middle, start] {
            extract_images(
                &mut json!({"type":"input_image","image_url":format!("original-{id}")}),
                &id.to_string(),
                &mut images,
            );
        }
        let template = json!({"input":[{},{"content":[]}],"tools":[{"name":"audit_result"}]});
        let mut review = PhoneFrames::navigation(&evidence, &calls, &pngs).unwrap();
        let sent = |review: &PhoneFrames| {
            review.request(&template, &images)["input"][1]["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["image_url"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        assert_eq!(sent(&review), [format!("original-{last}")]);
        assert!(
            review
                .reveal(&json!({"evidence_ids":[start,Uuid::new_v4()]}).to_string())
                .is_err()
        );
        assert_eq!(sent(&review), [format!("original-{last}")]);
        review
            .reveal(&json!({"evidence_ids":[middle]}).to_string())
            .unwrap();
        assert_eq!(
            sent(&review),
            [format!("original-{middle}"), format!("original-{last}")]
        );
        assert!(
            review
                .reveal(&json!({"evidence_ids":[middle]}).to_string())
                .is_err()
        );
        review
            .reveal(&json!({"evidence_ids":[start]}).to_string())
            .unwrap();
        assert_eq!(
            review.request(&template, &images)["tools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            sent(&review),
            [
                format!("original-{start}"),
                format!("original-{middle}"),
                format!("original-{last}")
            ]
        );

        let missing = HashMap::from([(middle, 2), (start, 4)]);
        assert!(PhoneFrames::navigation(&evidence, &calls, &missing).is_none());
        let other = json!({"local_after_action":"elsewhere","local_readback":true});
        let no_hint = json!({"local_after_action":"second"});
        for invalid in [&other, &no_hint] {
            let mut calls = calls.clone();
            calls.insert(last, invalid);
            assert!(PhoneFrames::navigation(&evidence, &calls, &pngs).is_none());
        }
        for replacement in [
            frame(middle, "b", "org.another.app/.Screen"),
            frame(middle, "a", "org.example.viewer/.Screen"),
            json!({"evidence_id":middle,"tool":"computer_observe","status":"completed"}),
        ] {
            let mut invalid = evidence.clone();
            invalid[2] = replacement;
            assert!(PhoneFrames::navigation(&invalid, &calls, &pngs).is_none());
        }
        for (key, value) in [("device_id", json!("second")), ("elements", json!([]))] {
            let mut invalid = evidence.clone();
            let mut metadata = crate::worker::tool_value(&invalid[2]["result"]).unwrap();
            metadata[key] = value;
            invalid[2]["result"][0]["text"] = json!(metadata.to_string());
            assert!(PhoneFrames::navigation(&invalid, &calls, &pngs).is_none());
        }
        let mut invalid = evidence.clone();
        invalid[1]["result"][0]["text"] = json!(r#"{"success":true,"result_unknown":true}"#);
        assert!(PhoneFrames::navigation(&invalid, &calls, &pngs).is_none());
    }

    #[test]
    fn audit_learning_references_only_the_exposed_event_namespace() {
        let evidence = LearningEvidence::from([(
            "event-id".into(),
            crate::app_learning::Metadata {
                app_id: "linux:libreoffice-draw".into(),
                device_id: "desktop".into(),
                observation_id: "frame-id".into(),
            },
        )]);
        let schema = learning_schema(&evidence).unwrap();
        assert!(schema["properties"].get("observation_ids").is_none());
        assert_eq!(
            schema["properties"]["evidence_ids"]["items"]["enum"],
            json!(["event-id"])
        );
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "evidence_ids")
        );
        // Execution tools retain their observation-ID contract.
        assert!(
            crate::tool_catalog::experience_schema()["properties"]
                .get("observation_ids")
                .is_some()
        );
        assert!(learning_schema(&LearningEvidence::new()).is_none());
    }
    #[test]
    fn audit_learning_binds_current_app_events_to_actual_observations() {
        let evidence = LearningEvidence::from([
            (
                "event-a".into(),
                crate::app_learning::Metadata {
                    app_id: "linux:libreoffice-draw".into(),
                    device_id: "desktop".into(),
                    observation_id: "frame-a".into(),
                },
            ),
            (
                "event-b".into(),
                crate::app_learning::Metadata {
                    app_id: "linux:libreoffice-calc".into(),
                    device_id: "desktop".into(),
                    observation_id: "frame-b".into(),
                },
            ),
        ]);
        let candidate = json!({"app_id":"linux:libreoffice-draw","title":"draw method","evidence_ids":["event-a"]});
        let actual = learning_candidate(&candidate, &evidence).unwrap();
        assert_eq!(actual["observation_ids"], json!(["frame-a"]));
        assert!(actual.get("evidence_ids").is_none());
        for ids in [
            json!(["frame-a"]),
            json!(["unknown-event"]),
            json!(["event-a", "event-b"]),
            json!(null),
        ] {
            let mut invalid = candidate.clone();
            invalid["evidence_ids"] = ids;
            assert!(learning_candidate(&invalid, &evidence).is_err());
        }
        assert!(!crate::app_learning::observation_tool("host_exec"));
        assert!(!crate::app_learning::observation_tool("workspace_read"));
    }
    #[test]
    fn audit_cannot_default_to_approved_on_missing_or_malformed_result() {
        assert!(parse_result(&json!({"output":[]})).is_err());
        assert!(parse_result(&json!({"output":[{"type":"function_call","name":"audit_result","arguments":"{\"feedback\":\"done\"}"}]})).is_err());
    }
    #[test]
    fn successful_audit_retains_optional_experience_without_requiring_another_model_turn() {
        let candidate = json!({"app_id":"com.example.reader","title":"搜索","procedure":"点击当前可见搜索入口，输入后核对实际列表。","outcome":"verified","limitations":"仅限已观察版本","observation_ids":["before","after"]});
        let result=parse_result(&json!({"output":[{"type":"function_call","name":"audit_result","arguments":json!({"approved":true,"feedback":"界面与记录吻合","experience":candidate}).to_string()}]})).unwrap();
        assert!(result.approved);
        assert_eq!(result.experience, Some(candidate));
    }
    #[test]
    fn rejected_or_absent_experience_does_not_create_learning_or_reject_a_valid_task() {
        for (approved, experience) in [
            (false, json!({"app_id":"com.example.fake"})),
            (true, Value::Null),
            (true, json!("invalid")),
        ] {
            let result=parse_result(&json!({"output":[{"type":"function_call","name":"audit_result","arguments":json!({"approved":approved,"feedback":"核验结论","experience":experience}).to_string()}]})).unwrap();
            assert_eq!(result.approved, approved);
            assert!(result.experience.is_none());
        }
    }
    #[test]
    fn evidence_retains_only_bounded_recent_images() {
        let mut result = json!([{"type":"input_image","image_url":"old"},{"type":"input_image","image_url":"new"}]);
        limit_result(&mut result, &mut 1);
        assert!(result[0].get("image_url").is_none());
        assert_eq!(result[1]["image_url"], "new");
    }
    #[tokio::test]
    #[ignore = "real disposable PostgreSQL and loopback audit provider; no external model or device calls"]
    async fn audit_event_references_save_promote_and_recall_only_real_current_app_evidence() {
        use axum::{Json, Router, routing::post};
        async fn scenario(app: Shared) {
            let reply = Arc::new(std::sync::Mutex::new(Value::Null));
            let response = reply.clone();
            let (sent, mut received) = tokio::sync::mpsc::channel(4);
            let router = Router::new().route("/responses",post(move |Json(body):Json<Value>| {
                let sent=sent.clone();let response=response.clone();
                async move {
                    sent.send(body).await.unwrap();
                    let args=response.lock().unwrap().clone();
                    let event=json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","name":"audit_result","arguments":args.to_string()}]}});
                    ([("content-type","text/event-stream")],format!("data: {event}\n\n"))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            struct Server(tokio::task::JoinHandle<()>);
            impl Drop for Server {
                fn drop(&mut self) {
                    self.0.abort();
                }
            }
            let _server = Server(tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap()
            }));
            app.settings.test_revision(crate::settings::ModelConfig {
                api_key: String::new(),
                base_url: format!("http://{address}"),
                model: "audit-fixture".into(),
                provider: "compatible".into(),
                protocol: "responses".into(),
                proxy_url: String::new(),
                verified_at_ms: 1,
            });
            crate::registry::initialize(&app).await.unwrap();
            let conversation: Uuid =
                sqlx::query_scalar("SELECT id FROM conversations WHERE owner_id='local-user'")
                    .fetch_one(&app.db)
                    .await
                    .unwrap();
            let run = Uuid::now_v7();
            let message = Uuid::now_v7();
            sqlx::query("INSERT INTO messages(id,conversation_id,role) VALUES($1,$2,'assistant')")
                .bind(message)
                .bind(conversation)
                .execute(&app.db)
                .await
                .unwrap();
            sqlx::query("INSERT INTO runs(id,conversation_id,assistant_message_id,request_id,request_fingerprint,status) VALUES($1,$2,$3,$1,'fixture','running')").bind(run).bind(conversation).bind(message).execute(&app.db).await.unwrap();
            let ids = [
                Uuid::now_v7(),
                Uuid::now_v7(),
                Uuid::now_v7(),
                Uuid::now_v7(),
            ];
            let mut context = vec![
                json!({"role":"user","content":[{"type":"input_text","text":"Read the current app"}]}),
            ];
            for (i, id) in ids.iter().enumerate() {
                let tool = if i == 3 {
                    "workspace_read"
                } else {
                    "phone_observe"
                };
                let package = if i == 2 {
                    "com.example.other"
                } else {
                    "com.example.reader"
                };
                let mut output = json!([{"type":"input_text","text":json!({"success":true,"device_id":"phone-local-01","observation_id":format!("frame-{i}"),"foreground_app":package}).to_string()}]);
                if i == 2 {
                    let png = include_bytes!("../fixtures/vision-probe.png");
                    let encoded =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png);
                    output.as_array_mut().unwrap().push(json!({"type":"input_image","image_url":format!("data:image/png;base64,{encoded}")}));
                }
                sqlx::query("INSERT INTO harness_events(id,run_id,message_id,kind,title,status,result) VALUES($1,$2,$3,'tool','fixture','completed',$4)").bind(id).bind(run).bind(message).bind(&output).execute(&app.db).await.unwrap();
                context.push(json!({"type":"function_call","name":tool,"local_action_id":id.to_string(),"arguments":"{}"}));
                if crate::app_learning::observation_tool(tool) {
                    crate::app_learning::observe(&app, run, *id, i > 0, &output)
                        .await
                        .unwrap();
                }
            }
            let other_run = Uuid::now_v7();
            let other_message = Uuid::now_v7();
            sqlx::query("INSERT INTO messages(id,conversation_id,role) VALUES($1,$2,'assistant')")
                .bind(other_message)
                .bind(conversation)
                .execute(&app.db)
                .await
                .unwrap();
            sqlx::query("INSERT INTO runs(id,conversation_id,assistant_message_id,request_id,request_fingerprint,status) VALUES($1,$2,$3,$1,'fixture','canceled')")
                .bind(other_run).bind(conversation).bind(other_message).execute(&app.db).await.unwrap();
            let wrong_run = verify(
                &app,
                other_run,
                other_message,
                &context,
                Proposal {
                    summary: "Read the app",
                    needs_input: false,
                    attachments: &[],
                },
                &CancellationToken::new(),
            )
            .await;
            assert!(
                matches!(wrong_run, Err(error) if error == "本轮没有已确认的执行回执，无法核验完成。")
            );
            assert!(
                received.try_recv().is_err(),
                "another run's receipts reached the model"
            );
            let mut candidate = json!({"app_id":"com.example.reader","title":"Open the search field","procedure":"Click the visible search entry and verify the actual input field before typing.","outcome":"verified","limitations":"Only the observed application version."});
            for (references, saved, note_references, approved, notes) in [
                (
                    json!([ids[0].to_string(), ids[2].to_string()]),
                    0i64,
                    [ids[0], ids[1]],
                    true,
                    0i64,
                ),
                (
                    json!([ids[0].to_string(), "unknown-event"]),
                    0,
                    [ids[0], ids[3]],
                    true,
                    0,
                ),
                (
                    json!([ids[0].to_string(), ids[3].to_string()]),
                    0,
                    [ids[0], ids[2]],
                    false,
                    0,
                ),
                (
                    json!([ids[0].to_string(), ids[3].to_string()]),
                    0,
                    [ids[0], ids[2]],
                    true,
                    1,
                ),
                (
                    json!([ids[0].to_string(), ids[1].to_string()]),
                    1,
                    [ids[0], ids[2]],
                    true,
                    1,
                ),
            ] {
                candidate["evidence_ids"] = references;
                *reply.lock().unwrap() = json!({"approved":approved,"feedback":"Fixture task evidence","experience":candidate,"knowledge":{
                    "title":"Check import results in both applications",
                    "summary":"Verify the actual result in the receiving application instead of assuming a handoff is complete.",
                    "evidence_ids":note_references,
                    "image_evidence_id":ids[2],
                }});
                assert_eq!(
                    verify(
                        &app,
                        run,
                        message,
                        &context,
                        Proposal {
                            summary: "Fixture result",
                            needs_input: false,
                            attachments: &[]
                        },
                        &CancellationToken::new()
                    )
                    .await
                    .unwrap()
                    .approved,
                    approved
                );
                let request = received.recv().await.unwrap();
                let schema = &request["tools"][0]["parameters"]["properties"]["experience"];
                assert!(schema["properties"].get("observation_ids").is_none());
                let allowed = schema["properties"]["evidence_ids"]["items"]["enum"]
                    .as_array()
                    .unwrap();
                assert!(allowed.contains(&json!(ids[0].to_string())));
                assert!(
                    !allowed.contains(&json!(ids[3].to_string())),
                    "arbitrary workspace output cannot produce GUI learning evidence"
                );
                assert_eq!(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT count(*) FROM app_experiences WHERE run_id=$1"
                    )
                    .bind(run)
                    .fetch_one(&app.db)
                    .await
                    .unwrap(),
                    saved
                );
                let knowledge = crate::workbench::invoke(
                    &app,
                    "knowledge_search",
                    &json!({"query":"Check import results"}),
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
                assert_eq!(knowledge.as_array().unwrap().len(), notes as usize);
            }
            let (id, initial): (Uuid, i64) =
                sqlx::query_as("SELECT id,revision FROM knowledge_documents WHERE learned_key=$1")
                    .bind("check import results in both applications")
                    .fetch_one(&app.db)
                    .await
                    .unwrap();
            let image: Vec<u8> = sqlx::query_scalar("SELECT image FROM knowledge_images")
                .fetch_one(&app.db)
                .await
                .unwrap();
            assert_eq!(image, include_bytes!("../fixtures/vision-probe.png"));
            let observations = [String::from("frame-0"), String::from("frame-2")];
            assert!(
                crate::workbench::publish_learned_knowledge(
                    &app,
                    other_run,
                    Uuid::now_v7(),
                    "Check import results in both applications",
                    "A different run cannot attach this screenshot.",
                    &observations,
                    Some("frame-2"),
                )
                .await
                .is_err()
            );
            crate::workbench::publish_learned_knowledge(
                &app,
                run,
                Uuid::now_v7(),
                "Check import results in both applications",
                "Reopen the result in the receiving application to verify its format.",
                &observations,
                None,
            )
            .await
            .unwrap();
            let (content, revision): (String, i64) =
                sqlx::query_as("SELECT content,revision FROM knowledge_documents WHERE id=$1")
                    .bind(id)
                    .fetch_one(&app.db)
                    .await
                    .unwrap();
            assert!(content.contains("](/api/workbench/knowledge-image/"));
            assert!(content.contains("Reopen the result"));
            assert_eq!(revision, initial + 1);
            let related = crate::workbench::invoke_tool(
                &app,
                "knowledge_write",
                &json!({"title":"Inspect the receiving application","content":"Use the application UI to reopen the result.","related_ids":[id]}),
                &CancellationToken::new(),
                Some(run),
            )
            .await
            .unwrap();
            let linked = crate::workbench::invoke(
                &app,
                "knowledge_read",
                &json!({"id":id}),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(linked["related_ids"], json!([related["id"]]));
            let revised = crate::workbench::invoke_tool(
                &app,
                "knowledge_write",
                &json!({"id":related["id"],"revision":related["revision"],"title":"Inspect the receiving application","content":"Read the final result in the UI.","image_observation_id":"frame-2","image_alt":"Observed import"}),
                &CancellationToken::new(),
                Some(run),
            )
            .await
            .unwrap();
            assert_eq!(revised["id"], related["id"]);
            assert_eq!(revised["related_ids"], json!([id]));
            crate::app_learning::promote(&app, run).await.unwrap();
            let name = crate::app_learning::recall(&app, "com.example.reader")
                .await
                .unwrap()
                .unwrap();
            let stored = crate::workbench::skill_text(&app, &name).await.unwrap();
            assert!(stored.contains("Click the visible search entry"));
            let observed = vec![
                json!({"type":"function_call","name":"phone_observe","call_id":"current"}),
                json!({"type":"function_call_output","call_id":"current","output":[{"type":"input_text","text":json!({"success":true,"observation_id":"current-frame","foreground_app":"com.example.reader"}).to_string()}]}),
            ];
            let call = crate::worker::experience_call(
                &app,
                &observed,
                &mut crate::agent_loop::LoopState::default(),
            )
            .await
            .unwrap();
            assert_eq!(call["name"], "skill_read");
            assert_eq!(
                serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap()["name"],
                name
            );
        }
        crate::worker::steering_regressions::database_case(scenario).await;
    }
}
