#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use super::GUEST_PORT;
use super::{LIMIT, NATIVE_PORTS};
use crate::{Error, Result, linux_vm, native_ipc};
use ai_phone_api::wire::{
    DeviceBridgePeerOwner, LinuxVmControlReply, LinuxVmControlRequest, LinuxVmNativeCredentials,
    LinuxVmNativeService, LinuxVmOperation,
};
use prost::Message;
use ring::hmac;
use std::{net::Ipv4Addr, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
};

const BOOTSTRAP: &str = "/usr/local/libexec/aiphone-bootstrap";
const BUSYBOX: &str = "/opt/aiphone/engine/bin/busybox";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) async fn listen() -> Result<TcpListener> {
    Ok(TcpListener::bind((Ipv4Addr::new(10, 0, 2, 15), GUEST_PORT)).await?)
}

pub(crate) async fn run(listener: TcpListener, key: hmac::Key) -> Result<()> {
    let key = Arc::new(key);
    loop {
        let (stream, _) = listener.accept().await?;
        let key = key.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(stream, &key).await {
                eprintln!("AI_PHONE_QEMU_CONTROL {}", error.code());
            }
        });
    }
}

async fn admit(
    stream: &mut TcpStream,
    key: &hmac::Key,
    nonce: &[u8],
) -> Result<LinuxVmControlRequest> {
    let hello: LinuxVmControlRequest = native_ipc::read(stream).await?;
    let client_nonce: [u8; 32] = hello
        .payload
        .try_into()
        .map_err(|_| Error::Invalid("vm_control_nonce"))?;
    native_ipc::write(
        stream,
        &LinuxVmControlReply {
            payload: nonce.to_vec(),
            proof: linux_vm::proof(key, b"control-challenge", &client_nonce, nonce),
            error: String::new(),
        },
    )
    .await?;
    let request: LinuxVmControlRequest = native_ipc::read_bounded(stream, LIMIT).await?;
    linux_vm::verify(
        key,
        &request.operation.to_be_bytes(),
        nonce,
        &request.payload,
        &request.proof,
    )?;
    Ok(request)
}

async fn reply(
    stream: &mut TcpStream,
    key: &hmac::Key,
    nonce: &[u8],
    result: Result<Vec<u8>>,
) -> Result<()> {
    let response = match result {
        Ok(payload) => LinuxVmControlReply {
            proof: linux_vm::proof(key, b"control-reply", nonce, &payload),
            payload,
            error: String::new(),
        },
        Err(error) => LinuxVmControlReply {
            proof: linux_vm::proof(key, b"control-error", nonce, error.code().as_bytes()),
            payload: Vec::new(),
            error: error.code().into(),
        },
    };
    native_ipc::write(stream, &response).await?;
    Ok(())
}

async fn installation() -> Result<std::path::PathBuf> {
    let release = tokio::fs::read_to_string("/opt/aiphone/bundle/source.sha256").await?;
    let release = release.trim();
    if !crate::manifest::identity(release) {
        return Err(Error::Invalid("source_resource_identity"));
    }
    Ok(std::path::Path::new("/var/lib/aiphone/installation/releases").join(release))
}

async fn native_credentials() -> Result<Vec<u8>> {
    let data = installation().await?.join("data");
    let (assistant_token, phone_token) = tokio::try_join!(
        tokio::fs::read_to_string(data.join("assistant/token")),
        tokio::fs::read_to_string(data.join("phone-runner/token")),
    )?;
    Ok(LinuxVmNativeCredentials {
        assistant_token,
        phone_token,
    }
    .encode_to_vec())
}

async fn finish_devices(payload: &[u8]) -> Result<Vec<u8>> {
    use ai_phone_api::wire::{
        NativeAndroidInputSetup, NativeWslDeviceSetupRequest, NativeWslDevicesReady,
    };
    let q = NativeWslDeviceSetupRequest::decode(payload)
        .map_err(|_| Error::Invalid("vm_device_setup_request"))?;
    if q.version != 1 {
        return Err(Error::Invalid("vm_device_setup_request"));
    }
    let root = installation().await?;
    let serial = crate::device_bridge::paired_phone_serial(&q.bridge_id)?;
    let (_end, stop) = tokio::sync::watch::channel(false);
    let phone = crate::android::input::setup(
        NativeAndroidInputSetup {
            version: 1,
            project_root: root.to_string_lossy().into_owned(),
            adb_program: "/opt/aiphone/native-phone-bridge/platform-tools/adb".into(),
            serial,
            apk_path: root
                .join("runtime/phone-apps/input.apk")
                .to_string_lossy()
                .into_owned(),
            offline_apps: true,
            observe_only: false,
        },
        stop,
    );
    let desktop = async {
        let mut command = crate::desktop_bridge::native_command(&root, "exec")?;
        let receipt = command.arg("snapshot").kill_on_drop(true).output().await?;
        if !receipt.status.success() || !receipt.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(Error::Invalid("vm_desktop_frame"));
        }
        Ok(())
    };
    tokio::try_join!(phone, desktop)?;
    crate::device_bridge::paired_phone_serial(&q.bridge_id)?;
    let folder = root.join("data/deployment");
    let path = folder.join("desktop-id");
    if !path.try_exists()? {
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| Error::Invalid("host_profile_random"))?;
        match crate::filesystem::publish_private(
            &folder,
            "desktop-id",
            hex::encode(id).as_bytes(),
            true,
        ) {
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            other => other?,
        }
    }
    Ok(NativeWslDevicesReady {
        version: 1,
        bridge_id: q.bridge_id,
        desktop_id: tokio::fs::read_to_string(path).await?,
    }
    .encode_to_vec())
}

