use super::{
    plan::Plan,
    process::{self, Started},
};
use crate::{Error, Result, native_ipc as transport};
use ai_phone_api::wire::{
    NativeAndroidBooted, NativeAndroidSessionControl, NativeAndroidSessionFailure,
    NativeAndroidSessionReply, NativeAndroidStart, native_android_session_reply::Result as Reply,
};
use tokio::sync::{oneshot, watch};

async fn send_boot(boot: NativeAndroidBooted) -> Result<()> {
    transport::write(
        &mut tokio::io::stdout(),
        &NativeAndroidSessionReply {
            result: Some(Reply::Booted(boot)),
        },
    )
    .await?;
    Ok(())
}

async fn serve(
    request: NativeAndroidStart,
    linux_ready: oneshot::Receiver<()>,
    stop: &mut watch::Receiver<bool>,
    published: &mut bool,
) -> Result<()> {
    let started = process::start(Plan::parse(request)?, stop, async |boot, stop| {
        send_boot(boot.clone()).await?;
        super::readiness::cancelled(stop)?;
        tokio::select! { biased;
            _ = stop.changed() => Err(Error::Cancelled),
            result = linux_ready => result.map_err(|_| Error::Cancelled),
        }
    })
    .await?;
    match started {
        Started::Existing(receipt) => {
            send_boot(receipt).await?;
            *published = true;
            Ok(())
        }
        Started::Owned(mut phone) => {
            let result = async {
                let receipt = phone
                    .receipt
                    .as_ref()
                    .ok_or(Error::Invalid("android_not_booted"))?;
                send_boot(receipt.clone()).await?;
                *published = true;
                phone.wait(stop).await
            }
            .await;
            let cleanup = phone.shutdown().await;
            cleanup.and(result)
        }
    }
}
pub async fn run(arguments: &[std::ffi::OsString]) -> Result<()> {
    if !arguments.is_empty() {
        return Err(Error::Invalid("android_session_arguments"));
    }
    let request: NativeAndroidStart =
        transport::read_bounded(&mut tokio::io::stdin(), 16384).await?;
    let vm = request.linux_vm.is_some();
    let (pair, linux_ready) = oneshot::channel();
    let (end, mut stop) = watch::channel(false);
    let control_end = end.clone();
    let control = tokio::spawn(async move {
        let mut input = tokio::io::stdin();
        let mut command = transport::read::<NativeAndroidSessionControl>(&mut input).await;
        if vm
            && command
                .as_ref()
                .is_ok_and(|value| value.linux_ready && !value.cancel_startup)
        {
            let _ = pair.send(());
            command = transport::read(&mut input).await;
        }
        if command.is_ok_and(|value| value.cancel_startup) {
            let _ = control_end.send(true);
        }
    });
    let signal_end = end.clone();
    let signal = tokio::spawn(async move {
        let result = crate::component::interrupt().await;
        if result.is_ok() {
            let _ = signal_end.send(true);
        }
        result
    });
    let mut published = false;
    let result = serve(request, linux_ready, &mut stop, &mut published).await;
    if let Err(error) = &result
        && !published
    {
        if let Error::Io(io) = error {
            eprintln!("android_session_io: {io}");
        }
        let reply = NativeAndroidSessionReply {
            result: Some(Reply::Failure(NativeAndroidSessionFailure {
                code: error.code().into(),
                outcome_unknown: error.code().ends_with("unknown"),
            })),
        };
        let _ = transport::write(&mut tokio::io::stdout(), &reply).await;
    }
    signal.abort();
    let _ = signal.await;
    control.abort();
    let _ = control.await;
    result
}
