use super::Result;
use crate::paired_phone::Phone;
use ai_phone_adb::{Client, Limits};
use ai_phone_api::wire::{
    DesktopActionRequest as Action, DeviceIoOperation as Op, DeviceIoRequest,
    HostDesktopReply as Reply,
};
use ai_phone_host_runtime::guest_channel::{self, Service};
use aimanity_device_io::{StorageMedia, files::Endpoint, unpack};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::{path::Path, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message as Frame, protocol::Role},
};

type Guest = WebSocketStream<Box<dyn guest_channel::Stream>>;
const STEP: Duration = Duration::from_secs(15);

async fn channel(home: &Path, service: Service) -> Result<Guest> {
    let (stream, _) = guest_channel::open(home, service)
        .await?
        .ok_or("Linux guest stopped")?;
    Ok(WebSocketStream::from_raw_socket(stream, Role::Client, None).await)
}
async fn rpc(socket: &mut Guest, mut action: Action) -> Result<Reply> {
    action.action_id = uuid::Uuid::new_v4().to_string();
    socket
        .send(Frame::Binary(action.encode_to_vec().into()))
        .await?;
    timeout(STEP, async {
        while let Some(frame) = socket.next().await {
            if let Frame::Binary(bytes) = frame? {
                let reply = Reply::decode(bytes)?;
                if reply.notification.is_some() {
                    continue;
                }
                if reply.action_id != action.action_id {
                    return Err("Wrong action receipt".into());
                }
                if !reply.error.is_empty() {
                    return Err(reply.error.into());
                }
                return Ok(reply);
            }
        }
        Err("Linux control channel closed".into())
    })
    .await?
}
async fn act(socket: &mut Guest, kind: &str, key: &str, text: &str) -> Result<()> {
    let view = rpc(
        socket,
        Action {
            kind: "observe".into(),
            ..Default::default()
        },
    )
    .await?;
    if view.observation_id.is_empty() || !view.jpeg.starts_with(&[255, 216, 255]) {
        return Err("Linux did not return a frame".into());
    }
    rpc(
        socket,
        Action {
            kind: kind.into(),
            key: key.into(),
            text: text.into(),
            x: (view.width / 2).try_into()?,
            y: (view.height / 2).try_into()?,
            observation_id: view.observation_id,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}
async fn io(home: &Path, op: Op, path: &str) -> Result<Guest> {
    let mut ws = channel(home, Service::FilesAndAudio).await?;
    let req = DeviceIoRequest {
        operation: op.into(),
        path: path.into(),
        ..Default::default()
    };
    ws.send(Frame::Binary(req.encode_to_vec().into())).await?;
    Ok(ws)
}
async fn signal<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>) -> Result<()> {
    let mut negative = false;
    let mut positive = false;
    timeout(Duration::from_secs(20), async {
        while let Some(frame) = ws.next().await {
            if let Frame::Binary(bytes) = frame? {
                let Some(pcm) = unpack(bytes)? else { break };
                for sample in pcm.chunks_exact(2) {
                    let v = i16::from_le_bytes([sample[0], sample[1]]);
                    negative |= v < -32;
                    positive |= v > 32;
                }
                if positive && negative {
                    return Ok(());
                }
            }
        }
        Err("No audible samples on the selected device".into())
    })
    .await?
}
pub async fn desktop(home: &Path) -> Result<()> {
    let mut ctrl = channel(home, Service::Control).await?;
    let Some(Ok(Frame::Binary(hello))) = timeout(STEP, ctrl.next()).await? else {
        return Err("Missing Linux greeting".into());
    };
    Reply::decode(hello)?;
    let entry = rpc(
        &mut ctrl,
        Action {
            kind: "foreground".into(),
            ..Default::default()
        },
    )
    .await?
    .foreground;
    act(&mut ctrl, "right_click", "", "").await?;
    for key in ["home", "down", "down", "enter"] {
        act(&mut ctrl, "key", key, "").await?;
    }
    timeout(STEP, async {
        while rpc(
            &mut ctrl,
            Action {
                kind: "foreground".into(),
                ..Default::default()
            },
        )
        .await?
        .foreground
            == entry
        {
            sleep(Duration::from_millis(150)).await;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    let sample = "工作区中文回执";
    let file = "/tmp/aimanity-acceptance.txt";
    act(
        &mut ctrl,
        "type",
        "",
        &format!("printf %s '{sample}' > '{file}'"),
    )
    .await?;
    act(&mut ctrl, "key", "enter", "").await?;
    let mut received = Vec::new();
    let mut stream = io(home, Op::Read, file).await?;
    while let Some(frame) = timeout(STEP, stream.next()).await? {
        if let Frame::Binary(bytes) = frame? {
            match unpack(bytes)? {
                Some(data) => received.extend_from_slice(&data),
                None => break,
            }
        }
    }
    assert_eq!(received, sample.as_bytes());
    let mut speaker = io(home, Op::AudioOutput, "").await?;
    act(&mut ctrl,"type","","ffmpeg -loglevel error -y -f lavfi -i sine=frequency=660:duration=2 /tmp/qa-tone.wav && paplay --device=aiphone /tmp/qa-tone.wav").await?;
    act(&mut ctrl, "key", "enter", "").await?;
    signal(&mut speaker).await?;
    println!(
        "Linux: observed desktop, typed Chinese, read the saved bytes and captured speaker PCM."
    );
    Ok(())
}

async fn shell(client: &Client, command: &str) -> Result<String> {
    let out = client
        .shell(
            command,
            Limits {
                stdout: 262144,
                stderr: 2048,
            },
            STEP,
        )
        .await?;
    if out.code != 0 {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(String::from_utf8(out.stdout)?)
}
async fn search_box(adb: &Client, sample: Option<&str>) -> Result<(i32, i32)> {
    timeout(Duration::from_secs(30),async {
        loop {
            let xml=shell(adb,"uiautomator dump /sdcard/aimanity-qa.xml >/dev/null && cat /sdcard/aimanity-qa.xml").await?;
            if let Some(node)=xml.split("<node ").filter_map(|n|n.split_once('>').map(|(n,_)|n))
                .find(|n|n.contains("resource-id=\"android:id/search_src_text\"") && sample.is_none_or(|s|n.contains(&format!("text=\"{s}\"")))) {
                let bounds=node.split_once("bounds=\"").and_then(|(_,s)|s.split_once('"')).ok_or("Android field bounds missing")?.0;
                let points:Vec<i32>=bounds.split(['[',',',']']).filter(|s|!s.is_empty()).map(str::parse).collect::<std::result::Result<_,_>>()?;
                if let [x1,y1,x2,y2]=points.as_slice() {return Result::Ok(((x1+x2)/2,(y1+y2)/2));}
            }
            sleep(Duration::from_millis(150)).await;
        }
    }).await?
}
pub async fn android(adb: Client) -> Result<()> {
    assert_eq!(shell(&adb, "id -u").await?.trim(), "0");
    let mut phone = Phone::owned(adb.clone());
    let started=shell(&adb,"wm dismiss-keyguard; am start -W -a android.settings.APP_SEARCH_SETTINGS -p com.android.settings.intelligence").await?;
    assert!(started.contains("Status: ok"));
    let (x, y) = search_box(&adb, None).await?;
    let sample = "Fluorine 中文";
    for (kind, value) in [("click", ""), ("type", sample)] {
        let view = phone
            .perform(&Action {
                kind: "observe".into(),
                ..Default::default()
            })
            .await?;
        assert!(view.jpeg.starts_with(&[255, 216, 255]));
        phone
            .perform(&Action {
                kind: kind.into(),
                text: value.into(),
                x,
                y,
                action_id: uuid::Uuid::new_v4().to_string(),
                observation_id: view.observation_id,
                ..Default::default()
            })
            .await?;
    }
    search_box(&adb, Some(sample)).await?;
    let (server, client) = tokio::io::duplex(65536);
    let storage = StorageMedia {
        files: Endpoint::Android(adb.clone()),
        guest_audio: true,
        pulse_server: None,
    };
    let mut output = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
    let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
    let record = storage.serve(
        &mut server,
        DeviceIoRequest {
            operation: Op::AudioOutput.into(),
            ..Default::default()
        },
    );
    let music_present = shell(&adb, "pm list packages --user 0 com.android.music")
        .await?
        .trim()
        == "package:com.android.music";
    let listen = async {
        shell(&adb,"cmd package install-existing --user 0 com.android.music >/dev/null && am start -W -a android.intent.action.VIEW -d file:///product/media/audio/alarms/Neon.ogg -t audio/ogg -n com.android.music/.AudioPreview").await?;
        signal(&mut output).await
    };
    tokio::select! {res=listen=>res?,res=record=>{res?;return Err("Android speaker closed before playback".into());}}
    shell(&adb, "am force-stop com.android.music").await?;
    if !music_present {
        shell(&adb, "pm uninstall -k --user 0 com.android.music").await?;
    }
    println!(
        "Android: captured frame over root ADB, verified touch and Chinese text in the UI tree, and captured speaker PCM."
    );
    Ok(())
}
