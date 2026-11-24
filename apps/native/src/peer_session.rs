use crate::device_io::Result;
#[cfg(not(target_os = "android"))]
use crate::host_desktop::{Views, execute};
use ai_phone_api::wire::{DesktopActionRequest, HostDesktopReply};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::tungstenite::Message as Frame;

pub(super) async fn serve_socket<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    mut socket: tokio_tungstenite::WebSocketStream<S>,
    mut phone: Option<crate::paired_phone::Phone>,
    services: crate::device_io::Services,
    ready: oneshot::Sender<()>,
    mut stop: watch::Receiver<()>,
) -> Result<()> {
    let first = tokio::select! {biased; _=stop.changed()=>return Ok(()), msg=tokio::time::timeout(std::time::Duration::from_secs(8),socket.next())=>msg?};
    let Some(Ok(Frame::Binary(first))) = first else {
        return Err("本机桌面已有使用中的任务".into());
    };
    HostDesktopReply::decode(first)?;
    if ready.send(()).is_err() {
        return Ok(());
    }
    #[cfg(not(target_os = "android"))]
    let (authority, live) = watch::channel(());
    #[cfg(not(target_os = "android"))]
    let mut views = Views::default();
    let mut streams = tokio::task::JoinSet::new();
    let (send_notice, mut notices) = tokio::sync::mpsc::channel(16);
    if services.notices {
        streams.spawn(crate::device_notices::watch(
            phone.as_ref().map(|p| p.client.clone()),
            send_notice,
        ));
    } else {
        drop(send_notice);
    }
    let mut notices_open = true;
    let mut session_location = None;
    #[cfg(target_os = "linux")]
    let location_http = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    loop {
        let received = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = streams.join_next(), if !streams.is_empty() => continue,
            n=notices.recv(),if notices_open=>{if let Some(notification)=n{
                let item=HostDesktopReply{notification:Some(notification),..Default::default()};
                socket.send(Frame::Binary(item.encode_to_vec().into())).await?;
            }else{notices_open=false;}continue;},
            received = socket.next() => received,
        };
        let Some(msg) = received else {
            break;
        };
        let bytes = match msg? {
            Frame::Binary(bytes) => bytes,
            Frame::Close(_) => break,
            _ => continue,
        };
        let action = DesktopActionRequest::decode(bytes)?;
        let id = action.action_id.clone();
        let observing = action.kind == "observe";
        let result = if let Some(io) = action.io {
            let services = services.clone();
            let token = id.clone();
            streams.spawn(async move {
                if let Err(e) = services.serve(&token, io).await {
                    eprintln!("Device stream: {e}");
                }
            });
            Ok(HostDesktopReply::default())
        } else {
            let execute_input = async {
                if let Some(phone) = phone.as_mut() {
                    Ok(phone.perform(&action).await)
                } else {
                    #[cfg(target_os = "android")]
                    {
                        Result::Ok(Err("Phone control session has no device".into()))
                    }
                    #[cfg(not(target_os = "android"))]
                    if let Some(fix) = &action.location {
                        #[cfg(target_os = "linux")]
                        {
                            let result = async {
                                let response = location_http
                                    .post("http://127.0.0.1:17867/fix")
                                    .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
                                    .body(fix.encode_to_vec())
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .bytes()
                                    .await?;
                                let ack = ai_phone_api::wire::LocationDevice::decode(response)?;
                                if !ack.error.is_empty() {
                                    return Err(ack.error.into());
                                }
                                if ack.status != if fix.enabled { "applied" } else { "disabled" } {
                                    return Err("设备未确认定位".into());
                                }
                                Result::Ok(HostDesktopReply {
                                    location: Some(*fix),
                                    location_scope: "browser".into(),
                                    ..Default::default()
                                })
                            }
                            .await;
                            Ok(result.map_err(|e| e.to_string()))
                        }
                        #[cfg(not(target_os = "linux"))]
                        {
                            Ok(Ok(HostDesktopReply {
                                location: Some(*fix),
                                location_scope: "workspace".into(),
                                ..Default::default()
                            }))
                        }
                    } else {
                        let live = live.clone();
                        let mut view = if action.kind == "observe" {
                            None
                        } else {
                            views.take(&action)
                        };
                        let (action, next, result) = tokio::task::spawn_blocking(move || {
                            let result =
                                execute(&mut view, &action, &live).map_err(|e| e.to_string());
                            (action, view, result)
                        })
                        .await?;
                        if let Some(next) = next {
                            views.remember(&action, next.id.clone(), next);
                        }
                        Result::Ok(result)
                    }
                }
            };
            tokio::pin!(execute_input);
            let receipt = loop {
                tokio::select! {biased;
                    _=stop.changed()=>break None,
                    incoming=socket.next()=>match incoming.transpose()?{Some(Frame::Ping(_) | Frame::Pong(_))=>continue,_=>break None},
                    result=&mut execute_input=>break Some(result?),
                }
            };
            let Some(result) = receipt else {
                break;
            };
            result
        };
        let mut reply = result.unwrap_or_else(|error| HostDesktopReply {
            error,
            ..Default::default()
        });
        if let Some(fix) = reply.location {
            session_location = fix.enabled.then(|| (fix, reply.location_scope.clone()));
        } else if observing && let Some((fix, scope)) = &session_location {
            reply.location = Some(*fix);
            reply.location_scope.clone_from(scope);
        }
        reply.action_id = id;
        if stop.has_changed().is_err() {
            break;
        }
        socket
            .send(Frame::Binary(reply.encode_to_vec().into()))
            .await?;
    }
    #[cfg(not(target_os = "android"))]
    drop(authority);
    streams.shutdown().await;
    let _ = socket.flush().await;
    let _ = socket.close(None).await;
    Ok(())
}
