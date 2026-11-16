use crate::{
    connection::Connection,
    controls::{Icon, sized_icon_button},
    locale::Locale,
    style,
};
use iced::{
    Element, Task,
    widget::{row, slider, text, tooltip},
};
use rodio::{OutputStreamBuilder, Source};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const SAMPLES_PER_MS: usize = RATE as usize * CHANNELS as usize / 1_000;
const REFILL: usize = 10 * SAMPLES_PER_MS;
const PREBUFFER: usize = 60 * SAMPLES_PER_MS;
const PACKET_LIMIT: usize = 200 * SAMPLES_PER_MS;
const QUEUE_LIMIT: usize = 350 * SAMPLES_PER_MS;

#[derive(Clone, Debug)]
pub enum Message {
    Toggle,
    Volume(u8),
    Stream(u64, StreamState),
    PreferenceFailed(String),
}

#[derive(Clone, Debug)]
pub enum StreamState {
    Playing,
    Reconnecting(String),
    Stopped(String),
}

pub struct Playback {
    volume: u8,
    audible: u8,
    preference: Option<watch::Sender<u8>>,
    active: Option<Active>,
    generation: u64,
    error: String,
}

struct Active {
    job: iced::task::Handle,
    buffer: Arc<Buffer>,
}

impl Playback {
    pub fn new() -> Self {
        Self {
            volume: 100,
            audible: 100,
            preference: None,
            active: None,
            generation: 0,
            error: String::new(),
        }
    }

    pub fn restore(&mut self, root: &str, device_id: &str) -> Task<Message> {
        self.stop();
        let directory = Path::new(root).join("data/native/audio");
        let path = directory.join(device_id);
        self.volume = std::fs::read_to_string(&path)
            .ok()
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(100)
            .min(100);
        self.audible = if self.volume > 0 { self.volume } else { 100 };
        let (preference, mut updates) = watch::channel(self.volume);
        self.preference = Some(preference);
        Task::future(async move {
            tokio::fs::create_dir_all(directory).await?;
            let temporary = path.with_extension("tmp");
            while updates.changed().await.is_ok() {
                let value = updates.borrow_and_update().to_string();
                tokio::fs::write(&temporary, value).await?;
                tokio::fs::rename(&temporary, &path).await?;
            }
            Ok::<_, std::io::Error>(())
        })
        .then(|result| match result {
            Ok(()) => Task::none(),
            Err(error) => Task::done(Message::PreferenceFailed(error.to_string())),
        })
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn stop(&mut self) {
        if let Some(active) = self.active.take() {
            active.buffer.volume.store(0, Relaxed);
            active.job.abort();
        }
        self.error.clear();
    }

    fn change_volume(&mut self, volume: u8) {
        self.volume = volume;
        if volume > 0 {
            self.audible = volume;
        }
        if let Some(preference) = &self.preference {
            preference.send_replace(volume);
        }
        if let Some(active) = &self.active {
            active.buffer.volume.store(volume, Relaxed);
        }
    }

    pub fn update(
        &mut self,
        message: Message,
        client: Option<&Connection>,
        index: usize,
    ) -> Task<Message> {
        match message {
            Message::Toggle => {
                if self.active.is_some() && self.volume > 0 {
                    self.stop();
                    return Task::none();
                }
                if self.volume == 0 {
                    self.change_volume(self.audible);
                }
            }
            Message::Volume(volume) => self.change_volume(volume),
            Message::Stream(generation, state) => {
                if generation == self.generation && self.active.is_some() {
                    match state {
                        StreamState::Playing => self.error.clear(),
                        StreamState::Reconnecting(error) => self.error = error,
                        StreamState::Stopped(error) => {
                            self.stop();
                            self.error = error;
                        }
                    }
                }
                return Task::none();
            }
            Message::PreferenceFailed(error) => {
                self.error = format!("音量保存失败：{error}");
                return Task::none();
            }
        }
        let Some(client) = client.filter(|_| self.volume > 0 && self.active.is_none()) else {
            return Task::none();
        };
        self.generation += 1;
        let generation = self.generation;
        let client = client.clone();
        let buffer = Arc::new(Buffer::new(self.volume));
        let playback = buffer.clone();
        let (task, job) = Task::run(
            iced::stream::channel(4, move |output| play(client, index, playback, output)),
            move |state| Message::Stream(generation, state),
        )
        .abortable();
        self.active = Some(Active { job, buffer });
        self.error.clear();
        task
    }

    pub fn view(&self, enabled: bool, index: usize, locale: Locale) -> Element<'_, Message> {
        let volume = if self.active.is_some() {
            self.volume
        } else {
            0
        };
        let muted = volume == 0;
        let button = sized_icon_button(
            if muted { Icon::Muted } else { Icon::Volume },
            locale.tr(if muted { "开启声音" } else { "静音" }),
            enabled.then_some(Message::Toggle),
            32.,
            34.,
        );
        let range = tooltip(
            slider(0..=100, volume, Message::Volume)
                .width(60)
                .height(16)
                .style(style::range),
            text(format!(
                "{} {volume}%",
                locale.tr(if index == 0 {
                    "手机播放音量"
                } else {
                    "电脑播放音量"
                })
            ))
            .size(12),
            tooltip::Position::Bottom,
        );
        row![button, range].align_y(iced::Alignment::Center).into()
    }
}

