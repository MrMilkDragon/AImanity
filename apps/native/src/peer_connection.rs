#[cfg(not(target_os = "android"))]
use crate::host_desktop;
use crate::paired_phone;
use std::{error::Error, net::Ipv4Addr, process::Stdio};
use tokio::{
    net::TcpStream,
    process::Command,
    sync::{oneshot, watch},
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[cfg(not(target_os = "android"))]
pub(super) async fn share(
    target: &str,
    code: &str,
    serial: Option<&str>,
    notices: bool,
    ready: oneshot::Sender<()>,
    stop: watch::Receiver<()>,
) -> Result<()> {
    let phone = match serial {
        Some(s) => Some(paired_phone::Phone::open(s).await?),
        None => {
            host_desktop::permission()?;
            None
        }
    };
    share_device(target, code, phone, notices, ready, stop).await
}
pub(super) fn registration(
    code: &str,
    system: &'static str,
    guest_audio: bool,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>> {
    let mut req = "ws://127.0.0.1:17863/v1/peer/share".into_client_request()?;
    req.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {code}"))?,
    );
    req.headers_mut().insert(
        "x-aimanity-device",
        HeaderValue::from_static(if system == "android" {
            "phone"
        } else {
            "desktop"
        }),
    );
    req.headers_mut()
        .insert("x-aimanity-system", HeaderValue::from_static(system));
    if guest_audio {
        req.headers_mut()
            .insert("x-aimanity-audio", HeaderValue::from_static("guest-pcm"));
    }
    Ok(req)
}
pub(super) async fn share_device(
    target: &str,
    code: &str,
    phone: Option<paired_phone::Phone>,
    notices: bool,
    ready: oneshot::Sender<()>,
    mut stop: watch::Receiver<()>,
) -> Result<()> {
    let guest_audio = match &phone {
        Some(phone) => phone.guest_audio,
        None => crate::device_io::has_guest_audio().await,
    };
    let mut req = registration(
        code,
        if phone.is_some() {
            "android"
        } else {
            std::env::consts::OS
        },
        guest_audio,
    )?;
    if phone.is_none() {
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
    }
    let (ws, _tunnel) =
        tokio::select! {biased; _=stop.changed()=>return Ok(()), result=dial(target,req)=>result?};
    let files = phone
        .as_ref()
        .map_or(crate::device_files::Endpoint::Desktop, |p| {
            crate::device_files::Endpoint::Android(p.client.clone())
        });
    let services = crate::device_io::Services {
        target: target.into(),
        files,
        guest_audio,
        notices,
        pulse_server: std::env::var("PULSE_SERVER").ok(),
    };
    crate::peer_session::serve_socket(ws, phone, services, ready, stop).await
}
pub(super) trait Transport:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send
{
}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}
pub(super) type Socket = tokio_tungstenite::WebSocketStream<Box<dyn Transport>>;
pub(super) async fn io_socket(
    target: &str,
    key: &str,
) -> Result<(Socket, Option<tokio::process::Child>)> {
    let mut req = "ws://127.0.0.1:17863/v1/peer/io".into_client_request()?;
    req.headers_mut()
        .insert("authorization", format!("Bearer {key}").parse()?);
    dial(target, req).await
}
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
        let mut command = Command::new("ssh");
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let mut child = command
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

#[cfg(not(target_os = "android"))]
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
            share(target, code.trim(), Some(serial), true, ready, stop).await
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
