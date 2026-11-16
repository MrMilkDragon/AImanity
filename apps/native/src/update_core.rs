use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Client, Response};
use ring::digest::{Context, SHA512, SHA512_OUTPUT_LEN};
use ring::signature::{ED25519, UnparsedPublicKey};
use semver::Version;
use serde::Deserialize;
use std::{
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncWriteExt, BufWriter};

#[cfg(target_os = "macos")]
#[path = "update_install/macos.rs"]
pub(crate) mod macos;
#[cfg(windows)]
#[path = "update_install/windows.rs"]
mod windows;

const REPOSITORY: &str = "MrMilkDragon/AImanity";
const UPDATE_FEED: &str =
    "https://api.github.com/repos/MrMilkDragon/AImanity/contents/stable.json?ref=update-feed";
const MAX_MANIFEST: usize = 16 * 1024;
const MAX_INSTALLER: u64 = 2 * 1024 * 1024 * 1024;
pub(crate) type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy)]
pub enum Platform {
    Mac,
    Windows,
}

impl Platform {
    pub(crate) fn installer(self, version: &Version) -> String {
        let suffix = match self {
            Self::Mac => "macos-arm64.dmg",
            Self::Windows => "windows-x64-setup.exe",
        };
        format!("AImanity-{version}-{suffix}")
    }

    fn artifact(self, manifest: Manifest) -> Artifact {
        match self {
            Self::Mac => manifest.macos_arm64,
            Self::Windows => manifest.windows_x64,
        }
    }
}

pub fn platform() -> Option<Platform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(Platform::Mac),
        ("windows", "x86_64") => Some(Platform::Windows),
        _ => None,
    }
}

pub(crate) fn remove_current(cache: &Path, version: &Version, platform: Platform) {
    let _ = std::fs::remove_file(
        cache
            .join("client-updates")
            .join(platform.installer(version)),
    );
}

#[derive(Deserialize)]
struct SignedManifest {
    manifest: String,
    signature: String,
}

#[derive(Deserialize)]
struct Manifest {
    format: u8,
    version: String,
    name: String,
    notes: String,
    macos_arm64: Artifact,
    windows_x64: Artifact,
}

#[derive(Deserialize, Clone, Debug)]
struct Artifact {
    size: u64,
    sha512: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Offer {
    pub(crate) version: Version,
    pub(crate) name: String,
    pub(crate) notes: String,
    artifact: Artifact,
}

fn asset_url(version: &Version, name: &str) -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}")
}

fn verify_manifest(
    bytes: &[u8],
    key: &[u8],
    current: &Version,
    platform: Platform,
) -> Result<Option<Offer>> {
    let signed: SignedManifest = serde_json::from_slice(bytes)?;
    let signature = STANDARD.decode(signed.signature)?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(signed.manifest.as_bytes(), &signature)
        .map_err(|_| "Update signature did not verify")?;
    let manifest: Manifest = serde_json::from_str(&signed.manifest)?;
    if manifest.format != 2 {
        return Err("Unsupported update manifest format".into());
    }
    let version = Version::parse(&manifest.version)?;
    if !version.pre.is_empty() || !version.build.is_empty() || &version <= current {
        return Ok(None);
    }
    let name = manifest.name.trim().to_owned();
    let notes = manifest.notes.trim().to_owned();
    if name.is_empty() || notes.is_empty() {
        return Err("The signed release is missing its name or notes".into());
    }
    let artifact = platform.artifact(manifest);
    if !(1..MAX_INSTALLER).contains(&artifact.size)
        || STANDARD.decode(&artifact.sha512)?.len() != SHA512_OUTPUT_LEN
    {
        return Err("Invalid installer metadata".into());
    }
    Ok(Some(Offer {
        version,
        name,
        notes,
        artifact,
    }))
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("AImanity/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(12))
        .read_timeout(Duration::from_secs(60))
        .build()?)
}

