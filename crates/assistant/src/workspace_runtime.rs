//! Workspace execution in the owned Linux guest.
use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

type Result<T> = std::result::Result<T, String>;
async fn drain(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    bytes: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<()> {
    let mut chunk = [0; 8192];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        let keep = n.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk[..keep]);
    }
}
async fn output(child: &mut Child, limit: usize) -> Result<(ExitStatus, String)> {
    let out = child.stdout.take().expect("workspace stdout");
    let err = child.stderr.take().expect("workspace stderr");
    let (mut stdout, mut stderr) = (vec![], vec![]);
    let (_, _, status) = tokio::try_join!(
        drain(out, &mut stdout, limit),
        drain(err, &mut stderr, limit),
        child.wait()
    )
    .map_err(|error| error.to_string())?;
    stdout.extend(stderr);
    stdout.truncate(limit);
    Ok((status, String::from_utf8_lossy(&stdout).into_owned()))
}

fn command(root: &Path, operation: &str) -> Result<Command> {
    let mut command = ai_phone_host_runtime::desktop_bridge::native_command(root, operation)
        .map_err(|error| error.to_string())?;
    command.kill_on_drop(true);
    Ok(command)
}

pub(crate) async fn prepare(root: &Path) -> Result<()> {
    let result = command(root, "stage")?
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned());
    }
    Ok(())
}

pub(crate) async fn execute(
    root: &Path,
    script: &str,
    cancel: &CancellationToken,
) -> Result<(bool, String)> {
    let mut child = command(root, "workspace")?
        .arg(script)
        .arg(std::env::var_os("AI_PHONE_CONTAINER_PROXY").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let lease = child.stdin.take().expect("workspace cancellation pipe");
    let result = output(&mut child, 65536);
    tokio::pin!(result);
    tokio::select! {
        result = &mut result => result.map(|(status, text)| (status.success(), text)),
        _ = tokio::time::sleep(Duration::from_secs(110)) => Err("工作指令超时，请先检查结果，勿直接重放".into()),
        _ = cancel.cancelled() => {
            drop(lease);
            match tokio::time::timeout(Duration::from_secs(5), result).await {
                Ok(Ok((status, _))) if status.code() == Some(130) => Err("canceled".into()),
                Ok(result) => result.map(|(status, text)| (status.success(), text)),
                Err(_) => Err("取消尚未确认；结果可能未知".into()),
            }
        }
    }
}
