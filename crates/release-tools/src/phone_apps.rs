use crate::{Result, android_package, assets, source};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io,
    path::Path,
};

const ROLES: [&str; 2] = ["home", "input"];
const FILES: [&str; 3] = ["browser.apk", "home.apk", "input.apk"];
const BROWSER: &str = "config/phone-browser.json";

#[derive(Deserialize)]
struct Browser {
    package: String,
    version: String,
    signer_sha256: String,
    abis: BTreeMap<String, assets::Pin>,
}

#[derive(Deserialize, Serialize, PartialEq)]
struct FileRecord {
    path: String,
    size: u64,
    sha256: String,
    mode: u32,
}

#[derive(Deserialize, Serialize)]
struct Manifest {
    format: u32,
    source_sha256: String,
    apps: BTreeMap<String, Value>,
    browser: Value,
    files: Vec<FileRecord>,
}

fn fingerprint(root: &Path) -> Result<String> {
    let mut paths = vec![
        BROWSER.into(),
        "crates/release-tools/src/android_package.rs".into(),
        "crates/release-tools/src/phone_apps.rs".into(),
    ];
    for role in ROLES {
        source::files(root, &format!("apps/android-{role}"), &mut paths)?;
    }
    paths.sort_unstable();
    let mut hash = Sha256::new();
    for path in paths {
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(Sha256::digest(fs::read(root.join(path))?));
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn inventory(folder: &Path) -> Result<Vec<FileRecord>> {
    FILES
        .into_iter()
        .map(|name| {
            let path = folder.join(name);
            if !fs::symlink_metadata(&path)?.is_file() {
                return Err(format!("Invalid phone component file: {name}").into());
            }
            let mut hash = Sha256::new();
            let size = io::copy(&mut File::open(path)?, &mut hash)?;
            Ok(FileRecord {
                path: format!("runtime/phone-apps/{name}"),
                size,
                sha256: format!("{:x}", hash.finalize()),
                mode: 0o644,
            })
        })
        .collect()
}

fn verify(folder: &Path, identity: &str) -> Result<Manifest> {
    let value: Manifest = serde_json::from_reader(File::open(folder.join("manifest.json"))?)?;
    if value.format != 1
        || value.source_sha256 != identity
        || !value.apps.keys().map(String::as_str).eq(ROLES)
    {
        return Err("Phone helpers differ from source".into());
    }
    if value.files != inventory(folder)? {
        return Err("Phone component files differ from manifest".into());
    }
    Ok(value)
}

fn build(root: &Path, cache: &Path, destination: &Path, identity: String) -> Result<Manifest> {
    fs::create_dir_all(cache)?;
    let work = tempfile::tempdir_in(cache)?;
    let mut apps = BTreeMap::new();
    for role in ROLES {
        let (apk, receipt) = android_package::build_app(root, role)?;
        fs::copy(apk, work.path().join(format!("{role}.apk")))?;
        apps.insert(role.into(), receipt);
    }
    let policy: Browser = serde_json::from_reader(File::open(root.join(BROWSER))?)?;
    let abi = if cfg!(target_arch = "aarch64") {
        "arm64-v8a"
    } else {
        "x86_64"
    };
    let browser = policy.abis[abi].stage(&cache.join("phone-browser"))?;
    android_package::verify_package(&browser, &policy.package)?;
    fs::hard_link(browser, work.path().join("browser.apk"))?;
    let manifest = Manifest {
        format: 1,
        source_sha256: identity,
        apps,
        browser: json!({"package":policy.package,"signer_sha256":policy.signer_sha256,"version":policy.version,"abi":abi}),
        files: inventory(work.path())?,
    };
    serde_json::to_writer_pretty(File::create(work.path().join("manifest.json"))?, &manifest)?;
    fs::rename(work.path(), destination)?;
    Ok(manifest)
}

pub(super) fn run(arguments: &[OsString]) -> Result<Value> {
    let [operation, root] = arguments else {
        return Err("phone-apps requires build or verify and the project root".into());
    };
    let root = std::path::absolute(root)?;
    let cache = root.join(".tools");
    let destination = cache.join("phone-apps");
    let identity = fingerprint(&root)?;
    let manifest = match operation.to_str() {
        Some("build") if !destination.try_exists()? => {
            build(&root, &cache, &destination, identity)?
        }
        Some("build" | "verify") => verify(&destination, &identity)?,
        _ => return Err("phone-apps requires build or verify".into()),
    };
    Ok(serde_json::to_value(manifest)?)
}
