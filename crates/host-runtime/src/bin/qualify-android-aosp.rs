use ai_phone_api::wire::{
    NativeAndroidBooted, NativeAndroidSessionControl, NativeAndroidSessionReply,
    NativeAndroidStart, native_android_session_reply::Result as Reply,
};
use ai_phone_host_runtime::{Installation, android::Resources, native_ipc};
use serde::Deserialize;
use serde_json::json;
use std::{env, fs, path::Path, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    net::TcpStream,
    process::Command,
    time::{Instant, sleep, timeout},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SERIAL: &str = "127.0.0.1:5565";
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const DUMP: &str = "/sdcard/aimanity-whpx.xml";

#[derive(Deserialize)]
struct CapsuleReceipt {
    archive_sha256: String,
}

struct Adb(PathBuf);
impl Adb {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.0);
        command
            .args(["-P", "15037", "-s", SERIAL])
            .kill_on_drop(true);
        command
    }
    async fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = timeout(Duration::from_secs(20), self.command().args(args).output()).await??;
        if !output.status.success() {
            return Err(format!(
                "adb {args:?}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(output.stdout)
    }
    async fn shell(&self, words: &[&str]) -> Result<String> {
        let mut args = vec!["shell"];
        args.extend_from_slice(words);
        Ok(String::from_utf8(self.run(&args).await?)?)
    }
    async fn snapshot(&self) -> Result<Vec<u8>> {
        let png = self.run(&["exec-out", "screencap", "-p"]).await?;
        if !png.starts_with(&PNG) {
            return Err("Android returned an invalid PNG".into());
        }
        Ok(png)
    }
    async fn ui<T>(&self, read: impl Fn(&str) -> Option<T>) -> Result<T> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let command = format!(
            "rm -f {DUMP}; timeout 7 uiautomator dump {DUMP} 2>&1; [ ! -s {DUMP} ] || cat {DUMP}"
        );
        loop {
            let xml = self.shell(&[&command]).await?;
            if let Some(result) = read(&xml) {
                return Ok(result);
            }
            if Instant::now() >= deadline {
                let displays = self.shell(&["dumpsys", "window", "displays"]).await?;
                let focus = displays
                    .lines()
                    .filter(|line| line.contains("mCurrentFocus") || line.contains("mFocusedApp"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let sample: String = xml.trim().chars().take(500).collect();
                return Err(
                    format!("Android Dialer input did not appear: {focus}\n{sample}").into(),
                );
            }
            sleep(Duration::from_millis(150)).await;
        }
    }

    async fn audio(&self) -> Result<()> {
        let started = Instant::now();
        let port: u16 = std::str::from_utf8(
            &self
                .run(&["forward", "tcp:0", "localabstract:scrcpy_00000017"])
                .await?,
        )?
        .trim()
        .parse()?;
        let result: Result<()> = async {
            let mut recorder = self.command().args(["shell", "-T", "exec su 2000 env CLASSPATH=/data/local/tmp/aimanity-audio.jar app_process / com.genymobile.scrcpy.Server 3.3.4 scid=00000017 audio=true audio_source=output audio_codec=raw video=false control=false tunnel_forward=true send_dummy_byte=true send_device_meta=false send_frame_meta=false cleanup=false"])
                .stdin(Stdio::null()).spawn()?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let idle = Duration::from_secs(12);
            let mut stream = loop {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
                let mut prefix = [0; 5];
                let count = timeout(idle, stream.read(&mut prefix)).await??;
                if count != 0 {
                    timeout(idle, stream.read_exact(&mut prefix[count..])).await??;
                    if prefix != [0, 0, b'r', b'a', b'w'] { return Err("Android rejected raw audio capture".into()); }
                    break stream;
                }
                if recorder.try_wait()?.is_some() || Instant::now() >= deadline {
                    return Err("Android audio capture ended before returning PCM".into());
                }
                sleep(Duration::from_millis(100)).await;
            };
            let playback = self.shell(&["am", "start", "-W", "-a", "android.intent.action.VIEW", "-d", "file:///product/media/audio/alarms/Neon.ogg", "-t", "audio/ogg", "-n", "com.android.music/.AudioPreview"]).await?;
            if !playback.lines().any(|line| line.trim() == "Status: ok") {
                return Err(format!("Android alarm did not play: {}", playback.trim()).into());
            }
            let (mut min, mut max, mut samples, mut carry) = (0i16, 0i16, 0, 0);
            let mut buffer = [0; 8193];
            loop {
                let count = timeout(idle, stream.read(&mut buffer[carry..])).await??;
                if count == 0 { return Err("Android audio capture ended before returning nonzero PCM".into()); }
                let mut pcm = buffer[..count + carry].chunks_exact(2);
                for bytes in &mut pcm {
                    let value = i16::from_le_bytes([bytes[0], bytes[1]]);
                    min = min.min(value);
                    max = max.max(value);
                    samples += 1;
                }
                if min < 0 && max > 0 {
                    println!("{}", json!({"androidAudio":"scrcpy-adb", "rate":48000, "channels":2, "samples":samples, "min":min, "max":max, "elapsed_ms":started.elapsed().as_millis()}));
                    return Ok(());
                }
                if samples >= 10 * 48000 * 2 { return Err("Android alarm produced silent PCM".into()); }
                carry = pcm.remainder().len();
                if let [byte] = pcm.remainder() { buffer[0] = *byte; }
            }
        }.await;
        let unforward = self
            .run(&["forward", "--remove", &format!("tcp:{port}")])
            .await;
        let stop = self.shell(&["am", "force-stop", "com.android.music"]).await;
        result?;
        unforward?;
        stop?;
        Ok(())
    }
}

fn attribute<'a>(node: &'a str, name: &str) -> Option<&'a str> {
    node.split_once(&format!(" {name}=\""))?
        .1
        .split_once('"')
        .map(|part| part.0)
}
fn dialer_node<'a>(xml: &'a str, id: &str) -> Option<&'a str> {
    xml.split("<node")
        .skip(1)
        .filter_map(|node| node.split_once('>').map(|part| part.0))
        .find(|node| attribute(node, "resource-id") == Some(id))
}
fn dialpad_point(xml: &str) -> Option<(u32, u32)> {
    let node = dialer_node(xml, "com.android.dialer:id/one")?;
    let bounds = attribute(node, "bounds")?;
    let parts = bounds
        .split(['[', ']', ','])
        .filter(|part| !part.is_empty())
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    let [left, top, right, bottom] = parts.as_slice() else {
        return None;
    };
    Some((left.midpoint(*right), top.midpoint(*bottom)))
}