async fn native_service(payload: &[u8]) -> Result<TcpStream> {
    let request =
        LinuxVmNativeService::decode(payload).map_err(|_| Error::Invalid("vm_native_service"))?;
    let port = u16::try_from(request.port).map_err(|_| Error::Invalid("vm_native_service"))?;
    if !NATIVE_PORTS.contains(&port) {
        return Err(Error::Invalid("vm_native_service"));
    }
    Ok(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?)
}

async fn handle(mut stream: TcpStream, key: &hmac::Key) -> Result<()> {
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|_| Error::Invalid("vm_control_nonce"))?;
    let request = tokio::time::timeout(Duration::from_secs(10), admit(&mut stream, key, &nonce))
        .await
        .map_err(|_| Error::Invalid("vm_control_timeout"))??;
    let operation = LinuxVmOperation::try_from(request.operation)
        .map_err(|_| Error::Invalid("vm_control_operation"))?;
    let flag = match operation {
        LinuxVmOperation::Ping => {
            let release = tokio::fs::read_to_string("/opt/aiphone/bundle/source.sha256")
                .await
                .map(|value| value.trim().as_bytes().to_vec())
                .map_err(Error::from);
            return reply(&mut stream, key, &nonce, release).await;
        }
        LinuxVmOperation::Shutdown => {
            // BusyBox asks PID 1 to shut down and flush the persistent guest disk.
            reply(&mut stream, key, &nonce, Ok(Vec::new())).await?;
            let status = Command::new(BUSYBOX).arg("poweroff").status().await?;
            return status
                .success()
                .then_some(())
                .ok_or(Error::Invalid("vm_guest_shutdown"));
        }
        LinuxVmOperation::SetLocation => {
            let result = async {
                let fix = ai_phone_api::wire::LocationFix::decode(request.payload.as_slice())
                    .map_err(|_| Error::Invalid("location_request"))?;
                let command =
                    crate::desktop_bridge::native_command(&installation().await?, "exec")?;
                Ok(crate::desktop_bridge::apply_location(command, &fix)
                    .await?
                    .encode_to_vec())
            }
            .await;
            return reply(&mut stream, key, &nonce, result).await;
        }
        LinuxVmOperation::NativeCredentials => {
            return reply(&mut stream, key, &nonce, native_credentials().await).await;
        }
        LinuxVmOperation::NativeService => {
            let mut service = match native_service(&request.payload).await {
                Ok(service) => service,
                Err(error) => return reply(&mut stream, key, &nonce, Err(error)).await,
            };
            reply(&mut stream, key, &nonce, Ok(Vec::new())).await?;
            tokio::io::copy_bidirectional(&mut stream, &mut service).await?;
            return Ok(());
        }
        LinuxVmOperation::PhoneStatus => {
            let observation = async {
                if request.payload.len() > native_ipc::LIMIT {
                    return Err(native_ipc::invalid().into());
                }
                let owner = DeviceBridgePeerOwner::decode(request.payload.as_slice())
                    .map_err(|_| native_ipc::invalid())?;
                let status = crate::device_bridge::guest::observe(&owner).await?;
                Ok(status.encode_to_vec())
            };
            let result = tokio::time::timeout(Duration::from_secs(15), observation)
                .await
                .unwrap_or(Err(Error::Invalid("vm_control_timeout")));
            return reply(&mut stream, key, &nonce, result).await;
        }
        LinuxVmOperation::PhonePeer => "--phone-peer",
        LinuxVmOperation::SetupDevices => {
            let result =
                tokio::time::timeout(Duration::from_secs(595), finish_devices(&request.payload))
                    .await
                    .unwrap_or(Err(Error::Invalid("vm_device_setup_timeout")));
            return reply(&mut stream, key, &nonce, result).await;
        }
        LinuxVmOperation::Unspecified => return Err(Error::Invalid("vm_control_operation")),
    };
    let mut child = Command::new(BOOTSTRAP)
        .arg(flag)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or(Error::Invalid("vm_control_child"))?;
    let mut output = child
        .stdout
        .take()
        .ok_or(Error::Invalid("vm_control_child"))?;
    input.write_u32(request.payload.len() as u32).await?;
    input.write_all(&request.payload).await?;
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let length = output.read_u32().await? as usize;
        if length > 384 {
            return Err(Error::Invalid("vm_control_child"));
        }
        let mut payload = vec![0; length];
        output.read_exact(&mut payload).await?;
        Ok(payload)
    })
    .await
    .unwrap_or(Err(Error::Invalid("vm_control_timeout")));
    let connected = result.is_ok();
    if connected {
        let result = reply(&mut stream, key, &nonce, result).await;
        if result.is_ok() {
            tokio::select! {
                _ = stream.read_u8() => {},
                _ = child.wait() => return Ok(()),
            }
        }
        drop(input);
        let _ = tokio::time::timeout(Duration::from_secs(15), child.wait()).await;
        return result;
    }
    reply(&mut stream, key, &nonce, result).await
}