struct Buffer {
    samples: Mutex<VecDeque<i16>>,
    volume: AtomicU8,
    played_signal: AtomicBool,
}

impl Buffer {
    fn new(volume: u8) -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(QUEUE_LIMIT)),
            volume: AtomicU8::new(volume),
            played_signal: AtomicBool::new(false),
        }
    }
}

#[derive(Default)]
struct Pcm {
    pending: Vec<u8>,
}

impl Pcm {
    fn push(&mut self, bytes: &[u8], output: &Buffer) -> bool {
        self.pending.extend_from_slice(bytes);
        let end = self.pending.len() / 4 * 4;
        if end == 0 {
            return false;
        }
        let start = end.saturating_sub(PACKET_LIMIT * size_of::<i16>());
        let mut queue = output.samples.lock().expect("audio buffer");
        if queue.len() + (end - start) / size_of::<i16>() > QUEUE_LIMIT {
            queue.clear();
        }
        queue.extend(
            self.pending[start..end]
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]])),
        );
        let carry = self.pending.len() - end;
        self.pending.copy_within(end.., 0);
        self.pending.truncate(carry);
        true
    }
}

struct LiveSource {
    buffer: Arc<Buffer>,
    block: [i16; REFILL],
    index: usize,
    buffering: bool,
    gain: f32,
    gain_step: f32,
}

impl LiveSource {
    fn new(buffer: Arc<Buffer>) -> Self {
        Self {
            buffer,
            block: [0; REFILL],
            index: REFILL,
            buffering: true,
            gain: 0.,
            gain_step: 0.,
        }
    }

    fn refill(&mut self) {
        self.block.fill(0);
        if let Ok(mut samples) = self.buffer.samples.try_lock() {
            let minimum = if self.buffering { PREBUFFER } else { REFILL };
            if samples.len() >= minimum {
                for (slot, sample) in self.block.iter_mut().zip(samples.drain(..REFILL)) {
                    *slot = sample;
                }
                self.buffering = false;
            } else {
                self.buffering = true;
            }
        }
        let volume = f32::from(self.buffer.volume.load(Relaxed)) / 100.;
        self.gain_step = (volume - self.gain) / REFILL as f32;
        self.index = 0;
    }
}

impl Iterator for LiveSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.index == REFILL {
            self.refill();
        }
        let sample = f32::from(self.block[self.index]) * (self.gain / 32_768.);
        self.index += 1;
        self.gain += self.gain_step;
        if sample.abs() > 0.001 && !self.buffer.played_signal.load(Relaxed) {
            self.buffer.played_signal.store(true, Relaxed);
        }
        Some(sample)
    }
}