async fn check(adb: &Adb, boot: NativeAndroidBooted, started: Instant) -> Result<()> {
    if !boot.root_verified || boot.serial != SERIAL || boot.width == 0 || boot.height == 0 {
        return Err("Native boot receipt did not prove the expected root device".into());
    }
    if adb.shell(&["id", "-u"]).await?.trim() != "0" {
        return Err("Independent ADB root readback failed".into());
    }
    let before = adb.snapshot().await?;
    println!(
        "{}",
        json!({"acceleration":"whpx", "rootVerified":true, "serial":boot.serial, "width":boot.width, "height":boot.height, "before":before.len(), "boot_ms":started.elapsed().as_millis()})
    );
    adb.shell(&["locksettings", "set-disabled", "--user", "0", "true"])
        .await?;
    if adb
        .shell(&["locksettings", "get-disabled", "--user", "0"])
        .await?
        .trim()
        != "true"
    {
        return Err("Android keyguard is still enabled on the qualification guest".into());
    }
    adb.shell(&["input", "keyevent", "KEYCODE_SLEEP"]).await?;
    sleep(Duration::from_millis(350)).await;
    adb.shell(&["input keyevent KEYCODE_WAKEUP; svc power stayon true; cmd statusbar collapse; wm dismiss-keyguard; input keyevent KEYCODE_HOME"])
        .await?;
    adb.audio().await?;
    let launch = adb
        .shell(&[
            "am",
            "start",
            "-W",
            "--user",
            "0",
            "-a",
            "android.intent.action.DIAL",
            "-d",
            "tel:",
            "-p",
            "com.android.dialer",
        ])
        .await?;
    if !launch.lines().any(|line| line.trim() == "Status: ok") {
        return Err(format!("Android Dialer did not launch: {}", launch.trim()).into());
    }
    adb.shell(&["cmd", "statusbar", "collapse"]).await?;
    let (x, y) = adb.ui(dialpad_point).await?;
    let (x, y) = (x.to_string(), y.to_string());
    let tap = ["input", "tap", &x, &y];
    let text = ["input", "text", "374"];
    for (action, expected) in [(&tap[..], "1"), (&text[..], "1374")] {
        adb.shell(action).await?;
        adb.ui(|xml| {
            let text = attribute(dialer_node(xml, "com.android.dialer:id/digits")?, "text")?;
            text.chars()
                .filter(char::is_ascii_digit)
                .eq(expected.chars())
                .then_some(())
        })
        .await?;
    }
    let after = adb.snapshot().await?;
    if before == after {
        return Err("The Android frame did not change after input".into());
    }
    println!(
        "{}",
        json!({"acceleration":"whpx", "rootVerified":true, "serial":boot.serial, "width":boot.width, "height":boot.height, "before":before.len(), "after":after.len(), "tap":"1", "input":"374", "readback":"1374", "elapsed_ms":started.elapsed().as_millis()})
    );
    Ok(())
}

