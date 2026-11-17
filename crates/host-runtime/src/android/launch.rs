//! Start the fixed native component; viewport changes never own its lifetime.
use super::{Engine, Prepared};
use crate::native_ipc;
use ai_phone_api::wire::{
    NativeAndroidBooted, NativeAndroidSessionControl, NativeAndroidSessionReply,
    NativeAndroidStart, NativeAndroidVmEndpoint, native_android_session_reply::Result as Reply,
};
use std::{future::Future, io, pin::pin, process::Stdio, time::Duration};
use tokio::{
    process::{Child, ChildStdin, Command},
    sync::oneshot,
};

pub async fn cancelled(test: &dyn Fn() -> bool) {
    while !test() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
pub async fn start(
    phone: &Prepared,
    linux_vm: NativeAndroidVmEndpoint,
    linux_ready: oneshot::Receiver<()>,
    cancel: &dyn Fn() -> bool,
) -> Result<NativeAndroidBooted, String> {
    if cancel() {
        return Err("installation_cancelled".into());
    }
    let engine = Engine::installed(&phone.sdk_root);
    let request = NativeAndroidStart {
        phone: Some(phone.wire().map_err(|e| e.code().to_string())?),
        linux_vm: Some(linux_vm),
        ..Default::default()
    };
    let mut command = Command::new(std::env::current_exe().map_err(|_| "android_session_program")?);
    command
        .arg("--android-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|_| "android_session_start")?;
    // Always close both pipes before reaping: EOF unblocks the worker's stdin.
    // A missing/truncated reply cannot prove whether the phone already started.
    let serial = engine.serial(5564);
    let receipt = exchange(
        &mut child,
        &request,
        &serial,
        engine.boot_timeout(),
        linux_ready,
        cancel,
    )
    .await;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    interpret(receipt?, cancel(), &serial)
}

async fn exchange(
    child: &mut Child,
    request: &NativeAndroidStart,
    serial: &str,
    boot_timeout: Duration,
    linux_ready: oneshot::Receiver<()>,
    cancel: &dyn Fn() -> bool,
) -> Result<NativeAndroidSessionReply, &'static str> {
    let mut input = child
        .stdin
        .take()
        .ok_or("android_session_outcome_unknown")?;
    let mut output = child
        .stdout
        .take()
        .ok_or("android_session_outcome_unknown")?;
    native_ipc::write_bounded(&mut input, request, 16384)
        .await
        .map_err(|_| "android_session_outcome_unknown")?;
    let boot = wait_for_reply(
        &mut input,
        native_ipc::read(&mut output),
        boot_timeout + Duration::from_secs(30),
        cancel,
    )
    .await?;
    match &boot.result {
        Some(Reply::Booted(receipt)) if valid_boot(receipt, serial) && !receipt.backend_paired => {}
        Some(Reply::Failure(_)) => return Ok(boot),
        _ => return Err("android_session_outcome_unknown"),
    }
    let mut reply = pin!(native_ipc::read(&mut output));
    let pair = tokio::select! { biased;
        _ = cancelled(cancel) => false,
        value = &mut reply => return value.map_err(|_| "android_session_outcome_unknown"),
        result = linux_ready => result.is_ok(),
    };
    if !pair {
        return cancel_startup(&mut input, reply).await;
    }
    native_ipc::write(
        &mut input,
        &NativeAndroidSessionControl {
            cancel_startup: false,
            linux_ready: true,
        },
    )
    .await
    .map_err(|_| "android_session_outcome_unknown")?;
    wait_for_reply(&mut input, reply, Duration::from_secs(30), cancel).await
}

async fn wait_for_reply(
    input: &mut ChildStdin,
    reply: impl Future<Output = io::Result<NativeAndroidSessionReply>>,
    deadline: Duration,
    cancel: &dyn Fn() -> bool,
) -> Result<NativeAndroidSessionReply, &'static str> {
    let mut reply = pin!(reply);
    tokio::select! { biased;
        _ = cancelled(cancel) => {
            cancel_startup(input, reply).await?;
            Err("installation_cancelled")
        }
        value = &mut reply => value.map_err(|_| "android_session_outcome_unknown"),
        _ = tokio::time::sleep(deadline) => {
            cancel_startup(input, reply).await?;
            Err("android_session_outcome_unknown")
        }
    }
}