impl Source for LiveSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        CHANNELS
    }

    fn sample_rate(&self) -> u32 {
        RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

async fn play(
    client: Connection,
    index: usize,
    buffer: Arc<Buffer>,
    mut output: iced::futures::channel::mpsc::Sender<StreamState>,
) {
    use iced::futures::SinkExt;
    let started = Instant::now();
    let (stop, closed) = oneshot::channel::<()>();
    let (opened, ready) = oneshot::channel();
    let (errors, mut device_error) = mpsc::channel(1);
    let playback = buffer.clone();
    tokio::task::spawn_blocking(move || {
        let stream = OutputStreamBuilder::from_default_device().and_then(|builder| {
            builder
                .with_error_callback(move |error| {
                    let _ = errors.try_send(error.to_string());
                })
                .open_stream()
        });
        match stream {
            Ok(mut stream) => {
                stream.log_on_drop(false);
                stream.mixer().add(LiveSource::new(playback));
                if opened.send(Ok(())).is_ok() {
                    let _ = closed.blocking_recv();
                }
            }
            Err(error) => {
                let _ = opened.send(Err(format!("声音未开启：{error}")));
            }
        }
    });
    if let Err(error) = ready.await.unwrap_or_else(|_| Err("声音未开启".into())) {
        let _ = output.send(StreamState::Stopped(error)).await;
        return;
    }
    let error = tokio::select! {
        error = device_error.recv() => error.map(|error| format!("声音播放已暂停：{error}")),
        error = stream_audio(&client, index, &buffer, &mut output, started) => error,
    };
    drop(stop);
    if let Some(error) = error {
        let _ = output.send(StreamState::Stopped(error)).await;
    }
}

async fn stream_audio(
    client: &Connection,
    index: usize,
    buffer: &Buffer,
    output: &mut iced::futures::channel::mpsc::Sender<StreamState>,
    started: Instant,
) -> Option<String> {
    use iced::futures::{SinkExt, StreamExt};
    let mut reported_signal = false;
    loop {
        let attempt = async {
            let response = client.audio_stream(index).await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Ok(Some("请重新登录".into()));
            }
            let mut stream = response
                .error_for_status()
                .map_err(|error| error.to_string())?
                .bytes_stream();
            let mut pcm = Pcm::default();
            let mut ready = false;
            while let Some(part) = stream.next().await {
                if pcm.push(&part.map_err(|error| error.to_string())?, buffer) && !ready {
                    output
                        .send(StreamState::Playing)
                        .await
                        .map_err(|error| error.to_string())?;
                    ready = true;
                }
                if !reported_signal && buffer.played_signal.load(Relaxed) {
                    reported_signal = true;
                    eprintln!(
                        "native_metric audio_playback device={index} first_signal_ms={:.3}",
                        started.elapsed().as_secs_f64() * 1000.
                    );
                }
            }
            Err("声音连接已断开".to_string())
        }
        .await;
        match attempt {
            Ok(terminal) => return terminal,
            Err(error) => {
                buffer.samples.lock().expect("audio buffer").clear();
                if output.send(StreamState::Reconnecting(error)).await.is_err() {
                    return None;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_keeps_stereo_aligned_and_discards_audio_after_network_backlog() {
        let buffer = Buffer::new(100);
        let mut pcm = Pcm::default();
        assert!(!pcm.push(&[1, 0, 2], &buffer));
        assert!(pcm.push(&[0, 3, 0, 4, 0], &buffer));
        assert_eq!(*buffer.samples.lock().unwrap(), [1, 2, 3, 4]);

        let packet = [0xff, 0x7f, 0, 0x80].repeat(PACKET_LIMIT / 2);
        pcm.push(&packet, &buffer);
        pcm.push(&packet, &buffer);
        assert_eq!(buffer.samples.lock().unwrap().len(), PACKET_LIMIT);

        let mut output = LiveSource::new(Arc::new(buffer));
        let audible: Vec<_> = output.by_ref().skip(REFILL).take(2).collect();
        assert!(audible[0] > 0.99);
        assert!(audible[1] < -0.99);
    }
}
