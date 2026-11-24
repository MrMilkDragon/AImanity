use ai_phone_api::wire::device_io_packet::Body;
pub(crate) use aimanity_device_io::{BLOCK, Result, packet, unpack};
use axum::extract::ws::{Message as Frame, WebSocket};
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
pub(crate) struct Lane {
    pub(super) socket: WebSocket,
    pub(super) stop: CancellationToken,
    pub(crate) _microphone: Option<tokio::sync::OwnedSemaphorePermit>,
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

pub(crate) use remote::*;
mod remote {
    use super::*;
    use crate::{device_files::Endpoint, peer_connection};
    use ai_phone_api::wire::DeviceIoRequest;
    pub(crate) use aimanity_device_io::has_guest_audio;
    #[derive(Clone)]
    pub(crate) struct Services {
        pub(crate) target: String,
        pub(crate) files: Endpoint,
        pub(crate) guest_audio: bool,
        pub(crate) notices: bool,
        pub(crate) pulse_server: Option<String>,
    }
    impl Services {
        pub(crate) async fn serve(&self, key: &str, q: DeviceIoRequest) -> Result<()> {
            let (mut ws, _tunnel) = peer_connection::io_socket(&self.target, key).await?;
            aimanity_device_io::StorageMedia {
                files: self.files.clone(),
                guest_audio: self.guest_audio,
                pulse_server: self.pulse_server.clone(),
            }
            .serve(&mut ws, q)
            .await
        }
    }
}
