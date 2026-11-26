use std::path::Path;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperError,
    WhisperState,
};

#[derive(Debug)]
pub enum Error {
    UnsupportedLanguage,
    Engine(WhisperError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedLanguage => formatter.write_str("本机识别不支持此语言"),
            Self::Engine(error) => write!(formatter, "本机识别失败：{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<WhisperError> for Error {
    fn from(error: WhisperError) -> Self {
        Self::Engine(error)
    }
}

pub struct LocalSpeech {
    state: WhisperState,
    language: &'static str,
}

impl LocalSpeech {
    pub fn new(model_path: impl AsRef<Path>, language: &str) -> Result<Self, Error> {
        let primary = language.split('-').next().unwrap_or_default();
        if !primary.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(Error::UnsupportedLanguage);
        }
        let language = whisper_rs::get_lang_id(&primary.to_ascii_lowercase())
            .and_then(whisper_rs::get_lang_str)
            .ok_or(Error::UnsupportedLanguage)?;
        whisper_rs::install_logging_hooks();
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(false);
        let context = WhisperContext::new_with_params(model_path, parameters)?;
        let state = context.create_state()?;
        Ok(Self { state, language })
    }

    /// Accepts 16 kHz mono signed PCM16. The loaded model and state are reused.
    pub fn transcribe(&mut self, samples: &[i16], final_text: bool) -> Result<String, Error> {
        let audio: Vec<f32> = samples
            .iter()
            .map(|sample| f32::from(*sample) / 32768.0)
            .collect();
        let mut parameters = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        parameters.set_n_threads(4);
        parameters.set_language(Some(self.language));
        parameters.set_translate(false);
        parameters.set_no_context(true);
        parameters.set_no_timestamps(true);
        parameters.set_print_special(false);
        parameters.set_print_progress(false);
        parameters.set_print_realtime(false);
        parameters.set_print_timestamps(false);
        if !final_text {
            parameters.set_temperature_inc(0.0);
            parameters.set_single_segment(true);
        }
        self.state.full(parameters, &audio)?;
        let mut transcript = Vec::new();
        for segment in self.state.as_iter() {
            transcript.extend_from_slice(segment.to_bytes()?);
        }
        Ok(String::from_utf8_lossy(&transcript).trim().to_owned())
    }
}
