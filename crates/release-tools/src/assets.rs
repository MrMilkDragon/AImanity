use crate::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Deserialize)]
struct OcrModels {
    source_commit: String,
    models: Vec<Pin>,
}

#[derive(Deserialize)]
pub(super) struct Pin {
    file: String,
    url: String,
    size_bytes: u64,
    sha256: String,
}

impl Pin {
    pub(super) fn stage(&self, directory: &Path) -> Result<std::path::PathBuf> {
        let file = directory.join(&self.file);
        if file.is_symlink() {
            return Err("Pinned asset cache cannot be a symbolic link".into());
        }
        stage_pin::<Sha256>(&self.url, &file, self.size_bytes, &self.sha256)?;
        Ok(file)
    }

    #[cfg(windows)]
    pub(super) fn verify(&self, file: &Path) -> Result<()> {
        if !file.symlink_metadata()?.is_file() {
            return Err("Pinned asset must be a regular file".into());
        }
        stage_pin::<Sha256>(&self.url, file, self.size_bytes, &self.sha256)
    }
}

const CURL_FETCH: [&str; 5] = ["-fsSL", "--proto", "=https", "--proto-redir", "=https"];

pub(super) fn verify_stream<D: Digest>(
    mut stream: impl Read,
    mut output: impl Write,
    size: impl Into<Option<u64>>,
    digest: &str,
) -> io::Result<bool>
where
    sha2::digest::Output<D>: std::fmt::LowerHex,
{
    let size = size.into();
    let mut hash = D::new();
    let mut buffer = [0; 128 * 1024];
    let mut received = 0;
    loop {
        let length = stream.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        received += length as u64;
        if size.is_some_and(|size| received > size) {
            return Ok(false);
        }
        let chunk = &buffer[..length];
        hash.update(chunk);
        output.write_all(chunk)?;
    }
    Ok(size.is_none_or(|size| received == size) && format!("{:x}", hash.finalize()) == digest)
}

pub(super) fn stage_pin<D: Digest>(
    url: &str,
    path: &Path,
    size: impl Into<Option<u64>>,
    digest: &str,
) -> Result<()>
where
    sha2::digest::Output<D>: std::fmt::LowerHex,
{
    let size = size.into();
    let mismatch = || format!("Pinned download mismatch: {}", path.display());
    if path.exists() {
        let file = File::open(path)?;
        if !verify_stream::<D>(file, io::sink(), size, digest)? {
            return Err(mismatch().into());
        }
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    let mut download = Command::new("curl")
        .args(CURL_FETCH)
        .arg(url)
        .stdout(Stdio::piped())
        .spawn()?;
    let checked = verify_stream::<D>(
        download.stdout.take().expect("curl stdout"),
        pending.as_file_mut(),
        size,
        digest,
    );
    if !matches!(checked, Ok(true)) {
        let _ = download.kill();
    }
    let status = download.wait()?;
    if !checked? || !status.success() {
        return Err(mismatch().into());
    }
    pending.persist_noclobber(path)?;
    Ok(())
}

pub(super) fn ocr(arguments: &[OsString]) -> Result<Value> {
    let [flag, destination] = arguments else {
        return Err("prepare-ocr-models requires --destination DIRECTORY".into());
    };
    if flag != "--destination" {
        return Err("prepare-ocr-models requires --destination DIRECTORY".into());
    }
    let destination = Path::new(destination);
    let policy: OcrModels = serde_json::from_str(include_str!("../../../config/ocr-models.json"))?;
    for model in &policy.models {
        model.stage(destination)?;
    }
    Ok(json!({"models":policy.models.len(),"source_commit":policy.source_commit}))
}

pub(super) fn speech(arguments: &[OsString]) -> Result<Value> {
    let [cache, host] = arguments else {
        return Err("prepare-speech-model requires CACHE_DIRECTORY HOST_DIRECTORY".into());
    };
    let model: Pin = serde_json::from_str(include_str!("../../../config/speech-model.json"))?;
    let source = model.stage(Path::new(cache))?;
    let directory = Path::new(host).join("speech");
    fs::create_dir_all(&directory)?;
    fs::copy(source, directory.join(&model.file))?;
    Ok(json!({"resource":format!("speech/{}", model.file),"sha256":model.sha256}))
}

pub(super) fn aosp(arguments: &[OsString]) -> Result<()> {
    let [arch, destination] = arguments else {
        return Err("fetch-aosp requires arm64 or x86_64 and OUTPUT_ZIP".into());
    };
    let (arch, build, size, digest) = match arch.to_str() {
        Some("arm64") => (
            "arm64",
            "16644040",
            1_101_174_958,
            "21542c7cbc096a76d91b723c17ea391f",
        ),
        Some("x86_64") => (
            "x86_64",
            "13455678",
            1_001_184_764,
            "453044532a2070651c2576dc0343b1b6",
        ),
        _ => return Err("Unsupported AOSP guest architecture".into()),
    };
    let image = format!("aosp_cf_{arch}_only_phone");
    let viewer = Command::new("curl")
        .args(CURL_FETCH)
        .arg(format!("https://ci.android.com/builds/submitted/{build}/{image}-userdebug/latest/view/{image}-img-{build}.zip"))
        .output()?;
    if !viewer.status.success() {
        return Err("Android CI viewer download failed".into());
    }
    let viewer = std::str::from_utf8(&viewer.stdout)?;
    let pattern = regex::Regex::new(r"var JSVariables = (\{.*?\});")?;
    let metadata = pattern
        .captures(viewer)
        .ok_or("Android CI viewer metadata missing")?;
    let metadata: Value = serde_json::from_str(&metadata[1])?;
    let url = metadata["artifactUrl"]
        .as_str()
        .filter(|url| url.starts_with("https://storage.googleapis.com/"))
        .ok_or("Android CI did not return its image storage URL")?;
    stage_pin::<md5::Md5>(url, Path::new(destination), size, digest)
}

pub(super) fn windows_audio(arguments: &[OsString]) -> Result<()> {
    let [directory] = arguments else {
        return Err("stage-windows-android-audio requires SDK_CUTTLEFISH_DIRECTORY".into());
    };
    let directory = Path::new(directory);
    stage_pin::<Sha256>(
        "https://github.com/Genymobile/scrcpy/releases/download/v3.3.4/scrcpy-server-v3.3.4",
        &directory.join("aimanity-audio.jar"),
        90980,
        "8588238c9a5a00aa542906b6ec7e6d5541d9ffb9b5d0f6e1bc0e365e2303079e",
    )?;
    let licenses = directory.join("licenses");
    fs::create_dir_all(&licenses)?;
    fs::copy("LICENSE", licenses.join("scrcpy-Apache-2.0.txt"))?;
    Ok(())
}

#[cfg(windows)]
pub(super) fn windows_unpacker(root: &Path) -> Result<std::path::PathBuf> {
    let policy: Value =
        serde_json::from_reader(File::open(root.join("config/windows-installer.json"))?)?;
    let pin = &policy["unpacker"];
    let version = pin["version"]
        .as_str()
        .ok_or("Missing Inno unpacker version")?;
    let folder = root.join(".tools/windows-installer");
    let program = folder.join(format!("innounp-{version}.exe"));
    if !program.try_exists()? {
        let archive = folder.join(format!("innounp-{version}.zip"));
        stage_pin::<Sha256>(
            pin["url"].as_str().ok_or("Missing Inno unpacker URL")?,
            &archive,
            pin["size_bytes"]
                .as_u64()
                .ok_or("Missing Inno unpacker size")?,
            pin["sha256"]
                .as_str()
                .ok_or("Missing Inno unpacker digest")?,
        )?;
        let output = Command::new("tar")
            .arg("-xOf")
            .arg(archive)
            .arg("innounp.exe")
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr)
                .trim()
                .to_owned()
                .into());
        }
        let mut bytes = output.stdout;
        let offset = u32::from_le_bytes(
            bytes
                .get(60..64)
                .ok_or("Missing Inno PE header")?
                .try_into()?,
        ) as usize;
        let flags = bytes
            .get_mut(offset..)
            .filter(|header| header.starts_with(b"PE\0\0"))
            .and_then(|header| header.get_mut(22..24))
            .ok_or("Invalid Inno PE header")?;
        let large_address = u16::from_le_bytes((&*flags).try_into()?) | 0x20;
        flags.copy_from_slice(&large_address.to_le_bytes());
        let mut staged = tempfile::NamedTempFile::new_in(folder)?;
        staged.write_all(&bytes)?;
        staged.persist_noclobber(&program)?;
    }
    Ok(program)
}

