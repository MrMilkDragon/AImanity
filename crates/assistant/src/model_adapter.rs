use crate::{settings::ModelConfig, *};
pub use aimanity_model::{request_tracked, validate_call_ids};
use aimanity_model::{text, uses_claude_binding};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::time::Instant;
#[cfg(test)]
pub async fn request(
    config: &ModelConfig,
    body: Value,
    cancel: &CancellationToken,
) -> Result<Vec<Value>, String> {
    request_tracked(
        config,
        &config.client()?,
        body,
        cancel,
        &mut crate::model_timing::Timing::default(),
    )
    .await
}

async fn check(
    config: &ModelConfig,
    client: &reqwest::Client,
    name: &str,
    body: Value,
    expected: &str,
) -> wire::CapabilityCheck {
    let start = Instant::now();
    let cancel = CancellationToken::new();
    let result = tokio::time::timeout(
        Duration::from_secs(55),
        request_tracked(config, client, body, &cancel, &mut Default::default()),
    )
    .await;
    let (passed, detail) = match result {
        Ok(Ok(output)) if text(&output).contains(expected) => (true, "通过".into()),
        Ok(Ok(_)) => (false, "已收到输出，但实际内容未通过验证".into()),
        Ok(Err(e)) => (false, e),
        Err(_) => (false, "测试超过 55 秒，请检查网络或模型速度".into()),
    };
    wire::CapabilityCheck {
        name: name.into(),
        passed,
        detail,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}
pub async fn probe(
    config: &ModelConfig,
    client: &reqwest::Client,
    vision: bool,
) -> Vec<wire::CapabilityCheck> {
    let nonce = format!("{:06}", Uuid::new_v4().as_fields().0 % 1_000_000);
    let text_body = json!({"input":[{"role":"user","content":[{"type":"input_text","text":format!("Your ticket number is {nonce}. What is your ticket number? Reply with the number only.")}]}],"max_output_tokens":256,"reasoning":{"effort":"none"}});
    let (text_check, image_check, tool_check) = tokio::join!(
        check(config, client, "文字与流式输出", text_body, &nonce),
        async {
            if !vision {
                return None;
            }
            let image_body = json!({"input":[{"role":"user","content":[{"type":"input_text","text":"Read the four digits in this image. Reply only with the digits."},{"type":"input_image","image_url":format!("data:image/png;base64,{}",STANDARD.encode(include_bytes!("../fixtures/vision-probe.png")))}]}],"max_output_tokens":256,"reasoning":{"effort":"none"}});
            Some(check(config, client, "图片识别", image_body, "7319").await)
        },
        tool_probe(config, client)
    );
    [Some(text_check), image_check, Some(tool_check)]
        .into_iter()
        .flatten()
        .collect()
}
async fn tool_probe(config: &ModelConfig, client: &reqwest::Client) -> wire::CapabilityCheck {
    let start = Instant::now();
    let nonce = Uuid::new_v4().simple().to_string();
    let cancel = CancellationToken::new();
    let tool = json!({"type":"function","name":"diagnostic_echo","description":"Returns a diagnostic result; no external effects.","parameters":{"type":"object","properties":{"code":{"type":"string"}},"required":["code"],"additionalProperties":false}});
    let initial = json!({"role":"user","content":[{"type":"input_text","text":format!("Call diagnostic_echo with code {nonce}. After the result, reply with only the returned receipt.")} ]});
    let work = async {
        let output = request_tracked(
            config,
            client,
            json!({"input":[initial.clone()],"tools":[tool.clone()],"tool_choice":{"type":"function","name":"diagnostic_echo"},"parallel_tool_calls":false,"reasoning":{"effort":"none"},"max_output_tokens":512}),
            &cancel,
            &mut Default::default(),
        ).await?;
        let calls: Vec<_> = output
            .iter()
            .filter(|v| v["type"] == "function_call")
            .collect();
        if calls.len() != 1 || calls[0]["name"] != "diagnostic_echo" {
            return Err("模型没有正确调用测试工具".into());
        }
        let call = calls[0];
        let args: Value = serde_json::from_str(call["arguments"].as_str().unwrap_or(""))
            .map_err(|_| "工具参数不是有效 JSON")?;
        if args["code"] != nonce || call["call_id"].as_str().unwrap_or("").is_empty() {
            return Err("工具参数或调用标识不匹配".into());
        }
        let receipt = Uuid::new_v4().simple().to_string();
        let result = json!({"type":"function_call_output","call_id":call["call_id"],"output":json!({"receipt":receipt}).to_string()});
        let mut input = vec![initial];
        input.extend(output);
        input.push(result);
        let input = if config.protocol == "responses" {
            crate::model_context::for_api(&input)
        } else {
            crate::model_context::for_chat_api(&input)
        };
        let mut followup =
            json!({"input":input,"max_output_tokens":256,"reasoning":{"effort":"none"}});
        if config.protocol == "anthropic-messages" && uses_claude_binding(config) {
            followup["tools"] = json!([tool]);
            followup["tool_choice"] = json!("none");
        }
        let final_output =
            request_tracked(config, client, followup, &cancel, &mut Default::default()).await?;
        if !text(&final_output).contains(&receipt) {
            return Err("模型未读取工具的实际返回结果".into());
        }
        Ok::<(), String>(())
    };
    let (passed, detail) = match tokio::time::timeout(Duration::from_secs(110), work).await {
        Ok(Ok(())) => (true, "调用参数、结果回传与后续读取均通过".into()),
        Ok(Err(e)) => (false, e),
        Err(_) => (false, "工具往返测试超时".into()),
    };
    wire::CapabilityCheck {
        name: "工具调用与结果读取".into(),
        passed,
        detail,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_messages_http_auth_image_and_tool_round_trip() {
        use axum::{Json, Router, http::HeaderMap, routing::post};
        fn events(tool: bool) -> String {
            let block = if tool {
                json!({"type":"tool_use","id":"native-call","name":"observe","input":{}})
            } else {
                json!({"type":"text","text":"actual-native-receipt"})
            };
            [json!({"type":"message_start","message":{"role":"assistant"}}),json!({"type":"content_block_start","index":0,"content_block":block}),json!({"type":"content_block_stop","index":0}),json!({"type":"message_delta","delta":{"stop_reason":if tool{"tool_use"}else{"end_turn"}}}),json!({"type":"message_stop"})].iter().map(|v|format!("data: {v}\n\n")).collect()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route(
            "/v1/messages",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["x-api-key"], "fixture-secret");
                assert_eq!(headers["anthropic-version"], "2023-06-01");
                assert!(!headers.contains_key("authorization"));
                assert!(body.get("input").is_none());
                assert!(body.get("reasoning").is_none());
                assert_eq!(body["system"], "runtime-policy");
                let tool = body["messages"].as_array().unwrap().iter().any(|v| {
                    v["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|p| p["type"] == "tool_result")
                });
                if body["model"] == "deepseek-fixture" {
                    assert_eq!(
                        body["thinking"]["type"],
                        if tool { "enabled" } else { "disabled" }
                    );
                    assert_eq!(
                        body["output_config"]["effort"],
                        if tool { json!("low") } else { Value::Null }
                    );
                } else {
                    assert!(body.get("thinking").is_none());
                    assert!(body.get("output_config").is_none());
                }
                if tool {
                    assert_eq!(body["messages"][1]["content"][0]["id"], "native-call");
                    assert_eq!(
                        body["messages"][2]["content"][0]["tool_use_id"],
                        "native-call"
                    );
                    assert_eq!(
                        body["messages"][2]["content"][0]["content"][0]["text"],
                        "actual-result"
                    );
                } else {
                    assert_eq!(
                        body["messages"][0]["content"][1]["source"]["media_type"],
                        "image/png"
                    );
                    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
                    assert_eq!(body["tool_choice"]["name"], "observe");
                }
                ([("content-type", "text/event-stream")], events(!tool))
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut config = ModelConfig {
            provider: "anthropic".into(),
            protocol: "anthropic-messages".into(),
            api_key: "fixture-secret".into(),
            base_url: format!("http://{address}"),
            model: "fixture".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        for (provider, model) in [("anthropic", "fixture"), ("deepseek", "deepseek-fixture")] {
            config.provider = provider.into();
            config.model = model.into();
            let mut input = vec![
                json!({"role":"user","content":[{"type":"input_text","text":"inspect"},{"type":"input_image","image_url":"data:image/png;base64,Zml4dHVyZQ=="}]}),
            ];
            let first=request(&config,json!({"instructions":"runtime-policy","input":input,"tools":[{"type":"function","name":"observe","description":"inspect","parameters":{"type":"object"}}],"tool_choice":{"type":"function","name":"observe"},"reasoning":{"effort":"high"}}),&CancellationToken::new()).await.unwrap();
            assert_eq!(first[0]["call_id"], "native-call");
            input.extend(first);
            input.push(
                json!({"type":"function_call_output","call_id":"native-call","output":"actual-result"}),
            );
            let projected = crate::model_context::for_chat_api(&input);
            let second = request(
                &config,
                json!({"instructions":"runtime-policy","input":projected,"reasoning":{"effort":"low"}}),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(text(&second), "actual-native-receipt");
        }
        server.abort();
    }

    #[tokio::test]
    async fn sequential_model_calls_reuse_connections_without_sharing_credentials() {
        use axum::{Router, body::Body, extract::ConnectInfo, http::HeaderMap, routing::post};
        use std::{
            net::SocketAddr,
            sync::{Arc, Mutex},
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let captured = received.clone();
        let response = format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"pooled"}]}]}})
        );
        let router = Router::new().route(
            "/v1/responses",
            post(
                move |ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap| {
                    captured
                        .lock()
                        .unwrap()
                        .push((peer, headers["authorization"].to_str().unwrap().to_owned()));
                    let response = response.clone();
                    async move {
                        let stream =
                            futures_util::stream::iter([Ok::<_, std::io::Error>(response)]);
                        (
                            [("content-type", "text/event-stream")],
                            Body::from_stream(stream),
                        )
                    }
                },
            ),
        );
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let root = std::env::temp_dir().join(format!("ai-phone-pool-{}", Uuid::new_v4()));
        let store = crate::settings::SettingsStore::open(&root).unwrap();
        let mut config = ModelConfig {
            provider: "compatible".into(),
            protocol: "responses".into(),
            api_key: String::new(),
            base_url: format!("http://{address}/v1"),
            model: "fixture".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        for (credential, proxy) in [
            ("fixture-one", String::new()),
            ("fixture-two", String::new()),
            ("fixture-proxy", format!("http://{address}")),
            ("fixture-return", String::new()),
        ] {
            config.api_key = credential.into();
            config.proxy_url = proxy;
            let client = store.http_client(&config.proxy_url).unwrap();
            let output = request_tracked(
                &config,
                &client,
                json!({"input":[]}),
                &CancellationToken::new(),
                &mut crate::model_timing::Timing::default(),
            )
            .await
            .unwrap();
            assert_eq!(text(&output), "pooled");
        }
        server.abort();
        let _ = server.await;
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
        let calls = received.lock().unwrap();
        assert_eq!(calls.len(), 4);
        assert_eq!(
            calls[0].0, calls[1].0,
            "the second model call opened another TCP connection"
        );
        assert_ne!(
            calls[0].0, calls[2].0,
            "a different proxy reused the original routing pool"
        );
        assert_eq!(
            calls[0].0, calls[3].0,
            "returning to the direct route lost the idle connection"
        );
        assert_eq!(calls[0].1, "Bearer fixture-one");
        assert_eq!(calls[1].1, "Bearer fixture-two");
        assert_eq!(calls[2].1, "Bearer fixture-proxy");
        assert_eq!(calls[3].1, "Bearer fixture-return");
    }

    #[tokio::test]
    async fn transport_timing_separates_headers_first_bytes_first_text_and_complete_output() {
        use axum::{Router, body::Body, routing::post};
        use futures_util::StreamExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route("/v1/responses",post(|| async {
            let events = [json!({"type":"response.created"}),
                json!({"type":"response.reasoning_text.delta","delta":"fixture-reasoning"}),
                json!({"type":"response.output_text.delta","delta":"timed-"}),
                json!({"type":"response.completed",
                "response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"timed-output"}]}]}})];
            let stream = futures_util::stream::iter(events).then(|event| async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                Ok::<_,std::io::Error>(format!("data: {event}\n\n"))
            });
            ([("content-type","text/event-stream")],Body::from_stream(stream))
        }));
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let config = ModelConfig {
            provider: "compatible".into(),
            protocol: "responses".into(),
            api_key: "fixture-secret".into(),
            base_url: format!("http://{address}/v1"),
            model: "fixture".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        let mut timing = crate::model_timing::Timing::default();
        let output = request_tracked(
            &config,
            &config.client().unwrap(),
            json!({"input":[]}),
            &CancellationToken::new(),
            &mut timing,
        )
        .await;
        server.abort();
        let _ = server.await;
        assert_eq!(text(&output.unwrap()), "timed-output");
        let facts = timing.snapshot();
        let value = crate::model_timing::wire(&facts).unwrap();
        assert!(value.request_bytes > 0);
        assert!(value.headers_ms.unwrap() <= value.first_byte_ms.unwrap());
        assert!(value.first_byte_ms.unwrap() >= 20);
        assert!(value.first_text_ms.unwrap() >= value.first_byte_ms.unwrap() + 40);
        assert!(value.model_ms >= value.first_text_ms.unwrap() + 20);
        assert!(value.elapsed_ms >= value.model_ms);
        assert!(!facts.to_string().contains("fixture-secret"));
        let roundtrip = wire::ModelCallTiming::decode(value.encode_to_vec().as_slice()).unwrap();
        assert_eq!(roundtrip, value);
    }

    #[tokio::test]
    async fn a_billing_failure_has_headers_but_no_invented_stream_latency() {
        let (config, server) = error_fixture(StatusCode::PAYMENT_REQUIRED, false).await;
        let mut timing = crate::model_timing::Timing::default();
        let result = request_tracked(
            &config,
            &config.client().unwrap(),
            json!({"input":[]}),
            &CancellationToken::new(),
            &mut timing,
        )
        .await;
        server.abort();
        let _ = server.await;
        assert!(result.unwrap_err().contains("402"));
        let value = crate::model_timing::wire(&timing.snapshot()).unwrap();
        assert!(value.headers_ms.is_some());
        assert!(value.first_byte_ms.is_none());
        assert!(value.first_text_ms.is_none());
    }

    async fn error_fixture(
        status: StatusCode,
        stalled: bool,
    ) -> (ModelConfig, tokio::task::JoinHandle<()>) {
        use axum::{Router, body::Body, routing::post};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route(
            "/v1/responses",
            post(move || async move {
                let body = if stalled {
                    Body::from_stream(
                        futures_util::stream::once(async {
                            Ok::<_, std::io::Error>(Bytes::from_static(b"{"))
                        })
                        .chain(futures_util::stream::pending()),
                    )
                } else {
                    Body::from("{\"error\":{\"message\":\"private provider details\"}}")
                };
                (status, body)
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (
            ModelConfig {
                api_key: String::new(),
                base_url: format!("http://{address}/v1"),
                model: "fixture".into(),
                provider: "compatible".into(),
                protocol: "responses".into(),
                proxy_url: String::new(),
                verified_at_ms: 0,
            },
            server,
        )
    }
    #[tokio::test]
    async fn payment_required_reports_billing_without_disclosing_provider_payload() {
        let (config, server) = error_fixture(StatusCode::PAYMENT_REQUIRED, false).await;
        let result = request(&config, json!({"input":[]}), &CancellationToken::new())
            .await
            .unwrap_err();
        server.abort();
        assert!(result.contains("HTTP 402"));
        assert!(result.contains("余额"));
        assert!(!result.contains("private provider details"));
        assert!(crate::agent_loop::model_retry_delay(&result, 1).is_none());
    }
    #[tokio::test]
    async fn a_stalled_rate_limit_body_cannot_hold_the_agent_for_the_model_timeout() {
        let (config, server) = error_fixture(StatusCode::TOO_MANY_REQUESTS, true).await;
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            request(&config, json!({"input":[]}), &CancellationToken::new()),
        )
        .await;
        server.abort();
        let error = result
            .expect("an error-body inspection has its own short deadline")
            .unwrap_err();
        assert!(error.contains("HTTP 429"));
        assert_eq!(crate::agent_loop::model_retry_delay(&error, 1), Some(2000));
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_rate_limit_body() {
        use axum::{Router, body::Body, http::StatusCode, routing::post};
        let ready = Arc::new(tokio::sync::Notify::new());
        let handler_ready = ready.clone();
        let router = Router::new().route(
            "/v1/responses",
            post(move || {
                let ready = handler_ready.clone();
                async move {
                    let chunks = futures_util::stream::once(async move {
                        ready.notify_one();
                        Ok::<_, std::io::Error>(Bytes::from_static(b"{"))
                    })
                    .chain(futures_util::stream::pending());
                    (StatusCode::TOO_MANY_REQUESTS, Body::from_stream(chunks))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let config = ModelConfig {
            api_key: String::new(),
            base_url: format!("http://{address}/v1"),
            model: "fixture".into(),
            provider: "compatible".into(),
            protocol: "responses".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let mut task =
            tokio::spawn(async move { request(&config, json!({"input":[]}), &token).await });
        tokio::time::timeout(Duration::from_secs(2), ready.notified())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_millis(300), &mut task).await;
        task.abort();
        server.abort();
        assert_eq!(
            result
                .expect("cancel must not wait for the provider error body")
                .unwrap(),
            Err("canceled".into())
        );
    }
    #[tokio::test]
    async fn chat_protocol_round_trip_through_a_real_local_http_endpoint() {
        use axum::{Json, Router, routing::post};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router=Router::new().route("/v1/chat/completions",post(|Json(value):Json<Value>| async move {
            assert_eq!(value["model"],"fixture-model");
            assert_eq!(value["stream"],true);
            assert!(value.get("input").is_none());
            assert_eq!(value["thinking"]["type"],"enabled");
            assert_eq!(value["reasoning_effort"],"high");
            let text=value["messages"].as_array().unwrap().iter().find(|m|m["role"]=="tool");
            let delta=if let Some(result)=text {
                assert_eq!(result["tool_call_id"],"fixture-call");
                assert_eq!(value["messages"][1]["reasoning_content"],"private-replay");
                json!({"content":format!("Receipt: {}",result["content"].as_str().unwrap())})
            } else {
                assert_eq!(value["messages"][0]["content"][1]["image_url"]["url"],"data:image/png;base64,fixture");
                json!({"reasoning_content":"private-replay","tool_calls":[{"index":0,"id":"fixture-call","type":"function","function":{"name":"diagnostic_echo","arguments":"{\"code\":\"receipt\"}"}}]})
            };
            let reason=if text.is_some(){"stop"}else{"tool_calls"};
            ([ ("content-type","text/event-stream") ],format!("data: {}\n\ndata: [DONE]\n\n",json!({"choices":[{"index":0,"delta":delta,"finish_reason":reason}]})))
        }));
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let config = ModelConfig {
            api_key: String::new(),
            base_url: format!("http://{address}/v1"),
            model: "fixture-model".into(),
            provider: "deepseek".into(),
            protocol: "chat-completions".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        let input = json!({"role":"user","content":[{"type":"input_text","text":"inspect"},{"type":"input_image","image_url":"data:image/png;base64,fixture"}]});
        let output = request(
            &config,
            json!({"input":[input.clone()]}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output[0]["name"], "diagnostic_echo");
        let mut context = vec![input];
        context.extend(output);
        context.push(json!({"type":"function_call_output","call_id":"fixture-call","output":"actual-local-result"}));
        let output = request(&config, json!({"input":context}), &CancellationToken::new())
            .await
            .unwrap();
        assert!(text(&output).contains("actual-local-result"));
        server.abort();
    }
    #[tokio::test]
    async fn successful_http_with_a_non_stream_body_is_not_model_success() {
        use axum::{Router, routing::post};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route(
            "/responses",
            post(|| async {
                (
                    [("content-type", "application/json")],
                    "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
                )
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let config = ModelConfig {
            api_key: String::new(),
            base_url: format!("http://{address}"),
            model: "fixture".into(),
            provider: "compatible".into(),
            protocol: "responses".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        let result = request(&config, json!({"input":[]}), &CancellationToken::new()).await;
        server.abort();
        assert!(result.unwrap_err().contains("text/event-stream"));
    }
    #[tokio::test]
    async fn full_endpoint_addresses_are_not_appended_twice() {
        use axum::{Router, routing::post};
        for (protocol, suffix, body) in [
            (
                "responses",
                "responses",
                "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"text\":\"receipt\"}]}]}}\n\n",
            ),
            (
                "chat-completions",
                "chat/completions",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"receipt\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let router = Router::new().route(
                &format!("/v1/{suffix}"),
                post(move || async move {
                    ([("content-type", "text/event-stream; charset=utf-8")], body)
                }),
            );
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let config = ModelConfig {
                api_key: String::new(),
                base_url: format!("http://{address}/v1/{suffix}"),
                model: "fixture".into(),
                provider: "compatible".into(),
                protocol: protocol.into(),
                proxy_url: String::new(),
                verified_at_ms: 0,
            };
            let output = request(&config, json!({"input":[]}), &CancellationToken::new()).await;
            server.abort();
            assert_eq!(text(&output.unwrap()), "receipt");
        }
    }

    #[tokio::test]
    async fn http_completed_event_with_incomplete_status_does_not_release_tools() {
        use axum::{Router, routing::post};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let event = json!({"type":"response.completed","response":{"status":"incomplete","output":[{"type":"function_call","call_id":"unexecuted-fixture","name":"phone_tap","arguments":"{\"x\":1,\"y\":1}"}]}});
        let payload = format!("data: {event}\n\n");
        let router = Router::new().route(
            "/responses",
            post(move || {
                let payload = payload.clone();
                async move { ([("content-type", "text/event-stream")], payload) }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let config = ModelConfig {
            api_key: String::new(),
            base_url: format!("http://{address}"),
            model: "fixture".into(),
            provider: "compatible".into(),
            protocol: "responses".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        let tool = json!({"type":"function","name":"phone_tap","parameters":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"]}});
        let result = request(
            &config,
            json!({"input":[],"tools":[tool]}),
            &CancellationToken::new(),
        )
        .await;
        server.abort();
        assert!(
            result.is_err(),
            "HTTP success cannot turn explicitly incomplete output into executable proposals"
        );
    }
    #[tokio::test]
    #[ignore = "real provider capability test; uses private local configuration, no phone or chat writes"]
    async fn live_model_capabilities() {
        let root = std::env::var("AI_PHONE_LIVE_TEST_ROOT").expect("explicit live test root");
        let store = crate::settings::SettingsStore::open(std::path::Path::new(&root)).unwrap();
        let config = store.get(store.current()).unwrap();
        let checks = probe(&config, &config.client().unwrap(), true).await;
        for check in &checks {
            println!(
                "{}: {} ({}ms) {}",
                check.name, check.passed, check.duration_ms, check.detail
            );
        }
        assert!(
            checks.iter().all(|c| c.passed),
            "Live capability test failed"
        );
    }
}
