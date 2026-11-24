use crate::{Result, Transport, reader, send_reader};
use ai_phone_adb::{Client, Limits};
use std::{io, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::WebSocketStream;

const READY: u32 = 0x41554d31;
const DONE: u32 = 0x444f4e45;

pub(super) async fn transfer<S: Transport>(
    adb: &Client,
    ws: &mut WebSocketStream<S>,
    capture: bool,
    wav: bool,
) -> Result<()> {
    let mode = if capture { "speaker" } else { "mic" };
    let package = adb
        .shell(
            "pm path dev.aiphone.input && getprop ro.product.cpu.abi",
            Limits {
                stdout: 2048,
                stderr: 256,
            },
            Duration::from_secs(5),
        )
        .await?;
    let lines = std::str::from_utf8(&package.stdout)?;
    let mut lines = lines.lines();
    let apk = lines
        .next()
        .and_then(|line| line.trim().strip_prefix("package:"))
        .ok_or("找不到已安装的来宾音频组件")?;
    let directory = apk
        .strip_suffix("/base.apk")
        .filter(|d| d.starts_with("/data/app/"))
        .ok_or("来宾音频组件的安装路径无效")?;
    let abi = match lines.next().map(str::trim) {
        Some("arm64-v8a") => "arm64",
        Some("x86_64") => "x86_64",
        _ => return Err("来宾音频组件不支持此架构".into()),
    };
    let library = crate::files::quoted(&format!("{directory}/lib/{abi}/libaimanity_input.so"));
    let command = format!(
        "exec:exec su 1000 env CLASSPATH={} app_process -Daimanity.audio.library={library} / dev.aiphone.input.VirtualAudio {mode} 2>/dev/null",
        crate::files::quoted(apk)
    );
    let mut guest = adb.open_stream(&command, Duration::from_secs(8)).await?;
    if tokio::time::timeout(Duration::from_secs(12), guest.read_u32()).await?? != READY {
        return Err("Android 音频路由未就绪".into());
    }
    if capture {
        return send_reader(ws, &mut guest).await;
    }
    let mut input = reader(ws);
    let format = if wav {
        Wav::read(&mut input).await?
    } else {
        Wav {
            bits: 16,
            channels: 1,
            rate: 48000,
            float: false,
            length: None,
        }
    };
    let frame_bytes = usize::from(format.bits / 8) * usize::from(format.channels);
    let mut bytes = vec![0; frame_bytes * 1024];
    let mut remainder = format.length;
    let mut used = 0;
    let mut phase = 0;
    let mut pcm = Vec::new();
    loop {
        let size = remainder.map_or(bytes.len() - used, |n| (n as usize).min(bytes.len() - used));
        let n = input.read(&mut bytes[used..used + size]).await?;
        if n == 0 {
            if remainder.is_some_and(|left| left != 0) || used != 0 {
                return Err("音频样本不完整".into());
            }
            break;
        }
        if let Some(left) = &mut remainder {
            *left -= n as u32;
        }
        used += n;
        let whole = used - used % frame_bytes;
        pcm.clear();
        for frame in bytes[..whole].chunks_exact(frame_bytes) {
            let sample = frame
                .chunks_exact(usize::from(format.bits / 8))
                .map(|s| format.sample(s))
                .sum::<i64>()
                / i64::from(format.channels);
            phase += 48_000;
            while phase >= format.rate {
                pcm.extend_from_slice(&(sample as i16).to_le_bytes());
                phase -= format.rate;
            }
        }
        for block in pcm.chunks(8192) {
            guest.write_u32(block.len() as u32).await?;
            guest.write_all(block).await?;
        }
        bytes.copy_within(whole..used, 0);
        used -= whole;
    }
    if wav {
        tokio::io::copy(&mut input, &mut tokio::io::sink()).await?;
    }
    guest.write_u32(0).await?;
    guest.flush().await?;
    if tokio::time::timeout(Duration::from_secs(8), guest.read_u32()).await?? != DONE {
        return Err("Android 麦克风未确认送入完成".into());
    }
    Ok(())
}

struct Wav {
    bits: u16,
    channels: u16,
    rate: u32,
    float: bool,
    length: Option<u32>,
}

impl Wav {
    async fn read(input: &mut (impl AsyncRead + Unpin)) -> Result<Self> {
        let mut riff = [0; 12];
        input.read_exact(&mut riff).await?;
        if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
            return Err("需要 WAV 音频文件".into());
        }
        let mut format = None;
        loop {
            let mut name = [0; 4];
            input.read_exact(&mut name).await?;
            let length = input.read_u32_le().await?;
            let mut section = (&mut *input).take(length.into());
            match &name {
                b"fmt " => {
                    let encoding = section.read_u16_le().await?;
                    let channels = section.read_u16_le().await?;
                    let rate = section.read_u32_le().await?;
                    let _bytes_per_second = section.read_u32_le().await?;
                    let align = section.read_u16_le().await?;
                    let bits = section.read_u16_le().await?;
                    if !matches!((encoding, bits), (1, 8 | 16 | 24 | 32) | (3, 32))
                        || !(1..=8).contains(&channels)
                        || !(8000..=192000).contains(&rate)
                        || align != channels * (bits / 8)
                    {
                        return Err("WAV 音频格式不支持".into());
                    }
                    format = Some(Self {
                        bits,
                        channels,
                        rate,
                        float: encoding == 3,
                        length: None,
                    });
                }
                b"data" => {
                    let mut f = format.ok_or("缺少 WAV 格式信息")?;
                    f.length = Some(length);
                    return Ok(f);
                }
                _ => {}
            }
            tokio::io::copy(&mut section, &mut tokio::io::sink()).await?;
            if section.limit() != 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            if length % 2 != 0 {
                input.read_u8().await?;
            }
        }
    }
    fn sample(&self, bytes: &[u8]) -> i64 {
        match bytes {
            [a] => (i64::from(*a) - 128) << 8,
            [a, b] => i16::from_le_bytes([*a, *b]).into(),
            [_, b, c] => i16::from_le_bytes([*b, *c]).into(),
            [a, b, c, d] if self.float => {
                (f32::from_le_bytes([*a, *b, *c, *d]).clamp(-1., 1.) * 32767.) as i64
            }
            [_, _, c, d] => i16::from_le_bytes([*c, *d]).into(),
            _ => unreachable!(),
        }
    }
}
