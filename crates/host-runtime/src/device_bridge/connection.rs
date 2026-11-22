//! Own the host bridge and its authenticated QEMU guest connection.
use super::{Bridge, config::Config};
use crate::{Error, Result, vm_control};
use ai_phone_api::wire::{
    DeviceBridgeConfig, DeviceBridgePhone, DeviceBridgeRole, DeviceBridgeStarted, LinuxVmOperation,
};
use prost::Message;
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::watch,
    task::JoinHandle,
};

pub struct Connection {
    peer: TcpStream,
    end: watch::Sender<bool>,
    bridge: Option<JoinHandle<Result<()>>>,
}

impl Connection {
    pub(crate) async fn start(
        mut guest: DeviceBridgePhone,
        port: u16,
        key: &Path,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<Self> {
        if *stop.borrow() {
            return Err(Error::Cancelled);
        }
        let config = guest
            .transport
            .as_mut()
            .ok_or(Error::Invalid("phone_peer_configuration"))?;
        let adb_only = guest.serial == super::peer::CUTTLEFISH_SERIAL;
        let mut host_config = Config::try_from(DeviceBridgeConfig {
            version: config.version,
            role: DeviceBridgeRole::Host as i32,
            bridge_id: config.bridge_id.clone(),
            secret: config.secret.clone(),
            endpoint: "127.0.0.1:0".into(),
            adb_port: 0,
            emulator_port: 0,
        })?;
        host_config.adb_only = adb_only;
        let host = Bridge::bind_config(host_config).await?;
        let address: SocketAddr = host
            .receipt()
            .endpoint
            .parse()
            .map_err(|_| Error::Invalid("phone_peer_configuration"))?;
        config.endpoint =
            SocketAddr::from((Ipv4Addr::new(10, 0, 2, 2), address.port())).to_string();
        let checked = Config::try_from(config.clone())?;
        if checked.role != DeviceBridgeRole::Guest {
            return Err(Error::Invalid("phone_peer_configuration"));
        }
        let expected = DeviceBridgeStarted {
            version: 1,
            role: DeviceBridgeRole::Guest as i32,
            bridge_id: checked.id,
            endpoint: checked.endpoint.to_string(),
            adb_port: checked.ports[0].into(),
            emulator_port: if adb_only { 0 } else { checked.ports[1].into() },
        };
        let (end, receiver) = watch::channel(false);
        let bridge = tokio::spawn(host.serve(receiver));
        let exchange = async {
            let (stream, bytes) = vm_control::open(
                port,
                key,
                LinuxVmOperation::PhonePeer,
                guest.encode_to_vec(),
            )
            .await?;
            let reply = DeviceBridgeStarted::decode(bytes.as_slice())
                .map_err(|_| Error::Invalid("phone_peer_start_receipt"))?;
            if reply != expected {
                return Err(Error::Invalid("phone_peer_start_receipt"));
            }
            Ok(stream)
        };
        let result = tokio::select! { biased;
            _ = stop.changed() => Err(Error::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(15), exchange) =>
                result.unwrap_or(Err(Error::Invalid("phone_peer_start_unknown"))),
        };
        match result {
            Ok(peer) => Ok(Self {
                peer,
                end,
                bridge: Some(bridge),
            }),
            Err(error) => {
                let _ = end.send(true);
                let _ = bridge.await;
                Err(error)
            }
        }
    }

    pub async fn wait(&mut self) -> Result<()> {
        let bridge = self
            .bridge
            .as_mut()
            .ok_or(Error::Invalid("device_bridge_worker"))?;
        tokio::select! {
            _ = self.peer.read_u8() => Err(Error::Invalid("phone_peer_exited")),
            result = bridge => {
                self.bridge = None;
                result.map_err(|_| Error::Invalid("device_bridge_worker"))??;
                Err(Error::Invalid("device_bridge_worker"))
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let peer = async {
            self.peer.shutdown().await?;
            let mut byte = [0];
            if matches!(
                tokio::time::timeout(Duration::from_secs(15), self.peer.read(&mut byte)).await,
                Ok(Ok(0))
            ) {
                Ok(())
            } else {
                Err(Error::Invalid("phone_peer_stop_unknown"))
            }
        }
        .await;
        let _ = self.end.send(true);
        let bridge = if let Some(task) = self.bridge.take() {
            task.await
                .map_err(|_| Error::Invalid("device_bridge_worker"))?
        } else {
            Ok(())
        };
        peer.and(bridge)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.end.send(true);
    }
}
