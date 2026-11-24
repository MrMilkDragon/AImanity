use tokio::sync::oneshot;

pub fn readable(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    for content in value.split("```").step_by(2) {
        let mut rest = content;
        while let Some((before, after)) = rest.split_once('[') {
            text.extend(before.chars().filter(|c| !"*_`#".contains(*c)));
            if let Some((label, url)) = after.split_once("](") {
                text.push_str(label);
                rest = url.split_once(')').map_or("", |(_, remaining)| remaining);
            } else {
                text.push('[');
                rest = after;
            }
        }
        text.extend(rest.chars().filter(|c| !"*_`#".contains(*c)));
    }
    text
}

pub fn phrase_len(value: &str, complete: bool) -> Option<usize> {
    let mut letters = value.char_indices().peekable();
    let mut word_break = None;
    let mut count = 0;
    while let Some((offset, letter)) = letters.next() {
        let end = offset + letter.len_utf8();
        let sentence = match letter {
            '.' => letters
                .peek()
                .map_or(complete, |(_, next)| next.is_whitespace()),
            '。' | '！' | '？' | '!' | '?' | '؟' | '\n' => true,
            _ => false,
        };
        let clause = count >= 8 && matches!(letter, '，' | ',' | '،' | '；' | ';');
        if sentence || clause || letter.is_whitespace() {
            word_break = Some(end);
        }
        if !complete && (sentence || clause) {
            return Some(end);
        }
        count += 1;
        if count == 140 {
            return Some(
                word_break
                    .filter(|&split| split >= offset / 2)
                    .unwrap_or(end),
            );
        }
    }
    complete.then_some(value.len())
}

#[cfg(target_os="android")]
pub(crate) use crate::android_os::speak;

#[cfg(any(target_os="macos",target_os="linux"))]
pub async fn speak(
    text: String,
    language: String,
    mut cancel: oneshot::Receiver<()>,
) -> Result<(), String> {
    use std::process::Stdio;
    use tokio::{io::AsyncWriteExt, process::Command};
    #[cfg(target_os = "macos")]
    let mut command = {
        use objc2_avf_audio::AVSpeechSynthesisVoice;
        use objc2_foundation::NSString;
        let mut command = Command::new("/usr/bin/say");
        if let Some(voice) = unsafe {
            AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(&language)))
        } {
            command.arg("-v").arg(unsafe { voice.name() }.to_string());
        }
        command.args(["-f", "-"]);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let tools = crate::deployment::Setup::resource_dir()?.join("tools");
        let mut command = Command::new(tools.join("bin/espeak-ng"));
        command.env("LD_LIBRARY_PATH", tools.join("lib"));
        command.arg("--path").arg(tools.join("share"));
        let voice = match language.as_str() {
            "zh-CN" | "zh-TW" => "cmn",
            other => other,
        };
        command.args(["-v", voice, "--stdin"]);
        command
    };
    let mut speech = command
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = speech.stdin.take().expect("piped speech input");
    input
        .write_all(text.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    drop(input);
    tokio::select! {
        result = speech.wait() => {
            if result.map_err(|e| e.to_string())?.success() { Ok(()) } else { Err("本机播报失败".into()) }
        }
        _ = &mut cancel => speech.kill().await.map_err(|e| e.to_string()),
    }
}

#[cfg(windows)]
pub async fn speak(
    text: String,
    language: String,
    mut cancel: oneshot::Receiver<()>,
) -> Result<(), String> {
    use tokio::sync::oneshot::error::TryRecvError;
    use windows::{
        Win32::{
            Foundation::VARIANT_TRUE,
            Media::Speech::{
                ISpeechVoice, SVSFIsNotXML, SVSFPurgeBeforeSpeak, SVSFlagsAsync, SpVoice,
                SpeechVoiceSpeakFlags,
            },
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
        },
        core::BSTR,
    };
    tokio::task::spawn_blocking(move || {
        // SAPI is apartment-local, so its full lifetime stays on this thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
            .map_err(|error| error.to_string())?;
        let result = (|| -> Result<(), String> {
            let engine: ISpeechVoice =
                unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|error| error.to_string())?;
            let lang = match language.as_str() {
                "zh-CN" => "804",
                "zh-TW" => "404",
                "en" => "409",
                "ja" => "411",
                "ko" => "412",
                "de" => "407",
                "fr" => "40c",
                "es" => "c0a",
                "ru" => "419",
                "ar" => "401",
                _ => "409",
            };
            let voices =
                unsafe { engine.GetVoices(&BSTR::from(format!("Language={lang}")), &BSTR::new()) }
                    .map_err(|error| error.to_string())?;
            if unsafe { voices.Count() }.map_err(|error| error.to_string())? > 0 {
                let voice = unsafe { voices.Item(0) }.map_err(|error| error.to_string())?;
                unsafe { engine.putref_Voice(&voice) }.map_err(|error| error.to_string())?;
            }
            unsafe {
                engine.Speak(
                    &BSTR::from(text),
                    SpeechVoiceSpeakFlags(SVSFlagsAsync.0 | SVSFIsNotXML.0),
                )
            }
            .map_err(|error| error.to_string())?;
            while unsafe { engine.WaitUntilDone(20) }.map_err(|error| error.to_string())?
                != VARIANT_TRUE
            {
                if cancel.try_recv() != Err(TryRecvError::Empty) {
                    unsafe {
                        engine.Speak(
                            &BSTR::new(),
                            SpeechVoiceSpeakFlags(SVSFlagsAsync.0 | SVSFPurgeBeforeSpeak.0),
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    break;
                }
            }
            Ok(())
        })();
        unsafe { CoUninitialize() };
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
