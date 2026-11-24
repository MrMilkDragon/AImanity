use super::*;
#[cfg(target_os = "android")]
use crate::android_host::attached_client;
use crate::{paired_phone, peer_connection};
#[cfg(not(target_os = "android"))]
use ai_phone_host_runtime::android::session::attached_client;
use ai_phone_host_runtime::guest_channel::{self, Service};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Weak,
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinSet};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message as Frame, protocol::Role},
};

pub(super) async fn watch(
    work: Weak<Workspace>,
    mut changes: watch::Receiver<u64>,
    stop: CancellationToken,
) {
    let mut live = HashMap::<String, CancellationToken>::new();
    let mut jobs = JoinSet::new();
    loop {
        let Some(w) = work.upgrade() else { break };
        let Ok(registry) = w.registry() else { break };
        let eligible: HashMap<_, _> = registry
            .devices
            .into_iter()
            .filter(|d| {
                !d.paired
                    && matches!(
                        d.runtime_state.as_str(),
                        "configured" | "managed" | "booted"
                    )
            })
            .map(|d| (d.id, d.kind))
            .collect();
        live.retain(|id, token| {
            if eligible.contains_key(id) {
                true
            } else {
                token.cancel();
                false
            }
        });
        for (id, kind) in eligible {
            if live.contains_key(&id) {
                continue;
            }
            let token = stop.child_token();
            let root = w.device_home(&id);
            live.insert(id.clone(), token.clone());
            let w = work.clone();
            jobs.spawn(async move {
                loop {
                    let Some(workspace) = w.upgrade() else { break };
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => break,
                        result = attach(&workspace, &id, &kind, &root) => {
                            if let Err(error) = result {
                                workspace.peers.owned_failure(&workspace, &id, error.to_string());
                            }
                        }
                    }
                    drop(workspace);
                    tokio::select! {
                        _ = token.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    }
                }
            });
        }
        drop(w);
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            change = changes.changed() => { if change.is_err() { break; } },
            Some(_) = jobs.join_next(), if !jobs.is_empty() => {}
        }
    }
    for (_, t) in live {
        t.cancel();
    }
    while jobs.join_next().await.is_some() {}
}

async fn attach(w: &Arc<Workspace>, id: &str, kind: &str, home: &Path) -> Result<()> {
    if kind == "phone" {
        let Some(adb) = attached_client(home)? else {
            return Ok(());
        };
        let boot = adb
            .shell(
                "getprop sys.boot_completed",
                ai_phone_adb::Limits {
                    stdout: 32,
                    stderr: 256,
                },
                Duration::from_secs(4),
            )
            .await?;
        if boot.code != 0 || boot.stdout.trim_ascii() != b"1" {
            return Ok(());
        }
        let addr = w.peers.listen(w, peers::PORT).await?;
        let code = w.peers.owned_ticket(w, id)?;
        let (_end, stop) = watch::channel(());
        let (ready, up) = oneshot::channel();
        let port = addr.port().to_string();
        let ticket = code.to_string();
        let peer = peer_connection::share_device(
            &port,
            &ticket,
            Some(paired_phone::Phone::owned(adb)),
            true,
            ready,
            stop,
        );
        tokio::pin!(peer);
        tokio::select! {
            result = &mut peer => return result,
            started = up => started?,
        }
        loop {
            tokio::select! {
                result = &mut peer => return result,
                _ = tokio::time::sleep(Duration::from_secs(3)) => {
                    if attached_client(home)?.is_none() {
                        return Err("手机已关机".into());
                    }
                }
            }
        }
    }
    let Some((pipe, _owner)) = tokio::time::timeout(
        Duration::from_secs(12),
        guest_channel::open(home, Service::Control),
    )
    .await??
    else {
        return Ok(());
    };
    let mut guest = WebSocketStream::from_raw_socket(pipe, Role::Client, None).await;
    let Some(Ok(Frame::Binary(hello))) =
        tokio::time::timeout(Duration::from_secs(12), guest.next()).await?
    else {
        return Err("Linux 桌面未就绪".into());
    };
    wire::HostDesktopReply::decode(hello)?;
    let addr = w.peers.listen(w, peers::PORT).await?;
    let target = addr.port().to_string();
    let code = w.peers.owned_ticket(w, id)?;
    let request = peer_connection::registration(&code.to_string(), "linux", true)?;
    let (mut hub, _tunnel) = peer_connection::dial(&target, request).await?;
    let Some(Ok(Frame::Binary(hello))) = hub.next().await else {
        return Err("工作区已关闭".into());
    };
    wire::HostDesktopReply::decode(hello)?;
    let mut transfers = JoinSet::new();
    loop {
        tokio::select! {
            message = hub.next() => match message.transpose()? {
                Some(Frame::Binary(bytes)) => {
                    let request = wire::DesktopActionRequest::decode(bytes.clone())?;
                    if let Some(io) = request.io {
                        let id = request.action_id;
                        transfers.spawn(relay_io(home.to_owned(), target.clone(), id.clone(), io));
                        let ack = wire::HostDesktopReply { action_id: id, ..Default::default() };
                        hub.send(Frame::Binary(ack.encode_to_vec().into())).await?;
                    } else {
                        guest.send(Frame::Binary(bytes)).await?;
                    }
                }
                Some(Frame::Close(_)) | None => break,
                _ => {}
            },
            message = guest.next() => match message.transpose()? {
                Some(Frame::Binary(bytes)) => hub.send(Frame::Binary(bytes)).await?,
                Some(Frame::Close(_)) | None => return Err("Linux 桌面连接已结束".into()),
                _ => {}
            },
            Some(result) = transfers.join_next(), if !transfers.is_empty() => {
                if let Ok(Err(error)) = result {
                    eprintln!("Guest file/audio transfer: {error}");
                }
            }
        }
    }
    Ok(())
}

async fn relay_io(
    home: PathBuf,
    target: String,
    key: String,
    io: wire::DeviceIoRequest,
) -> Result<()> {
    let (mut hub, _tunnel) = peer_connection::io_socket(&target, &key).await?;
    let transfer = async {
        let (pipe, _owner) = guest_channel::open(&home, Service::FilesAndAudio)
            .await?
            .ok_or("Linux 设备已断开")?;
        let mut guest = WebSocketStream::from_raw_socket(pipe, Role::Client, None).await;
        guest.send(Frame::Binary(io.encode_to_vec().into())).await?;
        loop {
            tokio::select! {
                message = hub.next() => match message.transpose()? {
                    Some(Frame::Binary(bytes)) => guest.send(Frame::Binary(bytes)).await?,
                    Some(Frame::Close(_)) | None => break,
                    _ => {}
                },
                message = guest.next() => match message.transpose()? {
                    Some(Frame::Binary(bytes)) => {
                        use wire::device_io_packet::Body;
                        let packet = wire::DeviceIoPacket::decode(bytes.clone())?;
                        let end = matches!(packet.body, Some(Body::End(true) | Body::Error(_)));
                        hub.send(Frame::Binary(bytes)).await?;
                        if end { break; }
                    }
                    Some(Frame::Close(_)) | None => return Err("Linux 文件或音频传输中断".into()),
                    _ => {}
                }
            }
        }
        Result::Ok(())
    }
    .await;
    if let Err(e) = &transfer {
        let _ = hub
            .send(Frame::Binary(aimanity_device_io::packet(
                wire::device_io_packet::Body::Error(e.to_string()),
            )))
            .await;
    }
    transfer
}