pub(super) fn factory_android() -> Result<()> {
    let repository = std::env::var("GITHUB_REPOSITORY")?;
    let github_env = std::env::var_os("GITHUB_ENV").ok_or("Missing GITHUB_ENV")?;
    let folder = PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("Missing RUNNER_TEMP")?);
    let release_id = std::env::var("AIMANITY_ANDROID_DRAFT_ID")?.parse::<u64>()?;
    let digest = std::env::var("AIMANITY_ANDROID_PAYLOAD_SHA256")?;
    if release_id == 0 || std::env::var("GH_TOKEN")?.is_empty() {
        return Err("Missing private Android factory credentials or release ID".into());
    }
    let repository = format!("repos/{repository}");
    let api = |endpoint: &str| -> Result<Value> {
        let answer = Command::new("gh").args(["api", endpoint]).output()?;
        if !answer.status.success() {
            return Err(format!("GitHub rejected {endpoint}").into());
        }
        Ok(serde_json::from_slice(&answer.stdout)?)
    };
    if api(&repository)?["private"] != true {
        return Err("Use a private repository for the installer factory".into());
    }
    let release = api(&format!("{repository}/releases/{release_id}"))?;
    if release["draft"] != true || !release.get("published_at").is_some_and(Value::is_null) {
        return Err("Use an unpublished Android payload".into());
    }
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "android-macos-arm64.payload",
        ("windows", "x86_64") => "android-windows-x64.payload",
        _ => return Err("Unsupported Android factory platform".into()),
    };
    let expected = format!("sha256:{digest}");
    let asset = release["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset["name"] == name && asset["state"] == "uploaded" && asset["digest"] == expected
            })
        })
        .and_then(|asset| asset["id"].as_u64())
        .ok_or("Missing pinned Android draft asset")?;
    let file = tempfile::NamedTempFile::new_in(&folder)?;
    let status = Command::new("gh")
        .args([
            "api",
            &format!("{repository}/releases/assets/{asset}"),
            "-H",
            "Accept: application/octet-stream",
        ])
        .stdout(file.reopen()?)
        .status()?;
    if !status.success() {
        return Err(format!("Could not download {name}").into());
    }
    let destination = folder.join(name);
    file.persist_noclobber(&destination)?;
    writeln!(
        File::options().append(true).open(github_env)?,
        "AIMANITY_ANDROID_PAYLOAD={}",
        destination.display()
    )?;
    Ok(())
}
