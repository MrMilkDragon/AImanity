use ai_phone_api::wire::{DeviceIoPacket, device_io_packet::Body};
use axum::extract::ws::{Message as Frame, WebSocket};
use bytes::Bytes;
use prost::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BLOCK: usize = 128 * 1024;
fn packet(body: Body) -> Bytes {
    DeviceIoPacket { body: Some(body) }.encode_to_vec().into()
}
fn unpack(bytes: Bytes) -> Result<Option<Bytes>> {
    match DeviceIoPacket::decode(bytes)?.body {
        Some(Body::Data(bytes)) if !bytes.is_empty() => Ok(Some(bytes.into())),
        Some(Body::End(true)) => Ok(None),
        Some(Body::Error(reason)) => Err(reason.into()),
        _ => Err("设备返回了无效的数据包".into()),
    }
}
pub(crate) struct Lane {
    pub(super) socket: WebSocket,
    pub(super) stop: CancellationToken,
}
impl Lane {
    pub(crate) async fn next(&mut self) -> Result<Option<Bytes>> {
        loop {
            tokio::select! {biased;
                _=self.stop.cancelled()=>return Err("设备已断开".into()),
                msg=self.socket.recv()=>match msg.transpose()? {
                    Some(Frame::Binary(bytes))=>return unpack(bytes),
                    Some(Frame::Pong(_) | Frame::Ping(_))=>continue,
                    _=>return Err("设备数据传输中断".into())
                },
            }
        }
    }
    async fn send(&mut self, body: Body) -> Result<()> {
        tokio::select! {biased; _=self.stop.cancelled()=>Err("设备已断开".into()), sent=self.socket.send(Frame::Binary(packet(body)))=>Ok(sent?)}
    }
    pub(crate) async fn receive(&mut self, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        while let Some(bytes) = self.next().await? {
            out.write_all(&bytes).await?;
        }
        Ok(())
    }
    pub(crate) async fn upload(&mut self, input: &mut (impl AsyncRead + Unpin)) -> Result<()> {
        let mut buf = vec![0; BLOCK];
        loop {
            let n = tokio::select! {biased; _=self.stop.cancelled()=>return Err("设备已断开".into()), n=input.read(&mut buf)=>n?};
            if n == 0 {
                break;
            }
            self.send(Body::Data(buf[..n].to_vec())).await?;
        }
        self.send(Body::End(true)).await?;
        self.ack().await
    }
    pub(crate) async fn ack(&mut self) -> Result<()> {
        if self.next().await?.is_some() {
            return Err("缺少设备完成回执".into());
        }
        Ok(())
    }
}

#[cfg(not(target_os = "android"))]
pub(crate) use remote::*;
#[cfg(not(target_os = "android"))]
mod remote {
    use super::*;
    use crate::{
        device_files::Endpoint,
        peer_connection::{self, Socket},
    };
    use ai_phone_api::wire::{DeviceIoOperation as Op, DeviceIoRequest};
    use futures_util::{SinkExt, StreamExt};
    use std::{io, process::Stdio};
    use tokio::process::Command;
    use tokio_tungstenite::tungstenite::{Message as Frame, client::IntoClientRequest};
    use tokio_util::io::StreamReader;

