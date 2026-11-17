use crate::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
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
        stage_pin::<Sha256>(&self.url, &file, self.size_bytes, &self.sha256)?;
        Ok(file)
    }
}

const CURL_FETCH: [&str; 5] = ["-fsSL", "--proto", "=https", "--proto-redir", "=https"];

fn verify_stream<D: Digest>(
    mut stream: impl Read,
    mut output: impl Write,
    size: u64,
    digest: &str,
) -> io::Result<bool>
where
    sha2::digest::Output<D>: std::fmt::LowerHex,
{
    let mut hash = D::new();
    let mut buffer = [0; 128 * 1024];
    let mut received = 0;
    loop {
        let length = stream.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        received += length as u64;
        if received > size {
            return Ok(false);
        }
        let chunk = &buffer[..length];
        hash.update(chunk);
        output.write_all(chunk)?;
    }
    Ok(received == size && format!("{:x}", hash.finalize()) == digest)
}

pub(super) fn stage_pin<D: Digest>(url: &str, path: &Path, size: u64, digest: &str) -> Result<()>
where
    sha2::digest::Output<D>: std::fmt::LowerHex,
{
    let mismatch = || format!("Pinned download mismatch: {}", path.display());
    if path.exists() {
        let file = File::open(path)?;
        if file.metadata()?.len() != size || !verify_stream::<D>(file, io::sink(), size, digest)? {
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

pub(super) fn windows_aosp(arguments: &[OsString]) -> Result<()> {
    let [destination] = arguments else {
        return Err("fetch-windows-aosp requires OUTPUT_ZIP".into());
    };
    let viewer = Command::new("curl")
        .args(CURL_FETCH)
        .arg("https://ci.android.com/builds/submitted/13455678/aosp_cf_x86_64_only_phone-userdebug/latest/view/aosp_cf_x86_64_only_phone-img-13455678.zip")
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
    stage_pin::<md5::Md5>(
        url,
        Path::new(destination),
        1_001_184_764,
        "453044532a2070651c2576dc0343b1b6",
    )
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

pub(super) fn windows_unpacker(arguments: &[OsString]) -> Result<Value> {
    let [root] = arguments else {
        return Err("windows-unpacker requires the project root".into());
    };
    let root = std::path::absolute(root)?;
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
    Ok(serde_json::to_value(program)?)
}

#[derive(Deserialize)]
struct UploadPlan {
    source_commit: String,
    files: Vec<PlannedAsset>,
}

#[derive(Deserialize)]
struct PlannedAsset {
    name: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct Release {
    draft: bool,
    published_at: Value,
    target_commitish: String,
    assets: Vec<UploadedAsset>,
}

#[derive(Deserialize)]
struct UploadedAsset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}

pub(super) fn verify_upload(arguments: &[OsString]) -> Result<Value> {
    let [plan, release] = arguments else {
        return Err("verify-upload requires PLAN_JSON RELEASE_JSON".into());
    };
    let plan: UploadPlan = serde_json::from_reader(File::open(plan)?)?;
    let release: Release = serde_json::from_reader(File::open(release)?)?;
    if !release.draft
        || !release.published_at.is_null()
        || release.target_commitish != plan.source_commit
    {
        return Err("Candidate is not an unpublished source-matched draft".into());
    }
    let mut assets: BTreeMap<_, _> = release
        .assets
        .iter()
        .map(|asset| (&asset.name, asset))
        .collect();
    if assets.len() != release.assets.len() || assets.len() != plan.files.len() {
        return Err("Candidate remote inventory differs".into());
    }
    for file in plan.files {
        let expected = format!("sha256:{}", file.sha256);
        if !assets.remove(&file.name).is_some_and(|asset| {
            asset.state == "uploaded"
                && asset.size == file.size
                && asset.digest.as_deref() == Some(expected.as_str())
        }) {
            return Err("Candidate remote asset differs".into());
        }
    }
    Ok(json!({"verified":true}))
}
