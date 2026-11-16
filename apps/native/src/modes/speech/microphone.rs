use super::Event;
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

const RATE: u32 = 16_000;
const FRAME: usize = RATE as usize / 50;

pub fn events(
    model: PathBuf,
    language: String,
    cancel: Arc<AtomicBool>,
) -> impl Stream<Item = Event> {
    iced::stream::channel(12, async move |mut output| {
        let (sender, mut events) = tokio::sync::mpsc::channel(12);
        let consumer = cancel.clone();
        let spawn = std::thread::Builder::new()
            .name("aimanity-microphone".into())
            .spawn(move || {
                if let Err(error) = capture(model, &language, &consumer, &sender) {
                    let _ = sender.blocking_send(Event::Error(error));
                }
            });
        if let Err(error) = spawn {
            let _ = output.send(Event::Error(error.to_string())).await;
            return;
        }
        while let Some(event) = events.recv().await {
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
) -> Result<(), String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("无法读取麦克风")?;
    let supported = device.default_input_config().map_err(|error| {
        eprintln!("native microphone configuration: {error}");
        "无法读取麦克风"
    })?;
    let format = supported.sample_format();
    let config = supported.config();
    let mut decoder = LocalSpeech::new(model, language).map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let (sender, receiver) = mpsc::sync_channel(256);
    let stream = match format {
        SampleFormat::F32 => stream::<f32>(&device, &config, sender),
        SampleFormat::F64 => stream::<f64>(&device, &config, sender),
        SampleFormat::I16 => stream::<i16>(&device, &config, sender),
        SampleFormat::U16 => stream::<u16>(&device, &config, sender),
        SampleFormat::I32 => stream::<i32>(&device, &config, sender),
        SampleFormat::U32 => stream::<u32>(&device, &config, sender),
        _ => return Err(format!("麦克风采样格式不支持：{format}")),
    }?;
    stream.play().map_err(|error| {
        eprintln!("native microphone start: {error}");
        "麦克风开启失败"
    })?;
    events
        .blocking_send(Event::Ready)
        .map_err(|e| e.to_string())?;
    let mut segment = Segment::default();
    while !cancel.load(Ordering::Acquire) {
        let frame = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(data) => data?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("麦克风已断开".into()),
        };
        if let Some(event) = segment.push(frame, &mut decoder)? {
            events.blocking_send(event).map_err(|e| e.to_string())?;
        }
    }
    drop(stream);
    Ok(())
}

type Frame = [i16; FRAME];
type Sender = mpsc::SyncSender<Result<Frame, String>>;

fn stream<T: SizedSample + Sample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: Sender,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    if rate < RATE {
        return Err("麦克风采样率低于 16 kHz".into());
    }
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
                        numerator -= rate;
                        frame[write] = (sum / count as f32).to_sample::<i16>();
                        sum = 0.0;
                        count = 0;
                        write += 1;
                        if write == FRAME {
                            if sender.try_send(Ok(frame)).is_err() {
                                overrun = true;
                                break;
                            }
                            write = 0;
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
    before: Vec<i16>,
    id: String,
    revision: u32,
    quiet: usize,
    voice: usize,
    decoded: usize,
    floor: f32,
}
impl Segment {
    fn push(&mut self, frame: Frame, decoder: &mut LocalSpeech) -> Result<Option<Event>, String> {
        let power = frame
            .iter()
            .map(|sample| (f32::from(*sample) / 32768.).powi(2))
            .sum::<f32>();
        let level = (power / FRAME as f32).sqrt();
        let voiced = level > (self.floor * 3.).max(0.008);
        if voiced {
            self.quiet = 0;
            self.voice += FRAME;
        } else {
            self.quiet += FRAME;
            self.floor = self.floor * 0.95 + level * 0.05;
        }
        if self.samples.is_empty() && !voiced {
            self.before = frame.to_vec();
            return Ok(None);
        }
        if self.samples.is_empty() {
            self.samples.append(&mut self.before);
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
        let final_text = self.quiet >= rate * 65 / 100 || self.samples.len() >= rate * 28;
        let heard = self.samples.len() - self.quiet;
        if !final_text && heard < self.decoded + rate / 2 {
            return Ok(None);
        }
        let text = decoder
            .transcribe(&self.samples[..self.samples.len().min(heard + rate * 15 / 100)])
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
