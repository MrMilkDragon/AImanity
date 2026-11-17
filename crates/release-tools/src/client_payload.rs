use crate::{
    Result, assets, phone_apps,
    source::{self, Entry},
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

fn read_json(path: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_reader(File::open(path)?)?)
}

fn container_host(root: &Path) -> Result<Vec<Entry>> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("Container host bundle requires Apple Silicon".into());
    }
    let host = "macos-arm64";
    let policy = read_json(root.join("config/container-host-runtime.json"))?;
    let bootstrap = read_json(root.join("config/host-bootstrap.json"))?;
    let client = read_json(root.join("config/native-client.json"))?;
    let mut selected = json!({
        "host":host,
        "runtime":policy["hosts"][host],
        "lima":bootstrap["lima_hosts"][host],
        "minimum_macos":client["minimum_macos"],
    });
    selected.sort_all_objects();
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&selected)?));
    let folder = root.join(".tools/container-host");
    let manifest = read_json(folder.join("manifest.json"))?;
    if policy["format"] != 1
        || manifest["format"] != 1
        || manifest["host"] != host
        || manifest["policy_sha256"] != digest
    {
        return Err("Container host policy changed".into());
    }
    source::component(&folder, "runtime/container-host", &manifest)
}

fn copy_payload(input: Option<PathBuf>, output: &Path) -> Result<Option<String>> {
    let Some(input) = input else {
        return match fs::remove_file(output) {
            Ok(()) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        };
    };
    if std::path::absolute(&input)? != output {
        let stage =
            tempfile::NamedTempFile::new_in(output.parent().ok_or("Missing host directory")?)?;
        fs::copy(input, stage.path())?;
        stage.persist(output)?;
    }
    let mut sha256 = Sha256::new();
    io::copy(&mut File::open(output)?, &mut sha256)?;
    Ok(Some(format!("{:x}", sha256.finalize())))
}

pub(super) fn run(args: &[OsString]) -> Result<Value> {
    let root = std::env::current_dir()?;
    let mut arguments = args.iter();
    let (mut phone, mut service, mut container, mut qemu) = (false, false, false, None);
    let (mut ocr, mut android, mut images) = (None, None, None);
    while let Some(flag) = arguments.next() {
        match flag.to_str() {
            Some("--with-phone-apps") => phone = true,
            Some("--with-service-runtime") => service = true,
            Some("--with-container-host") => container = true,
            Some("--qemu-manifest") => {
                qemu = Some(
                    arguments
                        .next()
                        .and_then(|value| value.to_str())
                        .ok_or("Missing QEMU identity")?,
                );
            }
            Some("--linux-ocr" | "--android-runtime" | "--images-runtime") => {
                let path = PathBuf::from(arguments.next().ok_or("Missing payload path")?);
                match flag.to_str() {
                    Some("--linux-ocr") => ocr = Some(path),
                    Some("--android-runtime") => android = Some(path),
                    _ => images = Some(path),
                }
            }
            _ => {
                return Err(
                    format!("Unknown client payload option: {}", flag.to_string_lossy()).into(),
                );
            }
        }
    }
    let mut runtime = Vec::new();
    let mut components = Vec::new();
    if service {
        #[cfg(unix)]
        {
            runtime.extend(crate::native_services::package(ocr.as_deref())?);
            components.push("native-services");
        }
        #[cfg(not(unix))]
        return Err("The Windows backend already carries native services".into());
    }
    if phone {
        let manifest = phone_apps::run(&["verify".into(), root.as_os_str().into()])?;
        runtime.extend(source::component(
            &root.join(".tools/phone-apps"),
            "runtime/phone-apps",
            &manifest,
        )?);
        components.push("phone-apps");
    }
    if container {
        runtime.extend(container_host(&root)?);
        components.push("container-host");
    }

    let folder = root.join("apps/native/gen/host");
    fs::create_dir_all(&folder)?;
    let speech = assets::speech(&[root.join(".tools/speech").into(), folder.as_os_str().into()])?;
    let mut integrity = Map::new();
    integrity.insert(
        "source.payload".into(),
        json!(source::payload(&root, &folder, runtime, components)?),
    );
    let speech_file = speech["resource"]
        .as_str()
        .ok_or("Missing speech resource name")?;
    integrity.insert(speech_file.into(), speech["sha256"].clone());
    for (name, source) in [("android.payload", android), ("images.payload", images)] {
        if let Some(digest) = copy_payload(source, &folder.join(name))? {
            integrity.insert(name.into(), json!(digest));
        }
    }
    let directory = folder.join("qemu");
    if let Some(digest) = qemu {
        integrity.insert("qemu/manifest.json".into(), json!(digest));
    } else if directory.try_exists()? {
        fs::remove_dir_all(directory)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&folder)?;
    serde_json::to_writer_pretty(&mut temporary, &integrity)?;
    temporary.persist(folder.join("integrity.json"))?;
    Ok(Value::Object(integrity))
}
