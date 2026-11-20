use ai_phone_api::wire::{
    ControlState, DesktopActionRequest, DesktopObservation, DesktopRequest, DeviceActionRequest,
    DeviceActionResponse, FileListing, FileLocation, FileManageRequest, FileManageResult,
    FileOperation, InputAction, RegistryRequest, RegistryState, TakeControlRequest, TapAction,
    device_action_request::Action,
};
use ai_phone_host_runtime::vm_control;
use prost::Message;
use reqwest::{Client, Response, StatusCode, header::CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Result as IoResult},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    process::{Child, Command},
    time::{Instant, sleep, timeout},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const SECOND: Duration = Duration::from_secs(1);
const UI_DUMP: &str = "/sdcard/dysprosium-install.xml";
const EDIT: &str = "android:id/search_src_text";
const CLEAR: &str = "android:id/search_close_btn";
const PHONE: &str = "http://127.0.0.1:17862";
const ASSISTANT: &str = "http://127.0.0.1:17863";
const MEDIA: &str = "http://127.0.0.1:17865";
const SERIAL: &str = "127.0.0.1:5565";

#[derive(Deserialize)]
struct ReleasePolicy {
    version: String,
}

#[derive(Deserialize)]
struct InstallerReceipt {
    source_commit: String,
    interface: String,
    complete_device_bundle: bool,
    authenticode: Authenticode,
    installer_delivery: Delivery,
    sha256: String,
    payload: Payload,
}

#[derive(Deserialize)]
struct Authenticode {
    cryptographically_verified: bool,
}

#[derive(Deserialize)]
struct Delivery {
    size: u64,
}

#[derive(Deserialize)]
struct Payload {
    files: Vec<PayloadFile>,
}

#[derive(Deserialize)]
struct PayloadFile {
    path: String,
    sha256: String,
}

fn check(condition: bool, error: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(error.into().into())
    }
}

fn valid_digest(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn command(program: impl AsRef<Path>, profile: &Path) -> Command {
    let mut command = Command::new(program.as_ref());
    command.env("APPDATA", profile).env("LOCALAPPDATA", profile);
    command
}

async fn gui(profile: &Path, pid: u32, action: &str) -> Result<()> {
    let status = command("powershell.exe", profile)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "scripts/qualify-windows-window.ps1",
            "-AppId",
            &pid.to_string(),
            "-Action",
            action,
        ])
        .status()
        .await?;
    check(
        status.success(),
        format!("native window {action}: {status}"),
    )
}

struct Android {
    adb: PathBuf,
    profile: PathBuf,
}

impl Android {
    fn open(home: &Path, profile: &Path) -> Result<Self> {
        let component = home
            .join("components/android")
            .read_dir()?
            .next()
            .ok_or("installed Android capsule missing")??;
        Ok(Self {
            adb: component.path().join("sdk/platform-tools/adb.exe"),
            profile: profile.to_path_buf(),
        })
    }

    fn command(&self) -> Command {
        let mut adb = command(&self.adb, &self.profile);
        adb.args(["-P", "15037"]).kill_on_drop(true);
        adb
    }

    async fn shell(&self, script: &str) -> Result<String> {
        let output = self
            .command()
            .args(["-s", SERIAL, "shell", script])
            .output()
            .await?;
        check(
            output.status.success(),
            format!(
                "installed Android ADB {script}: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        Ok(String::from_utf8(output.stdout)?)
    }

    async fn screenshot(&self) -> Result<Vec<u8>> {
        let output = self
            .command()
            .args(["-s", SERIAL, "exec-out", "screencap", "-p"])
            .output()
            .await?;
        check(output.status.success(), "installed Android adb screencap")?;
        Ok(output.stdout)
    }

    async fn focus(&self) -> Result<String> {
        Ok(self
            .shell("dumpsys window displays")
            .await?
            .lines()
            .filter(|line| line.contains("mCurrentFocus") || line.contains("mFocusedApp"))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    async fn editor<T>(&self, read: impl Fn(&str) -> Option<T>) -> Result<T> {
        let deadline = Instant::now() + 15 * SECOND;
        let command = format!(
            "rm -f {UI_DUMP}; timeout 7 uiautomator dump {UI_DUMP} 2>&1; [ ! -s {UI_DUMP} ] || cat {UI_DUMP}"
        );
        loop {
            let xml = self.shell(&command).await?;
            if let Some(result) = read(&xml) {
                return Ok(result);
            }
            if Instant::now() >= deadline {
                let sample: String = xml.chars().take(450).collect();
                return Err(format!(
                    "Android Settings editor not found: {}\n{sample}",
                    self.focus().await?
                )
                .into());
            }
            sleep(Duration::from_millis(250)).await;
        }
    }
}

fn attr<'a>(node: &'a str, name: &str) -> Option<&'a str> {
    Some(node.split_once(name)?.1.split_once('"')?.0)
}

fn node<'a>(xml: &'a str, resource: &str) -> Option<&'a str> {
    xml.split("<node")
        .skip(1)
        .filter_map(|part| part.split_once('>').map(|(node, _)| node))
        .find(|node| attr(node, " resource-id=\"") == Some(resource))
}