pub(crate) async fn check(current: &Version, platform: Platform) -> Result<Option<Offer>> {
    let client = client()?;
    let mut response = client
        .get(UPDATE_FEED)
        .header(reqwest::header::ACCEPT, "application/vnd.github.raw+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    response.error_for_status_ref()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > MAX_MANIFEST {
            return Err("Update manifest exceeded its size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    #[derive(Deserialize)]
    struct Policy {
        public_key: String,
    }
    let policy: Policy = serde_json::from_str(include_str!("../../../config/client-update.json"))?;
    verify_manifest(
        &bytes,
        &STANDARD.decode(policy.public_key)?,
        current,
        platform,
    )
}

pub(crate) async fn download(
    cache: &Path,
    offer: &Offer,
    platform: Platform,
    show_progress: impl FnMut(u64),
) -> Result<PathBuf> {
    let folder = cache.join("client-updates");
    let name = platform.installer(&offer.version);
    let response = client()?
        .get(asset_url(&offer.version, &name))
        .send()
        .await?
        .error_for_status()?;
    stage_installer(response, &folder, &name, &offer.artifact, show_progress).await
}

async fn stage_installer(
    mut response: Response,
    folder: &Path,
    name: &str,
    artifact: &Artifact,
    mut show_progress: impl FnMut(u64),
) -> Result<PathBuf> {
    if response
        .content_length()
        .is_some_and(|size| size != artifact.size)
    {
        return Err("Installer did not match the signed size".into());
    }
    tokio::fs::create_dir_all(folder).await?;
    let (file, temporary) = tempfile::NamedTempFile::new_in(folder)?.into_parts();
    let mut file = BufWriter::with_capacity(1024 * 1024, tokio::fs::File::from_std(file));
    let mut hash = Context::new(&SHA512);
    let mut size = 0;
    let mut shown = 0;
    while let Some(chunk) = response.chunk().await? {
        size += chunk.len() as u64;
        if size > artifact.size {
            return Err("Installer exceeded the signed size".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk).await?;
        let percent = size * 100 / artifact.size;
        if percent != shown {
            shown = percent;
            show_progress(percent);
        }
    }
    if size != artifact.size || STANDARD.encode(hash.finish().as_ref()) != artifact.sha512 {
        return Err("Installer did not match the signed release".into());
    }
    file.flush().await?;
    let file = file.into_inner();
    file.sync_all().await?;
    drop(file);
    let path = folder.join(name);
    temporary.persist(&path)?;
    Ok(path)
}

pub(crate) fn launch(
    cache: &Path,
    downloaded: &Path,
    version: &Version,
    platform: Platform,
    _identifier: &str,
) -> Result<()> {
    let folder = cache.join("client-updates");
    if downloaded != folder.join(platform.installer(version)) {
        return Err("Unexpected installer path.".into());
    }
    #[cfg(target_os = "macos")]
    macos::launch(downloaded, version, _identifier)?;
    #[cfg(windows)]
    windows::launch(&folder, downloaded, version)?;
    Ok(())
}

pub(crate) fn run(args: &[OsString]) -> Option<i32> {
    #[cfg(target_os = "macos")]
    return macos::run(args);
    #[cfg(windows)]
    return windows::run(args);
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = args;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn installer_is_published_only_after_the_signed_size_and_digest_match() {
        let folder = tempfile::tempdir().unwrap();
        let name = "update.dmg";
        let path = folder.path().join(name);
        let body = b"complete installer";
        let artifact = Artifact {
            size: body.len() as u64,
            sha512: STANDARD.encode(ring::digest::digest(&SHA512, body).as_ref()),
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let client = Client::builder().no_proxy().build().unwrap();
                for (body, expected_error) in [
                    (body.as_slice(), None),
                    (
                        b"short".as_slice(),
                        Some("Installer did not match the signed size"),
                    ),
                    (
                        b"untrusted content!".as_slice(),
                        Some("Installer did not match the signed release"),
                    ),
                ] {
                    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                    let address = listener.local_addr().unwrap();
                    let server = thread::spawn(move || {
                        let (mut stream, _) = listener.accept().unwrap();
                        let mut reader = BufReader::new(&mut stream);
                        let mut line = String::new();
                        while reader.read_line(&mut line).unwrap() != 0 && line != "\r\n" {
                            line.clear();
                        }
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .unwrap();
                        let _ = stream.write_all(body);
                    });
                    let response = client
                        .get(format!("http://{address}"))
                        .send()
                        .await
                        .unwrap();
                    let result =
                        stage_installer(response, folder.path(), name, &artifact, |_| {}).await;
                    server.join().unwrap();
                    match expected_error {
                        Some(expected) => assert_eq!(result.unwrap_err().to_string(), expected),
                        None => assert_eq!(result.unwrap(), path),
                    }
                    assert_eq!(std::fs::read(&path).unwrap(), b"complete installer");
                    assert_eq!(folder.path().read_dir().unwrap().count(), 1);
                }
            });
    }

    #[test]
    fn stable_alias_offers_only_a_newer_signed_release_and_its_platform_installer() {
        let current = Version::new(0, 1, 6);
        let version = Version::new(0, 1, 7);
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        let mac = serde_json::json!({"size":17,"sha512":STANDARD.encode([1; SHA512_OUTPUT_LEN])});
        let windows =
            serde_json::json!({"size":29,"sha512":STANDARD.encode([2; SHA512_OUTPUT_LEN])});
        let manifest = serde_json::json!({
            "format": 2,
            "version":version.to_string(),
            "name":"Release name",
            "notes":"Signed release notes",
            "macos_arm64":mac,
            "windows_x64":windows
        })
        .to_string();
        let signature = STANDARD.encode(key.sign(manifest.as_bytes()).as_ref());
        let envelope = |manifest| {
            serde_json::to_vec(&serde_json::json!({"manifest":manifest,"signature":signature}))
                .unwrap()
        };
        let offer = verify_manifest(
            &envelope(manifest.clone()),
            key.public_key().as_ref(),
            &current,
            Platform::Windows,
        )
        .unwrap()
        .unwrap();
        assert_eq!(offer.version, version);
        assert_eq!(offer.artifact.size, 29);
        assert_eq!(offer.name, "Release name");
        assert_eq!(offer.notes, "Signed release notes");
        let mac = verify_manifest(
            &envelope(manifest.clone()),
            key.public_key().as_ref(),
            &current,
            Platform::Mac,
        )
        .unwrap()
        .unwrap();
        assert_eq!(mac.artifact.size, 17);
        assert!(
            verify_manifest(
                &envelope(manifest.replace("Signed release notes", "Untrusted notes")),
                key.public_key().as_ref(),
                &current,
                Platform::Windows
            )
            .is_err()
        );
        assert!(
            verify_manifest(
                &envelope(manifest),
                key.public_key().as_ref(),
                &version,
                Platform::Windows
            )
            .unwrap()
            .is_none()
        );
    }
}