    #[derive(Clone)]
    pub(crate) struct Services {
        pub(crate) target: String,
        pub(crate) files: Endpoint,
        pub(crate) guest_audio: bool,
        pub(crate) pulse_server: Option<String>,
    }
    pub(crate) async fn has_guest_audio() -> bool {
        let has = async |kind: &str, name: &str| {
            Command::new("pactl")
                .args(["list", "short", kind])
                .kill_on_drop(true)
                .output()
                .await
                .is_ok_and(|r| {
                    r.status.success()
                        && String::from_utf8_lossy(&r.stdout)
                            .lines()
                            .any(|line| line.split_whitespace().nth(1) == Some(name))
                })
        };
        let (speaker, mic) = tokio::join!(
            has("sources", "aiphone.monitor"),
            has("sinks", "aimanity_mic_input")
        );
        speaker && mic
    }
    async fn send_reader(ws: &mut Socket, input: &mut (impl AsyncRead + Unpin)) -> Result<()> {
        let mut buf = vec![0; BLOCK];
        loop {
            let n = tokio::select! {msg=ws.next()=>match msg.transpose()? {Some(Frame::Ping(_)|Frame::Pong(_))=>continue,_=>return Err("接收端已关闭".into())},n=input.read(&mut buf)=>n?};
            if n == 0 {
                return Ok(());
            }
            ws.send(Frame::Binary(packet(Body::Data(buf[..n].to_vec()))))
                .await?;
        }
    }
    fn reader(ws: &mut Socket) -> impl AsyncRead + Unpin + '_ {
        let stream = futures_util::stream::try_unfold(ws, |ws| async move {
            loop {
                match ws.next().await.transpose().map_err(io::Error::other)? {
                    Some(Frame::Binary(b)) => {
                        return Ok::<_, io::Error>(
                            unpack(b)
                                .map_err(io::Error::other)?
                                .map(|bytes| (bytes, ws)),
                        );
                    }
                    Some(Frame::Ping(_) | Frame::Pong(_)) => continue,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "设备数据传输中断",
                        ));
                    }
                }
            }
        });
        StreamReader::new(Box::pin(stream))
    }
    impl Services {
        pub(crate) async fn serve(&self, key: &str, q: DeviceIoRequest) -> Result<()> {
            let mut req = "ws://127.0.0.1:17863/v1/peer/io".into_client_request()?;
            req.headers_mut()
                .insert("authorization", format!("Bearer {key}").parse()?);
            let (mut ws, _tunnel) = peer_connection::dial(&self.target, req).await?;
            let result = self.transfer(&mut ws, q).await;
            let body = match result {
                Ok(()) => Body::End(true),
                Err(e) => Body::Error(e.to_string()),
            };
            use tokio_tungstenite::tungstenite::Error as WsError;
            match ws.send(Frame::Binary(packet(body))).await {
                Ok(()) | Err(WsError::AlreadyClosed | WsError::ConnectionClosed) => Ok(()),
                Err(WsError::Io(e))
                    if matches!(
                        e.kind(),
                        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                    ) =>
                {
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        }
        async fn transfer(&self, ws: &mut Socket, q: DeviceIoRequest) -> Result<()> {
            let op = Op::try_from(q.operation)?;
            match op {
                Op::List => {
                    let list = self.files.list(&q.path).await?;
                    let data = list.encode_to_vec();
                    for part in data.chunks(BLOCK) {
                        ws.send(Frame::Binary(packet(Body::Data(part.to_vec()))))
                            .await?;
                    }
                }
                Op::Read => {
                    let (mut read, mut write) = tokio::io::duplex(BLOCK);
                    let produce = async {
                        self.files.read(&q.path, &mut write).await?;
                        write.shutdown().await?;
                        Result::Ok(())
                    };
                    tokio::try_join!(produce, send_reader(ws, &mut read))?;
                }
                Op::Write => self.files.write(&q.path, &mut reader(ws)).await?,
                Op::Mkdir | Op::Remove | Op::Rename => {
                    self.files.change(op, &q.path, &q.destination).await?
                }
                Op::AudioInput | Op::AudioOutput if self.guest_audio => {
                    let capture = op == Op::AudioOutput;
                    let mut cmd = Command::new(if capture {
                        "parec"
                    } else if q.wav_input {
                        "paplay"
                    } else {
                        "pacat"
                    });
                    if let Some(server) = &self.pulse_server {
                        cmd.env("PULSE_SERVER", server);
                    }
                    cmd.arg("--latency-msec=20");
                    if !q.wav_input || capture {
                        cmd.args(["--raw", "--format=s16le", "--rate=48000"]);
                    }
                    if capture {
                        let mut process = cmd
                            .args(["--channels=2", "--device=aiphone.monitor"])
                            .stdin(Stdio::null())
                            .stdout(Stdio::piped())
                            .kill_on_drop(true)
                            .spawn()?;
                        send_reader(ws, process.stdout.as_mut().ok_or("设备播放通道不可用")?)
                            .await?;
                        if !process.wait().await?.success() {
                            return Err("设备音频采集中断".into());
                        }
                    } else {
                        if !q.wav_input {
                            cmd.args(["--playback", "--channels=1"]);
                        }
                        let mut process = cmd
                            .arg("--device=aimanity_mic_input")
                            .stdin(Stdio::piped())
                            .stdout(Stdio::null())
                            .kill_on_drop(true)
                            .spawn()?;
                        let mut writer = process.stdin.take().ok_or("设备麦克风不可用")?;
                        tokio::io::copy(&mut reader(ws), &mut writer).await?;
                        drop(writer);
                        if !process.wait().await?.success() {
                            return Err("设备麦克风写入失败".into());
                        }
                    }
                }
                _ => return Err("设备未提供这项能力".into()),
            }
            Ok(())
        }
    }
}