fn point(node: &str) -> Option<TapAction> {
    let bounds = attr(node, " bounds=\"")?;
    let mut positions = bounds
        .split(['[', ',', ']'])
        .filter(|part| !part.is_empty())
        .map(str::parse::<u32>);
    let (left, top, right, bottom) = (
        positions.next()?.ok()?,
        positions.next()?.ok()?,
        positions.next()?.ok()?,
        positions.next()?.ok()?,
    );
    Some(TapAction {
        x: (left + right) / 2,
        y: (top + bottom) / 2,
    })
}

async fn post<I: Message, O: Message + Default>(
    http: &Client,
    url: &str,
    token: &str,
    input: I,
    deadline: Duration,
) -> Result<O> {
    let response = http
        .post(url)
        .bearer_auth(token)
        .timeout(deadline)
        .header(CONTENT_TYPE, "application/x-protobuf")
        .body(input.encode_to_vec())
        .send()
        .await?;
    check(
        response.status() == StatusCode::OK,
        format!("{url}: {}", response.status()),
    )?;
    Ok(O::decode(response.bytes().await?)?)
}

async fn media(
    http: &Client,
    token: &str,
    path: &str,
    media_type: &str,
    deadline: Duration,
) -> Result<Response> {
    let response = http
        .get(format!("{MEDIA}{path}"))
        .bearer_auth(token)
        .timeout(deadline)
        .send()
        .await?;
    check(
        response.status() == StatusCode::OK,
        format!("{path}: {}", response.status()),
    )?;
    let mime = response
        .headers()
        .get(CONTENT_TYPE)
        .ok_or("media Content-Type missing")?
        .to_str()?
        .to_ascii_lowercase();
    let matches = if media_type == "multipart/x-mixed-replace" {
        mime.starts_with(media_type)
    } else {
        mime == media_type
    };
    check(matches, format!("{path} media Content-Type: {mime}"))?;
    Ok(response)
}

async fn verify_frame(http: &Client, token: &str, path: &str, device: &str) -> Result<()> {
    let mut stream = media(http, token, path, "multipart/x-mixed-replace", 12 * SECOND).await?;
    let mut frame = Vec::new();
    while frame.len() < 2_000_000 {
        let Some(chunk) = stream.chunk().await? else {
            break;
        };
        frame.extend_from_slice(&chunk);
        if let Some(start) = frame
            .windows(3)
            .position(|bytes| bytes == [0xff, 0xd8, 0xff])
            && frame[start + 3..]
                .windows(2)
                .any(|bytes| bytes == [0xff, 0xd9])
        {
            println!("{device} sent a complete JPEG through the native relay");
            return Ok(());
        }
    }
    Err(format!("{device} did not send a complete JPEG").into())
}

async fn audio_levels(mut pcm: Response) -> Result<(i16, i16, usize)> {
    let mut levels = (0, 0, 0);
    let mut tail = None;
    while let Some(chunk) = pcm.chunk().await? {
        let mut bytes = tail.into_iter().chain(chunk.iter().copied());
        tail = None;
        while let Some(lo) = bytes.next() {
            let Some(hi) = bytes.next() else {
                tail = Some(lo);
                break;
            };
            let sample = i16::from_le_bytes([lo, hi]);
            levels.0 = levels.0.min(sample);
            levels.1 = levels.1.max(sample);
            levels.2 += 1;
        }
        if levels.0 < 0 && levels.1 > 0 {
            return Ok(levels);
        }
    }
    Err("Expected audible 48 kHz stereo PCM".into())
}

