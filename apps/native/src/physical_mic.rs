use crate::{device_io::Result, workspace::Workspace};
use cpal::{
    FromSample, Sample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{io, sync::Arc};
use tokio::sync::mpsc;

pub(crate) async fn permission() -> std::result::Result<(), String> {
    #[cfg(target_os = "android")]
    crate::android_os::microphone().await?;
    #[cfg(target_os = "macos")]
    tokio::task::spawn_blocking(|| -> std::result::Result<(), String> {
        use block2::RcBlock;
        use objc2::runtime::Bool;
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
        let media = unsafe { AVMediaTypeAudio }.expect("macOS audio media type");
        let granted = match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) } {
            AVAuthorizationStatus::Authorized => true,
            AVAuthorizationStatus::NotDetermined => {
                let (answer, result) = std::sync::mpsc::channel();
                let reply = RcBlock::new(move |allowed: Bool| {
                    let _ = answer.send(allowed.as_bool());
                });
                unsafe {
                    AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &reply)
                };
                result.recv().map_err(|e| e.to_string())?
            }
            _ => false,
        };
        if !granted {
            return Err("麦克风权限未开启".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    #[cfg(windows)]
    {
        use windows::{
            Security::Authorization::AppCapabilityAccess::{
                AppCapability, AppCapabilityAccessStatus as Status,
            },
            core::HSTRING,
        };
        let cap = AppCapability::Create(&HSTRING::from("microphone")).map_err(|e| e.to_string())?;
        let status = cap.CheckAccess().map_err(|e| e.to_string())?;
        if status == Status::UserPromptRequired
            && cap
                .RequestAccessAsync()
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())?
                != Status::Allowed
            || matches!(status, Status::DeniedBySystem | Status::DeniedByUser)
        {
            return Err("请在 Windows 隐私设置中允许桌面应用使用麦克风".into());
        }
    }
    Ok(())
}

type Sender = mpsc::Sender<io::Result<bytes::Bytes>>;
fn stream<T: SizedSample + Sample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: Sender,
) -> Result<cpal::Stream>
where
    i16: FromSample<T>,
{
    let rate = config.sample_rate.0;
    let channels = usize::from(config.channels);
    let mut phase = 0u32;
    let err = tx.clone();
    let mut overrun = false;
    Ok(device.build_input_stream(
        config,
        move |input: &[T], _| {
            if overrun {
                let _ = tx.try_send(Err(io::Error::other("设备接收音频过慢，麦克风已断开")));
                return;
            }
            let mut pcm = Vec::with_capacity(input.len() / channels * 2);
            for frame in input.chunks_exact(channels) {
                let sample = (frame
                    .iter()
                    .map(|&x| i32::from(x.to_sample::<i16>()))
                    .sum::<i32>()
                    / channels as i32) as i16;
                phase += 48_000;
                while phase >= rate {
                    phase -= rate;
                    pcm.extend_from_slice(&sample.to_le_bytes());
                }
            }
            if !pcm.is_empty() {
                overrun = matches!(
                    tx.try_send(Ok(pcm.into())),
                    Err(mpsc::error::TrySendError::Full(_))
                );
            }
        },
        move |e| {
            let _ = err.try_send(Err(io::Error::other(e.to_string())));
        },
        None,
    )?)
}

pub(crate) async fn route(w: Arc<Workspace>, id: String) -> Result<()> {
    let source = w
        .host_microphone
        .clone()
        .try_acquire_owned()
        .map_err(|_| "本机麦克风已连接其他设备，请先断开该设备的麦克风")?;
    permission().await?;
    let mut lane = w.microphone_lane(&id, false, None).await?;
    let (tx, rx) = mpsc::channel(8);
    let (hold, end) = std::sync::mpsc::channel::<()>();
    let (started, ready) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("device-microphone".into())
        .spawn(move || {
            let _source = source;
            let start = || -> Result<cpal::Stream> {
                let device = cpal::default_host()
                    .default_input_device()
                    .ok_or("未检测到系统麦克风")?;
                let supported = device.default_input_config()?;
                let config = supported.config();
                let audio = match supported.sample_format() {
                    SampleFormat::F32 => stream::<f32>(&device, &config, tx),
                    SampleFormat::F64 => stream::<f64>(&device, &config, tx),
                    SampleFormat::I16 => stream::<i16>(&device, &config, tx),
                    SampleFormat::U16 => stream::<u16>(&device, &config, tx),
                    SampleFormat::I32 => stream::<i32>(&device, &config, tx),
                    SampleFormat::U32 => stream::<u32>(&device, &config, tx),
                    SampleFormat::I8 => stream::<i8>(&device, &config, tx),
                    SampleFormat::U8 => stream::<u8>(&device, &config, tx),
                    x => return Err(format!("Unsupported microphone format: {x}").into()),
                }?;
                audio.play()?;
                Ok(audio)
            };
            match start() {
                Ok(_audio) => {
                    if started.send(Ok(())).is_ok() {
                        let _ = end.recv();
                    }
                }
                Err(e) => {
                    let _ = started.send(Err(e.to_string()));
                }
            }
        })?;
    ready.await??;
    let frames =
        futures_util::stream::unfold(rx, |mut rx| async move { Some((rx.recv().await?, rx)) });
    let mut input = tokio_util::io::StreamReader::new(Box::pin(frames));
    let result = lane.upload(&mut input).await;
    drop(hold);
    result
}
