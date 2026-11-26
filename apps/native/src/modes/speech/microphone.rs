use super::{Event, MicrophoneLevels};
use ai_phone_local_speech::LocalSpeech;
use cpal::{
    FromSample, Sample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use futures_util::{SinkExt, Stream};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::sync::watch;

const RATE: u32 = 16_000;
const FRAME: usize = RATE as usize / 50;
const MIN_VOICE_RMS: f32 = 0.005;
const METER_RMS: f32 = 0.008;

pub fn events(
    model: PathBuf,
    language: String,
    cancel: Arc<AtomicBool>,
) -> impl Stream<Item = Event> {
    iced::stream::channel(12, async move |mut output| {
        if let Err(error) = crate::physical_mic::permission().await {
            let _ = output.send(Event::Error(error)).await;
            return;
        }
        let (sender, mut events) = tokio::sync::mpsc::channel(12);
        let (levels, mut updates) = watch::channel(MicrophoneLevels::default());
        let consumer = cancel.clone();
        let spawn = std::thread::Builder::new()
            .name("aimanity-microphone".into())
            .spawn(move || {
                #[cfg(target_os = "android")]
                if let Err(e) = model
                    .parent()
                    .and_then(|d| d.parent())
                    .ok_or_else(|| std::io::Error::other("Missing voice assets"))
                    .and_then(|resources| {
                        crate::deployment::android_assets::stage(resources, "speech", &|| {
                            consumer.load(Ordering::Acquire)
                        })
                    })
                {
                    let _ = sender.blocking_send(Event::Error(e.to_string()));
                    return;
                }
                if let Err(error) = capture(model, &language, &consumer, &sender, levels) {
                    let _ = sender.blocking_send(Event::Error(error));
                }
            });
        if let Err(error) = spawn {
            let _ = output.send(Event::Error(error.to_string())).await;
            return;
        }
        loop {
            let event = tokio::select! {
                biased;
                event = events.recv() => match event {
                    Some(event) => event,
                    None => return,
                },
                Ok(()) = updates.changed() => Event::Levels(*updates.borrow_and_update()),
            };
            if cancel.load(Ordering::Acquire) {
                return;
            }
            if output.send(event).await.is_err() {
                cancel.store(true, Ordering::Release);
                return;
            }
        }
    })
}

fn capture(
    model: PathBuf,
    language: &str,
    cancel: &AtomicBool,
    events: &tokio::sync::mpsc::Sender<Event>,
    levels: watch::Sender<MicrophoneLevels>,
) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("未检测到系统麦克风")?;
    let supported = device.default_input_config().map_err(|error| {
        eprintln!("native microphone configuration: {error}");
        "无法读取麦克风"
    })?;
    let format = supported.sample_format();
    let config = supported.config();
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let (sender, receiver) = mpsc::sync_channel(256);
    let stream = match format {
        SampleFormat::F32 => stream::<f32>(&device, &config, sender, levels),
        SampleFormat::F64 => stream::<f64>(&device, &config, sender, levels),
        SampleFormat::I16 => stream::<i16>(&device, &config, sender, levels),
        SampleFormat::I24 => stream::<cpal::I24>(&device, &config, sender, levels),
        SampleFormat::U16 => stream::<u16>(&device, &config, sender, levels),
        SampleFormat::I32 => stream::<i32>(&device, &config, sender, levels),
        SampleFormat::U32 => stream::<u32>(&device, &config, sender, levels),
        SampleFormat::I8 => stream::<i8>(&device, &config, sender, levels),
        SampleFormat::U8 => stream::<u8>(&device, &config, sender, levels),
        SampleFormat::I64 => stream::<i64>(&device, &config, sender, levels),
        SampleFormat::U64 => stream::<u64>(&device, &config, sender, levels),
        _ => return Err(format!("麦克风采样格式不支持：{format}")),
    }?;
    stream.play().map_err(|error| {
        eprintln!("native microphone start: {error}");
        "麦克风开启失败"
    })?;
    events
        .blocking_send(Event::Ready)
        .map_err(|e| e.to_string())?;
    let mut decoder = LocalSpeech::new(model, language).map_err(|e| e.to_string())?;
    let mut segment = Segment::default();
    while !cancel.load(Ordering::Acquire) {
        let (frame, rms) = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(data) => data?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("麦克风已断开".into()),
        };
        if let Some(event) = segment.push(frame, rms, &mut decoder)? {
            events.blocking_send(event).map_err(|e| e.to_string())?;
        }
    }
    drop(stream);
    Ok(())
}

type Frame = [i16; FRAME];
type Sender = mpsc::SyncSender<Result<(Frame, f32), String>>;

fn measure(frame: &Frame, levels: &watch::Sender<MicrophoneLevels>) -> f32 {
    let power: f32 = frame.iter().map(|&sample| f32::from(sample).powi(2)).sum();
    let rms = (power / FRAME as f32).sqrt() / 32768.;
    let point = if rms <= METER_RMS {
        0
    } else {
        (rms.sqrt() * 255.).round() as u8
    };
    levels.send_if_modified(|bins| {
        if bins.iter().all(|&bin| bin == point) {
            return false;
        }
        bins.rotate_left(1);
        bins[bins.len() - 1] = point;
        true
    });
    rms
}

