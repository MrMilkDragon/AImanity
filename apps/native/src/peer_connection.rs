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
    let guest_audio = phone.is_none() && crate::device_io::has_guest_audio().await;
    if guest_audio {
        req.headers_mut()
            .insert("x-aimanity-audio", HeaderValue::from_static("guest-pcm"));
    }
    let (ws, _tunnel) =
        tokio::select! {biased; _=stop.changed()=>return Ok(()), result=dial(target,req)=>result?};
    let files = match &phone {
        Some(p) => crate::device_files::Endpoint::Android(p.client.clone()),
        None => crate::device_files::Endpoint::Directory(
            std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR).canonicalize()?,
        ),
    };
    let services = crate::device_io::Services {
        target: target.to_owned(),
        files,
        guest_audio,
        pulse_server: std::env::var("PULSE_SERVER").ok(),
    };
    host_desktop::serve_socket(ws, phone, services, ready, stop).await
}
pub(super) trait Transport:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send
{
}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}
pub(super) type Socket = tokio_tungstenite::WebSocketStream<Box<dyn Transport>>;
pub(super) async fn dial(
    target: &str,
    req: tokio_tungstenite::tungstenite::http::Request<()>,
) -> Result<(Socket, Option<tokio::process::Child>)> {
    let (transport, child): (Box<dyn Transport>, _) = if let Ok(port) = target.parse::<u16>() {
        let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
        socket.set_nodelay(true)?;
        (Box::new(socket), None)
    } else {
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
        (Box::new(io), Some(child))
    };
    let (ws, _) = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        tokio_tungstenite::client_async(req, transport),
    )
    .await??;
    Ok((ws, child))
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
