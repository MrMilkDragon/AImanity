use crate::{host_desktop, paired_phone};
use std::{error::Error, net::Ipv4Addr, process::Stdio};
use tokio::{
    net::TcpStream,
    process::Command,
    sync::{oneshot, watch},
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub(super) async fn share(
    target: &str,
    code: &str,
    serial: Option<&str>,
    ready: oneshot::Sender<()>,
    mut stop: watch::Receiver<()>,
) -> Result<()> {
    let mut req = "ws://127.0.0.1:17863/v1/peer/share".into_client_request()?;
    let phone = match serial {
        Some(s) => Some(paired_phone::Phone::open(s).await?),
        None => {
            let local = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(1))
                .build()?;
            if let Ok(response) = local
                .head("http://127.0.0.1:17863/v1/peer/share")
                .send()
                .await
                && let Some(origin) = response.headers().get("x-aimanity-workspace")
            {
                req.headers_mut()
                    .insert("x-aimanity-origin", origin.clone());
            }
            host_desktop::permission()?;
            None
        }
    };
    req.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {code}"))?,
    );
    req.headers_mut().insert(
        "x-aimanity-device",
        HeaderValue::from_static(if phone.is_some() { "phone" } else { "desktop" }),
    );
    // A forwarded port is local to this computer. An SSH alias uses its known host key.
    if let Ok(port) = target.parse::<u16>() {
        let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
        socket.set_nodelay(true)?;
        let (ws, _) = tokio::select! {biased; _=stop.changed()=>return Ok(()), ws=tokio::time::timeout(std::time::Duration::from_secs(8),tokio_tungstenite::client_async(req,socket))=>ws??};
        return host_desktop::serve_socket(ws, phone, ready, stop).await;
    }
    if target.is_empty()
        || target.starts_with('-')
        || !target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_@:[]".contains(&b))
    {
        return Err("SSH 主机无效".into());
    }
    let mut child = Command::new("ssh")
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=6",
            "-W",
            "127.0.0.1:17863",
            "--",
            target,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let io = tokio::io::join(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let (ws, _) = tokio::select! {biased; _=stop.changed()=>return Ok(()), ws=tokio::time::timeout(std::time::Duration::from_secs(8),tokio_tungstenite::client_async(req,io))=>ws??};
    host_desktop::serve_socket(ws, phone, ready, stop).await
}

pub(super) fn android_cli(target: &str, serial: &str) -> i32 {
    let run = || -> Result<()> {
        let mut code = String::new();
        std::io::stdin().read_line(&mut code)?;
        tokio::runtime::Runtime::new()?.block_on(async {
            let (_grant, stop) = watch::channel(());
            let (ready, connected) = oneshot::channel();
            tokio::spawn(async move {
                if connected.await.is_ok() {
                    eprintln!("Android sharing connected");
                }
            });
            share(target, code.trim(), Some(serial), ready, stop).await
        })
    };
    match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