async fn cancel_startup(
    input: &mut ChildStdin,
    reply: impl Future<Output = io::Result<NativeAndroidSessionReply>>,
) -> Result<NativeAndroidSessionReply, &'static str> {
    native_ipc::write(
        input,
        &NativeAndroidSessionControl {
            cancel_startup: true,
            linux_ready: false,
        },
    )
    .await
    .map_err(|_| "android_session_outcome_unknown")?;
    tokio::time::timeout(Duration::from_secs(20), reply)
        .await
        .map_err(|_| "android_session_outcome_unknown")?
        .map_err(|_| "android_session_outcome_unknown")
}

fn valid_boot(boot: &NativeAndroidBooted, serial: &str) -> bool {
    boot.version == 1
        && boot.root_verified
        && boot.serial == serial
        && [boot.width, boot.height]
            .into_iter()
            .all(|value| (1..=8192).contains(&value))
}

fn interpret(
    receipt: NativeAndroidSessionReply,
    cancelled: bool,
    serial: &str,
) -> Result<NativeAndroidBooted, String> {
    match receipt.result {
        Some(Reply::Booted(boot))
            if valid_boot(&boot, serial)
                && boot.backend_paired
                && boot.backend_bridge_id.len() == 32
                && boot
                    .backend_bridge_id
                    .bytes()
                    .all(|v| v.is_ascii_hexdigit()) =>
        {
            if cancelled {
                Err("installation_cancelled".into())
            } else {
                Ok(boot)
            }
        }
        Some(Reply::Failure(failure)) => Err(if failure.outcome_unknown {
            "android_session_outcome_unknown".into()
        } else {
            failure.code
        }),
        _ => Err("android_session_outcome_unknown".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_initialization_never_launches_a_worker() {
        let phone = Prepared {
            sdk_root: "unused-sdk".into(),
            avd_home: "unused-avd".into(),
            reused_sdk: false,
            reused_device: false,
        };
        assert_eq!(
            start(
                &phone,
                NativeAndroidVmEndpoint::default(),
                oneshot::channel().1,
                &|| true
            )
            .await,
            Err("installation_cancelled".into())
        );
    }
    #[test]
    fn missing_unrooted_or_other_device_receipts_never_publish_ready() {
        for boot in [
            NativeAndroidBooted::default(),
            NativeAndroidBooted {
                version: 1,
                serial: "emulator-5582".into(),
                root_verified: true,
                width: 720,
                height: 1440,
                reused_session: false,
                backend_paired: false,
                backend_bridge_id: String::new(),
            },
        ] {
            assert_eq!(
                interpret(
                    NativeAndroidSessionReply {
                        result: Some(Reply::Booted(boot))
                    },
                    false,
                    "emulator-5564",
                ),
                Err("android_session_outcome_unknown".into())
            );
        }
        let unknown = NativeAndroidSessionReply {
            result: Some(Reply::Failure(
                ai_phone_api::wire::NativeAndroidSessionFailure {
                    code: "primary_failure".into(),
                    outcome_unknown: true,
                },
            )),
        };
        assert_eq!(
            interpret(unknown, false, "emulator-5564"),
            Err("android_session_outcome_unknown".into())
        );
    }
    #[test]
    fn a_local_boot_does_not_satisfy_a_requested_backend_pair() {
        let mut boot = NativeAndroidBooted {
            version: 1,
            serial: "emulator-5564".into(),
            root_verified: true,
            width: 720,
            height: 1440,
            ..Default::default()
        };
        let reply = |boot| NativeAndroidSessionReply {
            result: Some(Reply::Booted(boot)),
        };
        assert_eq!(
            interpret(reply(boot.clone()), false, "emulator-5564"),
            Err("android_session_outcome_unknown".into())
        );
        boot.backend_paired = true;
        boot.backend_bridge_id = "a".repeat(32);
        assert_eq!(
            interpret(reply(boot.clone()), false, "emulator-5564").unwrap(),
            boot
        );
        assert_eq!(
            interpret(reply(boot), true, "emulator-5564"),
            Err("installation_cancelled".into())
        );
    }
}
