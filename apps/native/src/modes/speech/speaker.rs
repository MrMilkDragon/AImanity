use std::sync::{Arc, atomic::AtomicBool};

pub fn readable(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    for content in value.split("```").step_by(2) {
        let mut rest = content;
        while let Some((before, after)) = rest.split_once('[') {
            text.extend(before.chars().filter(|c| !"*_`#".contains(*c)));
            if let Some((label, url)) = after.split_once("](")
                && let Some((_, remaining)) = url.split_once(')')
            {
                text.push_str(label);
                rest = remaining;
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
    for (index, (offset, letter)) in value.char_indices().enumerate() {
        if index == 180 {
            return Some(offset);
        }
        if "。！？.!?\n".contains(letter) {
            return Some(offset + letter.len_utf8());
        }
    }
    complete.then_some(value.len())
}

#[cfg(target_os = "macos")]
pub async fn speak(text: String, language: String, cancel: Arc<AtomicBool>) -> Result<(), String> {
    use std::{process::Stdio, sync::atomic::Ordering, time::Duration};
    use tokio::{io::AsyncWriteExt, process::Command};
    async fn play(text: String, language: String, cancel: Arc<AtomicBool>) -> std::io::Result<()> {
        let voices = Command::new("/usr/bin/say")
            .args(["-v", "?"])
            .output()
            .await?;
        let list = String::from_utf8_lossy(&voices.stdout);
        let language = language.replace('-', "_");
        let primary = language.split('_').next().expect("speech language");
        let mut matches = list.lines().filter_map(|line| {
            let (name_locale, _) = line.split_once('#')?;
            let (name, locale) = name_locale.trim_end().rsplit_once(' ')?;
            (locale.split('_').next() == Some(primary)).then_some((name.trim(), locale))
        });
        let voice = matches
            .clone()
            .find(|(_, locale)| *locale == language)
            .or_else(|| matches.next());
        let Some((voice, _)) = voice else {
            return Err(std::io::Error::other("当前语言没有本机播报音色"));
        };
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut child = Command::new("/usr/bin/say")
            .args(["-v", voice, "-f", "-"])
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).await?;
        }
        loop {
            if cancel.load(Ordering::Acquire) {
                child.kill().await?;
                return Ok(());
            }
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(std::io::Error::other("本机播报失败"))
                };
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    play(text, language, cancel)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
pub async fn speak(text: String, language: String, cancel: Arc<AtomicBool>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
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
            if unsafe { voices.Count() }.map_err(|error| error.to_string())? == 0 {
                return Err("当前语言没有本机播报音色".into());
            }
            let voice = unsafe { voices.Item(0) }.map_err(|error| error.to_string())?;
            unsafe { engine.putref_Voice(&voice) }.map_err(|error| error.to_string())?;
            unsafe {
                engine.Speak(
                    &BSTR::from(text),
                    SpeechVoiceSpeakFlags(SVSFlagsAsync.0 | SVSFIsNotXML.0),
                )
            }
            .map_err(|error| error.to_string())?;
            while unsafe { engine.WaitUntilDone(40) }.map_err(|error| error.to_string())?
                != VARIANT_TRUE
            {
                if cancel.load(Ordering::Acquire) {
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

#[cfg(not(any(target_os = "macos", windows)))]
pub async fn speak(
    _text: String,
    _language: String,
    _cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    Err("当前系统没有本机语音播报服务".into())
}
