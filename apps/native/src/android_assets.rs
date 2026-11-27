//! The signed APK owns the read-only guest templates. Writable disks live under the installation.
use android_activity::AndroidApp;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

static ACTIVITY: Mutex<Option<AndroidApp>> = Mutex::new(None);

pub(super) fn initialize(app: AndroidApp) -> Result<(), String> {
    let path = app
        .internal_data_path()
        .ok_or("Android internal storage unavailable")?;
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    *ACTIVITY.lock().unwrap() = Some(app);
    Ok(())
}

pub(crate) fn detach(app: &AndroidApp) -> bool {
    ACTIVITY
        .lock()
        .unwrap()
        .take_if(|current| current == app)
        .is_some()
}
pub(crate) fn activity() -> io::Result<AndroidApp> {
    ACTIVITY
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| io::Error::other("Android Activity unavailable"))
}

pub(super) fn home() -> Result<PathBuf, String> {
    activity()
        .map_err(|e| e.to_string())?
        .internal_data_path()
        .map(|p| p.join("host"))
        .ok_or_else(|| "Android internal storage unavailable".into())
}

pub(crate) fn helper() -> io::Result<PathBuf> {
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
    // The function is part of the Activity DSO installed alongside the executable helper.
    if unsafe { libc::dladdr(helper as *const () as _, info.as_mut_ptr()) } == 0 {
        return Err(io::Error::other(
            "Android native library directory unavailable",
        ));
    }
    let name = unsafe { info.assume_init().dli_fname };
    if name.is_null() {
        return Err(io::Error::other("Missing Android native library path"));
    }
    let path = Path::new(std::ffi::OsStr::from_bytes(
        unsafe { CStr::from_ptr(name) }.to_bytes(),
    ));
    let helper = path.with_file_name("libaimanity_host.so");
    if !helper.is_absolute() || !helper.is_file() {
        return Err(io::Error::other(
            "Android AVF helper missing from APK native library directory",
        ));
    }
    Ok(helper)
}

#[derive(Deserialize)]
struct Manifest {
    format: u32,
    files: BTreeMap<String, String>,
}

fn asset(app: &AndroidApp, path: &str) -> io::Result<impl Read + 'static> {
    let name = CString::new(format!("mobile-assets/{path}"))?;
    app.asset_manager().open(&name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Offline guest asset missing from APK: {path}"),
        )
    })
}

pub(super) fn manifest() -> io::Result<Option<(String, BTreeMap<String, String>)>> {
    let app = activity()?;
    let source = match asset(&app, "manifest.json") {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        result => result?,
    };
    let mut bytes = Vec::new();
    source.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(io::Error::other("Android asset manifest too large"));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.format != 1 {
        return Err(io::Error::other("Unknown offline asset manifest format"));
    }
    let mut destinations = BTreeSet::new();
    for (name, hash) in &manifest.files {
        let output = name.strip_suffix(".xz").unwrap_or(name);
        let valid = output
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
            && Path::new(output)
                .components()
                .all(|p| matches!(p, Component::Normal(_)))
            && matches!(
                output.split('/').next(),
                Some("android" | "linux" | "speech")
            )
            && hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        if !valid || !destinations.insert(output) {
            return Err(io::Error::other(
                "Invalid or duplicate Android offline asset",
            ));
        }
    }
    Ok(Some((hex::encode(Sha256::digest(&bytes)), manifest.files)))
}

struct HashReader<R> {
    input: R,
    hash: Sha256,
}

impl<R: Read> Read for HashReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let len = self.input.read(bytes)?;
        self.hash.update(&bytes[..len]);
        Ok(len)
    }
}

pub(crate) fn stage(root: &Path, group: &str, cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    let app = activity()?;
    let (identity, files) = manifest()?.ok_or_else(|| {
        io::Error::other("Offline Android and Linux guest assets are missing from this APK")
    })?;
    if root.file_name().and_then(|s| s.to_str()) != Some(&identity) {
        return Err(io::Error::other(
            "Installed Android package changed; restart the workbench",
        ));
    }
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!(".{group}.lock")))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let files: BTreeMap<_, _> = files
        .into_iter()
        .filter(|(name, _)| name.split('/').next() == Some(group))
        .collect();
    let required: &[&str] = match group {
        "android" => &[
            "android/avf.json",
            "android/apps/input.apk",
            "android/apps/home.apk",
            "android/apps/browser.apk",
        ],
        "linux" => &["linux/avf.json"],
        "speech" => &["speech/ggml-base-q5_1.bin"],
        _ => return Err(io::Error::other("Unknown offline asset group")),
    };
    for name in required {
        if !files.contains_key(*name) && !files.contains_key(&format!("{name}.xz")) {
            return Err(io::Error::other(format!(
                "Missing offline component: {name}"
            )));
        }
    }
    let receipt = root.join(format!(".{group}.complete"));
    if receipt.is_file()
        && files
            .keys()
            .all(|p| root.join(p.strip_suffix(".xz").unwrap_or(p)).is_file())
    {
        return Ok(());
    }
    let mut buf = vec![0; 256 * 1024];
    for (name, expected) in files {
        let mut reader = HashReader {
            input: asset(&app, &name)?,
            hash: Sha256::new(),
        };
        let path = root.join(name.strip_suffix(".xz").unwrap_or(&name));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        let mut decoded: Box<dyn Read + '_> = if name.ends_with(".xz") {
            Box::new(xz2::read::XzDecoder::new_multi_decoder(&mut reader))
        } else {
            Box::new(&mut reader)
        };
        let mut size = 0u64;
        loop {
            if cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Installation cancelled",
                ));
            }
            let len = decoded.read(&mut buf)?;
            if len == 0 {
                break;
            }
            if buf[..len].iter().all(|&b| b == 0) {
                file.seek(SeekFrom::Current(len as i64))?;
            } else {
                file.write_all(&buf[..len])?;
            }
            size += len as u64;
        }
        drop(decoded);
        if size == 0 || hex::encode(reader.hash.finalize()) != expected {
            return Err(io::Error::other(format!(
                "Offline asset integrity check failed: {name}"
            )));
        }
        file.as_file().set_len(size)?;
        file.as_file().sync_all()?;
        file.persist(&path)?;
    }
    super::write_atomic(&receipt, identity.as_bytes()).map_err(io::Error::other)
}
