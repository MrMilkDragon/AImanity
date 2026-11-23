//! Authenticated, loopback-forwarded control of the owned Linux VM.
#[cfg(any(test, target_os = "linux"))]
pub(crate) mod guest;

use crate::{Error, Result, linux_vm, native_ipc};
use ai_phone_api::wire::{
    LinuxVmControlReply, LinuxVmControlRequest, LinuxVmOperation, NativeWslDeviceSetupRequest,
    NativeWslDevicesReady,
};
use prost::Message;
use std::{net::Ipv4Addr, path::Path, time::Duration};
use tokio::net::TcpStream;

pub const GUEST_PORT: u16 = 17861;
const LIMIT: usize = 16448;
#[cfg(any(test, windows, target_os = "android", target_os = "linux"))]
const NATIVE_PORTS: [u16; 4] = [17862, 17863, 17864, 17865];

pub async fn native_credentials(
    key: &Path,
) -> Result<ai_phone_api::wire::LinuxVmNativeCredentials> {
    let (_, bytes) = tokio::time::timeout(
        Duration::from_secs(15),
        open(
            GUEST_PORT,
            key,
            LinuxVmOperation::NativeCredentials,
            Vec::new(),
        ),
    )
    .await
    .map_err(|_| Error::Invalid("vm_control_timeout"))??;
    ai_phone_api::wire::LinuxVmNativeCredentials::decode(bytes.as_slice())
        .map_err(|_| Error::Invalid("vm_native_credentials"))
}

/// Bind the four local data-plane sockets for the client lifetime.
#[cfg(any(
    windows,
    target_os = "android",
    all(target_os = "linux", target_arch = "x86_64")
))]
pub async fn expose_native_services(key: &Path) -> Result<tokio::task::JoinSet<()>> {
    use ai_phone_api::wire::LinuxVmNativeService;
    use std::sync::Arc;
    use tokio::{net::TcpListener, task::JoinSet};

    let key = Arc::new(key.to_owned());
    let mut services = JoinSet::new();
    for port in NATIVE_PORTS {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let key = Arc::clone(&key);
        let payload = LinuxVmNativeService { port: port.into() }.encode_to_vec();
        services.spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    connection = listener.accept() => {
                        let Ok((mut stream, _)) = connection else { return };
                        let key = Arc::clone(&key);
                        let payload = payload.clone();
                        clients.spawn(async move {
                            let upstream = tokio::time::timeout(
                                Duration::from_secs(15),
                                open(GUEST_PORT, &key, LinuxVmOperation::NativeService, payload),
                            ).await;
                            if let Ok(Ok((mut upstream, _))) = upstream {
                                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                            }
                        });
                    }
                    _ = clients.join_next(), if !clients.is_empty() => {}
                }
            }
        });
    }
    Ok(services)
}

pub(crate) async fn open(
    port: u16,
    key_file: &Path,
    operation: LinuxVmOperation,
    payload: Vec<u8>,
) -> Result<(TcpStream, Vec<u8>)> {
    let key = linux_vm::read_key(key_file)?;
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
    let mut client_nonce = [0; 32];
    getrandom::fill(&mut client_nonce).map_err(|_| Error::Invalid("vm_control_nonce"))?;
    native_ipc::write(
        &mut stream,
        &LinuxVmControlRequest {
            payload: client_nonce.to_vec(),
            ..Default::default()
        },
    )
    .await?;
    let challenge: LinuxVmControlReply = native_ipc::read(&mut stream).await?;
    let nonce: [u8; 32] = challenge
        .payload
        .try_into()
        .map_err(|_| Error::Invalid("vm_control_nonce"))?;
    linux_vm::verify(
        &key,
        b"control-challenge",
        &client_nonce,
        &nonce,
        &challenge.proof,
    )?;
    let proof = linux_vm::proof(&key, &(operation as i32).to_be_bytes(), &nonce, &payload);
    native_ipc::write_bounded(
        &mut stream,
        &LinuxVmControlRequest {
            operation: operation as i32,
            payload,
            proof,
        },
        LIMIT,
    )
    .await?;
    let reply: LinuxVmControlReply = native_ipc::read(&mut stream).await?;
    if reply.error.is_empty() {
        linux_vm::verify(&key, b"control-reply", &nonce, &reply.payload, &reply.proof)?;
        Ok((stream, reply.payload))
    } else {
        linux_vm::verify(
            &key,
            b"control-error",
            &nonce,
            reply.error.as_bytes(),
            &reply.proof,
        )?;
        eprintln!("AI_PHONE_QEMU_CONTROL_ERROR {}", reply.error);
        Err(Error::Invalid("vm_control_guest_failed"))
    }
}

async fn empty(port: u16, key: &Path, operation: LinuxVmOperation) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(15),
        open(port, key, operation, Vec::new()),
    )
    .await
    .map_err(|_| Error::Invalid("vm_control_timeout"))??;
    Ok(())
}

#[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
pub(crate) async fn version(port: u16, key: &Path) -> Result<Vec<u8>> {
    let (_, payload) = tokio::time::timeout(
        Duration::from_secs(15),
        open(port, key, LinuxVmOperation::Ping, Vec::new()),
    )
    .await
    .map_err(|_| Error::Invalid("vm_control_timeout"))??;
    Ok(payload)
}

pub async fn shutdown(port: u16, key: &Path) -> Result<()> {
    empty(port, key, LinuxVmOperation::Shutdown).await
}

pub async fn complete_devices(
    port: u16,
    key: &Path,
    bridge_id: &str,
) -> Result<NativeWslDevicesReady> {
    let request = NativeWslDeviceSetupRequest {
        version: 1,
        bridge_id: bridge_id.into(),
    };
    let (_, bytes) = tokio::time::timeout(
        Duration::from_secs(600),
        open(
            port,
            key,
            LinuxVmOperation::SetupDevices,
            request.encode_to_vec(),
        ),
    )
    .await
    .map_err(|_| Error::Invalid("vm_device_setup_unknown"))??;
    crate::wsl::devices::decode_receipt(&bytes, bridge_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_phone_api::wire::LinuxVmNativeService;

    #[tokio::test]
    async fn native_vm_control_cannot_relay_an_arbitrary_port_or_accept_a_foreign_key() {
        let key = [13; 32];
        let owned = tempfile::NamedTempFile::new().unwrap();
        let foreign = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(owned.path(), key).unwrap();
        std::fs::write(foreign.path(), [17; 32]).unwrap();
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let guest = tokio::spawn(guest::run(
            listener,
            ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key),
        ));
        assert!(matches!(
            open(
                port,
                owned.path(),
                LinuxVmOperation::NativeService,
                LinuxVmNativeService { port: 5432 }.encode_to_vec()
            )
            .await,
            Err(Error::Invalid("vm_control_guest_failed"))
        ));
        assert!(
            open(
                port,
                foreign.path(),
                LinuxVmOperation::NativeCredentials,
                Vec::new()
            )
            .await
            .is_err()
        );
        guest.abort();
    }
}