fn action_id() -> Result<String> {
    let mut random = [0; 16];
    getrandom::fill(&mut random)?;
    Ok(hex::encode(random))
}

struct Phone<'a> {
    http: &'a Client,
    token: &'a str,
    id: String,
    epoch: u64,
}

impl Phone<'_> {
    async fn action(&self, action: Action) -> Result<DeviceActionResponse> {
        let action_id = action_id()?;
        let result: DeviceActionResponse = post(
            self.http,
            &format!("{PHONE}/v1/action"),
            self.token,
            DeviceActionRequest {
                phone_id: self.id.clone(),
                owner_id: "local-user".into(),
                run_id: "manual".into(),
                action_id: action_id.clone(),
                control_epoch: self.epoch,
                controller: "manual".into(),
                action: Some(action),
                observation_id: String::new(),
            },
            16 * SECOND,
        )
        .await?;
        check(
            result.action_id == action_id,
            "Android action receipt ID differs",
        )?;
        check(
            result.success,
            format!("Android action did not execute: {}", result.output),
        )?;
        Ok(result)
    }
}

async fn verify_android(android: &Android, phone: &Phone<'_>) -> Result<()> {
    check(
        android.shell("id -u").await?.trim() == "0",
        "installed Android root ADB",
    )?;
    let frame = android.screenshot().await?;
    check(
        frame.starts_with(b"\x89PNG\r\n\x1a\n")
            && frame.len() >= 24
            && frame[16..20] == 720_u32.to_be_bytes()
            && frame[20..24] == 1440_u32.to_be_bytes(),
        "Android ADB screenshot is a 720×1440 PNG",
    )?;
    let policy = android.shell("dumpsys window policy").await?;
    let policy = policy
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            ["keyguard", "showing", "interactive"]
                .iter()
                .any(|part| line.contains(part))
        })
        .take(14)
        .collect::<Vec<_>>()
        .join("\n");
    println!(
        "Android pristine window state:\n{}\n{policy}",
        android.focus().await?
    );
    check(
        android
            .shell("locksettings set-disabled --user 0 true")
            .await?
            .contains("Lock screen disabled set to true"),
        "disable Android lock screen",
    )?;
    check(
        android
            .shell("locksettings get-disabled --user 0")
            .await?
            .trim()
            == "true",
        "Android lock screen is disabled",
    )?;
    android.shell("input keyevent KEYCODE_SLEEP").await?;
    sleep(Duration::from_millis(350)).await;
    android.shell("input keyevent KEYCODE_WAKEUP; svc power stayon true; cmd statusbar collapse; wm dismiss-keyguard; input keyevent KEYCODE_HOME").await?;
    check(android.shell("am start -W -a android.settings.APP_SEARCH_SETTINGS -p com.android.settings.intelligence").await?.lines().any(|line| line.trim() == "Status: ok"), "open Android Settings search")?;
    check(
        android
            .shell("settings get secure default_input_method")
            .await?
            .trim()
            == "dev.aiphone.input/.UnicodeIme",
        "installed Unicode IME is active",
    )?;
    let edit = android.editor(|xml| point(node(xml, EDIT)?)).await?;
    phone.action(Action::Tap(edit)).await?;
    android
        .editor(|xml| (attr(node(xml, EDIT)?, " focused=\"") == Some("true")).then_some(()))
        .await?;
    let sample = "Dysprosium 中文";
    let input = phone
        .action(Action::Input(InputAction {
            text: sample.into(),
        }))
        .await?;
    check(
        input
            .output
            .contains(&format!("committed:{}", input.action_id)),
        "installed Unicode IME returned the action receipt",
    )?;
    let clear = android
        .editor(|xml| {
            (attr(node(xml, EDIT)?, " text=\"") == Some(sample)).then_some(())?;
            point(node(xml, CLEAR)?)
        })
        .await?;
    phone.action(Action::Tap(clear)).await?;
    android
        .editor(|xml| {
            (attr(node(xml, EDIT)?, " text=\"") != Some(sample) && node(xml, CLEAR).is_none())
                .then_some(())
        })
        .await?;
    println!(
        "{}",
        json!({"androidRoot": true, "androidFrame": [720, 1440], "androidWorkbenchTouch": true, "androidWorkbenchChinese": true, "linuxJpeg": true})
    );
    verify_audio(android, phone).await
}