fn stream<T: SizedSample + Sample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: Sender,
    levels: watch::Sender<MicrophoneLevels>,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    let errors = sender.clone();
    let mut numerator = 0;
    let mut sum = 0.0;
    let mut count = 0;
    let mut frame = [0; FRAME];
    let mut write = 0;
    let mut overrun = false;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if overrun {
                    let _ = sender.try_send(Err("语音识别积压，录音已暂停".into()));
                    return;
                }
                for row in data.chunks_exact(channels) {
                    sum += row
                        .iter()
                        .map(|sample| sample.to_sample::<f32>())
                        .sum::<f32>()
                        / channels as f32;
                    count += 1;
                    numerator += RATE;
                    if numerator >= rate {
                        let sample = (sum / count as f32).to_sample::<i16>();
                        sum = 0.0;
                        count = 0;
                        while numerator >= rate {
                            numerator -= rate;
                            frame[write] = sample;
                            write += 1;
                            if write == FRAME {
                                let rms = measure(&frame, &levels);
                                if sender.try_send(Ok((frame, rms))).is_err() {
                                    overrun = true;
                                    return;
                                }
                                write = 0;
                            }
                        }
                    }
                }
            },
            move |error| {
                eprintln!("native microphone stream: {error}");
                let _ = errors.try_send(Err("麦克风已断开".into()));
            },
            None,
        )
        .map_err(|error| {
            eprintln!("native microphone open: {error}");
            "麦克风开启失败".into()
        })
}

#[derive(Default)]
struct Segment {
    samples: Vec<i16>,
    before: Option<Frame>,
    id: String,
    revision: u32,
    quiet: usize,
    voice: usize,
    decoded: usize,
    floor: f32,
}
impl Segment {
    fn push(
        &mut self,
        frame: Frame,
        level: f32,
        decoder: &mut LocalSpeech,
    ) -> Result<Option<Event>, String> {
        let voiced = level > (self.floor * 3.).max(MIN_VOICE_RMS);
        if voiced {
            self.quiet = 0;
            self.voice += FRAME;
        } else {
            self.quiet += FRAME;
            self.floor = self.floor * 0.95 + level * 0.05;
        }
        if self.samples.is_empty() && !voiced {
            self.before = Some(frame);
            return Ok(None);
        }
        if let Some(before) = self.before.take() {
            self.samples.extend_from_slice(&before);
        }
        self.samples.extend_from_slice(&frame);
        let rate = RATE as usize;
        if self.id.is_empty() {
            if self.voice >= rate / 10 {
                self.id = uuid::Uuid::now_v7().to_string();
                return Ok(Some(Event::Speech(self.id.clone())));
            }
            if self.quiet >= rate / 5 {
                self.reset();
            }
            return Ok(None);
        }
        let final_text = self.quiet >= rate / 2 || self.samples.len() >= rate * 28;
        let heard = self.samples.len() - self.quiet;
        let next_draft = if self.decoded == 0 {
            rate * 2 / 5
        } else {
            self.decoded + rate / 2
        };
        if !final_text && heard < next_draft {
            return Ok(None);
        }
        let text = decoder
            .transcribe(
                &self.samples[..self.samples.len().min(heard + rate * 15 / 100)],
                final_text,
            )
            .map_err(|error| error.to_string())?;
        self.decoded = heard;
        self.revision += 1;
        let event = Event::Transcript {
            id: self.id.clone(),
            revision: self.revision,
            text,
            final_text,
        };
        if final_text {
            self.reset();
        }
        Ok(Some(event))
    }
    fn reset(&mut self) {
        self.samples.clear();
        self.id.clear();
        self.revision = 0;
        self.voice = 0;
        self.quiet = 0;
        self.decoded = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_tracks_input_level_and_stops_redrawing_during_silence() {
        let (levels, mut updates) = watch::channel(MicrophoneLevels::default());
        let silence = [0; FRAME];
        assert_eq!(measure(&silence, &levels), 0.);
        for peak in [120_i16, 180, 250] {
            let room_noise = std::array::from_fn(|i| if i % 2 == 0 { peak } else { -peak });
            assert!(measure(&room_noise, &levels) < METER_RMS);
        }
        assert!(!updates.has_changed().unwrap());
        let quiet = std::array::from_fn(|i| if i % 2 == 0 { 1024 } else { -512 });
        let loud = quiet.map(|sample| sample * 8);
        let soft_rms = measure(&quiet, &levels);
        let loud_rms = measure(&loud, &levels);
        assert!((loud_rms / soft_rms - 8.).abs() < 0.001);
        {
            let history = updates.borrow_and_update();
            let soft = history[history.len() - 2];
            let loud = history[history.len() - 1];
            assert!(loud > soft && soft > 0);
        }
        for _ in 0..MicrophoneLevels::default().len() {
            measure(&silence, &levels);
        }
        assert_eq!(*updates.borrow_and_update(), MicrophoneLevels::default());
        measure(&silence, &levels);
        assert!(!updates.has_changed().unwrap());
    }
}
