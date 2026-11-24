mod android_component;
mod android_native;
mod android_package;
mod assets;
mod capsule;
mod client_installer;
mod client_payload;
mod cuttlefish;
mod dependency_audit;
#[cfg(unix)]
mod image_factory;
#[cfg(unix)]
mod linux_ocr;
#[cfg(unix)]
mod linux_rootfs;
#[cfg(target_os = "macos")]
mod macos_factory;
#[cfg(unix)]
mod native_services;
mod phone_apps;
mod source;
#[cfg(windows)]
mod windows_factory;
#[cfg(unix)]
mod windows_release;

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
        #[cfg(unix)]
        "android-linux-context" => native_services::android_guest(rest).map(Some),
        #[cfg(unix)]
        "windows-release-context" => windows_release::prepare(rest).map(Some),
        #[cfg(unix)]
        "windows-guest-assets" => windows_release::assets::run(rest).map(Some),
        "source" => source::run(rest),
        #[cfg(any(target_os = "macos", windows))]
        "client-installer" => client_installer::run(rest).map(Some),
        #[cfg(target_os = "linux")]
        "linux-payload" => client_installer::linux_payload::prepare(rest).map(Some),
        #[cfg(target_os = "linux")]
        "linux-packages" => client_installer::linux_packages::build(rest).map(Some),
        "client-payload" => client_payload::run(rest).map(Some),
        #[cfg(target_os = "macos")]
        "macos-factory" => macos_factory::run(rest).map(Some),
        "prepare-ocr-models" => assets::ocr(rest).map(Some),
        "prepare-speech-model" => assets::speech(rest).map(Some),
        #[cfg(unix)]
        "linux-ocr" => linux_ocr::run(rest).map(Some),
        #[cfg(unix)]
        "native-services" => native_services::run(rest).map(Some),
        "factory-android" => assets::factory_android().map(|()| None),
        "fetch-aosp" => assets::aosp(rest).map(|()| None),
        "windows-cuttlefish" => cuttlefish::run(rest).map(|()| None),
        "android-avf-boot" => cuttlefish::android_avf_boot(rest).map(|()| None),
        "android-component" => android_component::seal(rest).map(Some),
        "stage-windows-android-audio" => assets::windows_audio(rest).map(|()| None),
        #[cfg(windows)]
        "windows-factory" => windows_factory::run(rest),
        "audit-dependencies" => dependency_audit::run(rest).map(|()| None),
        #[cfg(unix)]
        "factory-images" => image_factory::run(rest).map(Some),
        "build-android-apk" => android_package::build(rest).map(Some),
        "mobile-activity-dex" => android_package::mobile_dex(rest).map(|()| None),
        "mobile-guest-apps" => phone_apps::mobile(rest).map(Some),
        "build-android-network" => android_package::network_dex::build(rest).map(|()| None),
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