fn diagnostics(avd: &Path) {
    for (path, kernel) in [
        (avd.join("../native/sdk.log"), false),
        (avd.join("cuttlefish/kernel.log"), true),
    ] {
        if let Ok(text) = fs::read_to_string(path) {
            if kernel {
                for line in text
                    .lines()
                    .filter(|line| {
                        [
                            "service 'console'",
                            "adbd",
                            "sys.boot_completed",
                            "Starting kernel",
                        ]
                        .iter()
                        .any(|name| line.contains(name))
                    })
                    .take(30)
                {
                    eprintln!("{line}");
                }
            }
            let mut tail: Vec<_> = text.lines().rev().take(80).collect();
            tail.reverse();
            for line in tail {
                eprintln!("{line}");
            }
        }
    }
}

async fn qualify() -> Result<()> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let [capsule, receipt, home] = arguments.as_slice() else {
        return Err("Provide the Android capsule, its receipt, and a clean test home".into());
    };
    let receipt: CapsuleReceipt = serde_json::from_reader(fs::File::open(receipt)?)?;
    let home = env::current_dir()?.join(home);
    let started = Instant::now();
    let phone = Installation::lock(&home)?
        .prepare_android(
            Path::new(capsule),
            &receipt.archive_sha256,
            Resources {
                cpus: 2,
                memory_mb: 2048,
                storage_gb: 16,
            },
            &|path| fs2::available_space(path).unwrap_or(0),
            &|| false,
        )?
        .wire()?;
    println!(
        "Local Android SDK prepared in {} ms",
        started.elapsed().as_millis()
    );
    let adb = Adb(Path::new(&phone.sdk_root).join("platform-tools/adb.exe"));
    let avd = phone.avd_home.clone();
    let mut worker = Command::new(env::current_exe()?.with_file_name("host-resources.exe"))
        .arg("--android-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = worker.stdin.take().expect("piped Android control");
    let mut output = worker.stdout.take().expect("piped Android receipts");
    let started = Instant::now();
    let result = async {
        native_ipc::write_bounded(
            &mut input,
            &NativeAndroidStart {
                phone: Some(phone),
                ..Default::default()
            },
            16384,
        )
        .await?;
        let reply: NativeAndroidSessionReply = timeout(
            Duration::from_secs(150),
            native_ipc::read_bounded(&mut output, 512),
        )
        .await??;
        match reply.result {
            Some(Reply::Booted(boot)) => check(&adb, boot, started).await,
            Some(Reply::Failure(failure)) => {
                Err(format!("Android boot failed: {}", failure.code).into())
            }
            None => Err("Android session returned no boot receipt".into()),
        }
    }
    .await;
    let stop = async {
        if worker.try_wait()?.is_none() {
            native_ipc::write_bounded(
                &mut input,
                &NativeAndroidSessionControl {
                    cancel_startup: true,
                    ..Default::default()
                },
                512,
            )
            .await?;
        }
        drop(input);
        timeout(Duration::from_secs(25), worker.wait()).await?
    }
    .await;
    if result.is_err() {
        diagnostics(Path::new(&avd));
    }
    result?;
    let status = stop?;
    if !status.success() {
        return Err(format!("Android owner did not stop cleanly: {status}").into());
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if !cfg!(windows) {
        return Err("WHPX qualification requires Windows".into());
    }
    qualify().await
}