async fn verify_audio(android: &Android, phone: &Phone<'_>) -> Result<()> {
    let music = android
        .shell("pm list packages --user 0 com.android.music")
        .await?;
    let install = music.trim() != "package:com.android.music";
    if install {
        check(
            android
                .shell("cmd package install-existing --user 0 com.android.music")
                .await?
                .contains("installed for user: 0"),
            "enable Android system audio preview",
        )?;
    }
    let result = async {
        let pcm = media(phone.http, phone.token, "/audio", "audio/l16;rate=48000;channels=2", 16 * SECOND).await?;
        let playback = android.shell("am start -W -a android.intent.action.VIEW -d file:///product/media/audio/alarms/Neon.ogg -t audio/ogg -n com.android.music/.AudioPreview").await?;
        check(playback.lines().any(|line| line.trim() == "Status: ok"), "play real Android system audio")?;
        let (min, max, samples) = audio_levels(pcm).await?;
        println!("{}", json!({"androidAudio": true, "rate": 48000, "channels": 2, "samples": samples, "min": min, "max": max}));
        Ok(())
    }.await;
    let stopped = android.shell("am force-stop com.android.music").await;
    let uninstalled = if install {
        android
            .shell("pm uninstall -k --user 0 com.android.music")
            .await
            .map(drop)
    } else {
        Ok(())
    };
    result.and(stopped.map(drop)).and(uninstalled)
}

