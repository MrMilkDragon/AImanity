mod android_package;
mod assets;
mod capsule;
mod dependency_audit;
#[cfg(unix)]
mod linux_ocr;
mod phone_apps;
mod source;
mod update_feed;
#[cfg(unix)]
mod windows_release;
#[cfg(unix)]
mod wsl_qualification;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn execute(arguments: Vec<std::ffi::OsString>) -> Result<Option<serde_json::Value>> {
    use std::{
        io::{self, Read, Write},
        path::Path,
    };
    let unknown = "Unknown release tool command";
    let (command, rest) = arguments.split_first().ok_or(unknown)?;
    match command.to_str().ok_or(unknown)? {
        #[cfg(unix)]
        "windows-release" => windows_release::seal(rest).map(Some),
        "source" => source::run(rest),
        "sign-update" => update_feed::sign(rest).map(|()| None),
        "prepare-ocr-models" => assets::ocr(rest).map(Some),
        "prepare-speech-model" => assets::speech(rest).map(Some),
        #[cfg(unix)]
        "linux-ocr" => linux_ocr::run(rest).map(Some),
        "verify-upload" => assets::verify_upload(rest).map(Some),
        "factory-android" => assets::factory_android().map(|()| None),
        "fetch-windows-aosp" => assets::windows_aosp(rest).map(|()| None),
        "stage-windows-android-audio" => assets::windows_audio(rest).map(|()| None),
        "windows-unpacker" => assets::windows_unpacker(rest).map(Some),
        "audit-dependencies" => dependency_audit::run(rest).map(|()| None),
        "build-android-apk" => android_package::build(rest).map(Some),
        "phone-apps" => phone_apps::run(rest).map(Some),
        "decode-capsule" => {
            let [codec] = rest else {
                return Err("decode-capsule requires gzip or xz".into());
            };
            let input = io::stdin().lock();
            let mut decoded: Box<dyn Read> = match codec.to_str() {
                Some("gzip") => Box::new(flate2::read::MultiGzDecoder::new(input)),
                Some("xz") => Box::new(xz2::read::XzDecoder::new(input)),
                _ => return Err("Unsupported capsule codec".into()),
            };
            io::copy(&mut decoded, &mut io::stdout().lock())?;
            Ok(None)
        }
        "encode-gzip" => {
            if !rest.is_empty() {
                return Err("encode-gzip takes its input through stdin".into());
            }
            let mut output =
                flate2::write::GzEncoder::new(io::stdout().lock(), flate2::Compression::best());
            io::copy(&mut io::stdin().lock(), &mut output)?;
            output.finish()?.flush()?;
            Ok(None)
        }
        "windows-sdk-files" => {
            if !rest.is_empty() {
                return Err("windows-sdk-files takes the file list through stdin".into());
            }
            let files = serde_json::from_reader(io::stdin().lock())?;
            Ok(Some(serde_json::to_value(android_package::select(files))?))
        }
        "encode-xz" => {
            let [arch, preset, directory] = rest else {
                return Err("encode-xz requires architecture, preset and output directory".into());
            };
            capsule::encode(
                &mut io::stdin().lock(),
                io::stdout().lock(),
                arch.to_str().ok_or("Invalid image architecture")?,
                preset.to_str().ok_or("Invalid XZ preset")?.parse()?,
                Path::new(directory),
            )?;
            Ok(None)
        }
        #[cfg(unix)]
        "wsl-qualify" => {
            if !rest.is_empty() {
                return Err("qualification_arguments".into());
            }
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(wsl_qualification::run())
                .map(Some)
        }
        _ => Err(unknown.into()),
    }
}

fn main() {
    match execute(std::env::args_os().skip(1).collect()) {
        Ok(Some(value)) => println!("{value}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