async fn verify_desktop(http: &Client, assistant: &str, media_token: &str, id: &str) -> Result<()> {
    let request = DesktopRequest {
        device_id: id.into(),
    };
    let initial: DesktopObservation = post(
        http,
        &format!("{ASSISTANT}/v1/desktop/control"),
        assistant,
        request.clone(),
        10 * SECOND,
    )
    .await?;
    let observe = async || {
        post::<_, DesktopObservation>(
            http,
            &format!("{ASSISTANT}/v1/desktop/observe"),
            assistant,
            request.clone(),
            10 * SECOND,
        )
        .await
    };
    let act = async |mut action: DesktopActionRequest| {
        let view = observe().await?;
        check(view.epoch == initial.epoch, "Linux desktop control changed")?;
        action.device_id = id.into();
        action.action_id = action_id()?;
        action.observation_id = view.observation_id;
        action.epoch = view.epoch;
        let receipt: DesktopObservation = post(
            http,
            &format!("{ASSISTANT}/v1/desktop/action"),
            assistant,
            action,
            12 * SECOND,
        )
        .await?;
        check(
            receipt.device_id == id && receipt.epoch == initial.epoch,
            "Linux desktop action receipt",
        )
    };
    let key = async |key: &str| {
        act(DesktopActionRequest {
            kind: "key".into(),
            key: key.into(),
            ..Default::default()
        })
        .await
    };
    let text = async |text: String| {
        act(DesktopActionRequest {
            kind: "text".into(),
            text,
            ..Default::default()
        })
        .await
    };
    // The packaged desktop menu begins Browser, Files, Terminal.
    act(DesktopActionRequest {
        kind: "click".into(),
        x: (initial.width / 2).try_into()?,
        y: (initial.height / 2).try_into()?,
        button: 3,
        ..Default::default()
    })
    .await?;
    for button in ["Home", "Down", "Down", "Return"] {
        key(button).await?;
    }
    timeout(6 * SECOND, async {
        loop {
            if observe()
                .await?
                .application_id
                .eq_ignore_ascii_case("linux:xterm")
            {
                return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .map_err(|_| "Linux desktop terminal did not open")??;

    let sample = format!("原生中文回执-{}", &action_id()?[..8]);
    let name = format!("{sample}.txt");
    let started = Instant::now();
    text(format!("printf %s '{sample}' > \"$HOME/Files/{name}\"")).await?;
    let paste_ms = started.elapsed().as_millis();
    key("Return").await?;
    let saved = timeout(6 * SECOND, async {
        loop {
            let list: FileListing = post(
                http,
                &format!("{ASSISTANT}/v1/files/list"),
                assistant,
                FileLocation {
                    device_id: id.into(),
                    path: "/home/ai/Files".into(),
                },
                8 * SECOND,
            )
            .await?;
            if let Some(saved) = list.entries.into_iter().find(|row| row.name == name) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(saved);
            }
            sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .map_err(|_| "Linux desktop input file was not written")??;
    let copied: FileManageResult = post(
        http,
        &format!("{ASSISTANT}/v1/files/manage"),
        assistant,
        FileManageRequest {
            operation: FileOperation::Copy.into(),
            sources: vec![FileLocation {
                device_id: id.into(),
                path: saved.path,
            }],
            target: Some(FileLocation {
                path: "/".into(),
                ..Default::default()
            }),
            name: String::new(),
        },
        8 * SECOND,
    )
    .await?;
    let path = copied
        .results
        .into_iter()
        .next()
        .filter(|result| result.error.is_empty())
        .and_then(|result| result.target)
        .ok_or("Linux desktop file copy failed")?
        .path;
    let bytes = http
        .get(format!("{ASSISTANT}/v1/workbench/file"))
        .query(&[("path", path.trim_start_matches('/'))])
        .bearer_auth(assistant)
        .timeout(8 * SECOND)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    check(
        bytes.as_ref() == sample.as_bytes(),
        "Linux desktop input differs from the saved UTF-8 file",
    )?;
    println!(
        "{}",
        json!({"linuxChinese": true, "pasteMs": paste_ms, "bytes": bytes.len()})
    );

    let pcm = media(
        http,
        media_token,
        "/desktop/audio",
        "audio/l16;rate=48000;channels=2",
        16 * SECOND,
    )
    .await?;
    let started = Instant::now();
    text("tone=$(mktemp --suffix=.wav); ffmpeg -loglevel error -y -f lavfi -i sine=frequency=660:duration=1.5 \"$tone\" && paplay --device=aiphone \"$tone\"; rm -f \"$tone\"".into()).await?;
    key("Return").await?;
    let (min, max, samples) = audio_levels(pcm).await?;
    println!(
        "{}",
        json!({"linuxAudio": true, "firstSignalMs": started.elapsed().as_millis(), "rate": 48000, "channels": 2, "samples": samples, "min": min, "max": max})
    );
    Ok(())
}

async fn watch_android(home: PathBuf, profile: PathBuf) -> Result<()> {
    let mut tick = tokio::time::interval(10 * SECOND);
    tick.tick().await;
    while !home.join("user/data/android/native/sdk.log").is_file() {
        tick.tick().await;
    }
    let started = Instant::now();
    while started.elapsed() < 85 * SECOND || !home.join("components/android").is_dir() {
        tick.tick().await;
    }
    let android = Android::open(&home, &profile)?;
    match timeout(
        4 * SECOND,
        android.command().args(["devices", "-l"]).output(),
    )
    .await
    {
        Ok(Ok(output)) => println!(
            "Android boot transport: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        ),
        other => eprintln!("Android boot transport unavailable: {other:?}"),
    }
    println!(
        "{:?}",
        timeout(
            4 * SECOND,
            android.shell("getprop sys.boot_completed; id -u")
        )
        .await
    );
    while started.elapsed() < 114 * SECOND {
        tick.tick().await;
    }
    let state = timeout(
        4 * SECOND,
        android.shell("am get-started-user-state 0; date +%s"),
    )
    .await??;
    println!(
        "Android first-run user state: {}; host clock {}",
        state.trim(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
    );
    let start = Instant::now();
    match timeout(15 * SECOND, android.screenshot()).await {
        Ok(Ok(png)) => println!(
            "Android cold screenshot: {} bytes in {} ms",
            png.len(),
            start.elapsed().as_millis()
        ),
        other => eprintln!("Android cold screenshot: {other:?}"),
    }
    for name in ["notifications", "attention"] {
        let script =
            format!("cat /data/user/0/dev.aiphone.input/files/{name}.json 2>/dev/null || true");
        let raw = timeout(4 * SECOND, android.shell(&script)).await??;
        if raw.is_empty() {
            println!("Android {name} listener has not written a receipt");
            continue;
        }
        let receipt: Value = serde_json::from_str(&raw)?;
        println!(
            "Android {name} listener: connected={}, checked_at_ms={}",
            receipt["connected"], receipt["checked_at_ms"]
        );
    }
    Ok(())
}

fn diagnostic_logs(home: &Path) {
    for path in [
        "user/qemu/owner.log",
        "user/qemu/boot.log",
        "user/qemu/qemu.log",
        "user/data/android/native/sdk.log",
        "user/data/android/whpx-avd/cuttlefish/kernel.log",
        "deployment.json",
    ] {
        let Ok(output) = fs::read(home.join(path)) else {
            continue;
        };
        let output = String::from_utf8_lossy(&output);
        let milestones = output
            .lines()
            .rev()
            .filter(|line| {
                let line = line.to_ascii_lowercase();
                [
                    "adbd",
                    "boot_completed",
                    "keyguard",
                    "surfaceflinger",
                    "panic",
                    "whpx",
                    "systemserver",
                    "fatal exception",
                ]
                .iter()
                .any(|part| line.contains(part))
            })
            .take(32)
            .collect::<Vec<_>>();
        let tail = output.floor_char_boundary(output.len().saturating_sub(3600));
        eprintln!(
            "{path}\n{}\n{}",
            milestones.into_iter().rev().collect::<Vec<_>>().join("\n"),
            &output[tail..]
        );
    }
}

fn first_frame(log: &Path) -> IoResult<Option<(f64, f64)>> {
    let mut frames = [None, None];
    for line in BufReader::new(File::open(log)?).lines() {
        let line = line?;
        if let Some((_, metric)) = line.split_once("native_metric first_frame device=")
            && let Some((device, ms)) = metric.split_once(" ms=")
            && let Ok(device @ (0 | 1)) = device.parse::<usize>()
        {
            frames[device] = ms.split_whitespace().next().and_then(|ms| ms.parse().ok());
            if let [Some(android), Some(desktop)] = frames {
                return Ok(Some((android, desktop)));
            }
        }
    }
    Ok(None)
}

async fn verify_installed(
    app: &mut Child,
    profile: &Path,
    home: &Path,
    log: &Path,
    source: &str,
    payload: &str,
) -> Result<()> {
    let pid = app
        .id()
        .ok_or("installed native client must remain running")?;
    gui(profile, pid, "fresh").await?;
    timeout(600 * SECOND, async {
        loop {
            check(
                app.try_wait()?.is_none(),
                "installed native client must remain running",
            )?;
            let checkpoint = home.join("deployment.json");
            if checkpoint.is_file() {
                let status: Value = serde_json::from_reader(File::open(checkpoint)?)?;
                let state = &status["progress"]["state"];
                if state == "ready" && status["installed_payload"] == payload {
                    return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
                }
                check(
                    state != "failed" && state != "blocked",
                    format!("native deployment: {state}"),
                )?;
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .map_err(|_| "native deployment did not complete using the reviewed payload")??;
    let (android_frame, desktop_frame) = timeout(30 * SECOND, async {
        loop {
            if let Some(frames) = first_frame(log)? {
                return Ok::<_, std::io::Error>(frames);
            }
            sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .map_err(|_| "the owned native window must receive both device frames")??;
    gui(profile, pid, "capture").await?;
    println!(
        "{}",
        json!({"androidFirstFrameMs": android_frame, "desktopFirstFrameMs": desktop_frame, "source": source})
    );

    let credentials = vm_control::native_credentials(&home.join("user/qemu/control.key")).await?;
    let http = Client::builder().no_proxy().build()?;
    let registry: RegistryState = post(
        &http,
        &format!("{ASSISTANT}/v1/registry/state"),
        credentials.assistant_token.trim(),
        RegistryRequest {},
        10 * SECOND,
    )
    .await?;
    let desktop = registry
        .devices
        .iter()
        .find(|device| device.kind == "desktop" && device.runtime_state == "managed")
        .ok_or("installed desktop is registered")?
        .id
        .clone();
    let phone = registry
        .devices
        .into_iter()
        .find(|device| device.kind == "phone" && device.runtime_state == "managed")
        .ok_or("installed Android is registered")?;
    let token = credentials.phone_token.trim();
    let (desktop_frame, phone_frame) = tokio::join!(
        verify_frame(&http, token, "/desktop/screen", "Linux"),
        verify_frame(&http, token, "/screen", "Android"),
    );
    let control: ControlState = post(
        &http,
        &format!("{PHONE}/v1/control"),
        token,
        TakeControlRequest {
            phone_id: phone.id.clone(),
            owner_id: "local-user".into(),
            controller: "manual".into(),
        },
        16 * SECOND,
    )
    .await?;
    let phone = Phone {
        http: &http,
        token,
        id: phone.id,
        epoch: control.control_epoch,
    };
    let android = Android::open(home, profile)?;
    let (phone_actions, desktop_actions) = tokio::join!(
        verify_android(&android, &phone),
        verify_desktop(&http, credentials.assistant_token.trim(), token, &desktop),
    );
    let mut failures = Vec::new();
    for (step, result) in [
        ("Android video", phone_frame),
        ("Linux video", desktop_frame),
        ("Android controls and audio", phone_actions),
        ("Linux controls and audio", desktop_actions),
    ] {
        if let Err(error) = result {
            failures.push(format!("{step}: {error}"));
        }
    }
    check(failures.is_empty(), failures.join("\n"))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let root = env::var("AIMANITY_QA_ROOT")?;
    check(
        cfg!(windows)
            && env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
            && root.starts_with(|c: char| c.is_ascii_uppercase())
            && root
                .get(1..)
                .is_some_and(|path| path.starts_with(":\\Dysprosium-QA-")),
        "Run on the disposable Windows acceptance host",
    )?;
    let source = env::var("AIMANITY_QA_SOURCE")?;
    check(
        valid_digest(&source, 40),
        "Supply the reviewed source commit",
    )?;
    let started = Instant::now();
    let root = PathBuf::from(root);
    let install = root.join("installed");
    let profile = root.join("profile");
    let home = profile.join("dev.aimanity.native/host");
    check(
        !home.exists() && !install.exists(),
        "fresh isolated Windows runtime",
    )?;
    let ReleasePolicy { version } =
        serde_json::from_reader(File::open("config/release-policy.json")?)?;
    let candidate = root.join("candidate");
    let executable = format!("AImanity-{version}-windows-x64-setup.exe");
    let installer = candidate.join(&executable);
    let receipt: InstallerReceipt = serde_json::from_reader(File::open(
        candidate.join(format!("{executable}.manifest.json")),
    )?)?;
    check(
        receipt.source_commit == source
            && receipt.interface == "iced-wgpu"
            && receipt.complete_device_bundle
            && receipt.authenticode.cryptographically_verified,
        "complete signed native installer for the reviewed source",
    )?;
    let mut file = File::open(&installer)?;
    check(
        file.metadata()?.len() == receipt.installer_delivery.size,
        "candidate size matches its installer receipt",
    )?;
    let mut hash = Sha256::new();
    let mut block = [0; 64 * 1024];
    loop {
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        hash.update(&block[..count]);
    }
    check(
        hex::encode(hash.finalize()) == receipt.sha256,
        "candidate matches its signed installer receipt",
    )?;
    drop(file);
    let payload = receipt
        .payload
        .files
        .into_iter()
        .find(|file| file.path.replace('\\', "/") == "host/source.payload")
        .filter(|file| valid_digest(&file.sha256, 64))
        .ok_or("installer includes the identified source payload")?;
    let status = command(&installer, &profile)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/SP-",
            "/NORESTART",
            "/RESTARTEXITCODE=3010",
            "/NORESTARTAPPLICATIONS",
        ])
        .arg(format!("/DIR={}", install.display()))
        .stdout(Stdio::null())
        .status()
        .await?;
    check(
        status.success(),
        format!("signed Inno installation must succeed without pending reboot: {status}"),
    )?;
    println!(
        "{}",
        json!({"installerExit": 0, "ms": started.elapsed().as_millis()})
    );

    let log = root.join("native.log");
    let mut app = command(install.join("ai-phone-client.exe"), &profile)
        .current_dir(&install)
        .stdout(Stdio::null())
        .stderr(File::create(&log)?)
        .spawn()?;
    let watcher = tokio::spawn(watch_android(home.clone(), profile.clone()));
    let result = verify_installed(&mut app, &profile, &home, &log, &source, &payload.sha256).await;
    if result.is_err() {
        diagnostic_logs(&home);
    }
    if watcher.is_finished() {
        if let Ok(Err(error)) = watcher.await {
            eprintln!("Android boot telemetry: {error}");
        }
    } else {
        watcher.abort();
    }
    let shutdown = async {
        if app.try_wait()?.is_none() {
            gui(
                &profile,
                app.id().ok_or("installed native client PID unavailable")?,
                "close",
            )
            .await?;
            let status = timeout(120 * SECOND, app.wait())
                .await
                .map_err(|_| "native client shutdown timed out")??;
            check(
                status.success(),
                format!("native client normal shutdown: {status}"),
            )?;
        }
        Ok(())
    }
    .await;
    result.and(shutdown)
}
