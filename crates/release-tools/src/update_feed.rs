use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::Path,
};

#[derive(Deserialize)]
struct Policy {
    public_key: String,
    feed_file: String,
}

#[derive(Deserialize)]
struct Release {
    version: String,
    release_name: String,
    release_notes: String,
}

#[derive(Serialize)]
struct Artifact {
    size: u64,
    sha512: String,
}

fn artifact(
    file: &Path,
    version: &str,
    os: &str,
    arch: &str,
    suffix: &str,
    from_stdin: bool,
) -> Result<Artifact> {
    let filename = format!("AImanity-{version}-{suffix}");
    if file.file_name().and_then(|name| name.to_str()) != Some(filename.as_str()) {
        return Err("Wrong installer filename".into());
    }
    let receipt: Value = serde_json::from_reader(File::open(
        file.with_file_name(format!("{filename}.manifest.json")),
    )?)?;
    if receipt["version"] != version
        || receipt["os"] != os
        || receipt["arch"] != arch
        || receipt["complete_device_bundle"] != true
        || receipt["runtime_payload_complete"] != true
    {
        return Err("Installer receipt does not match this release".into());
    }
    let signed = match receipt["signing"].as_str() {
        Some("ad-hoc") if os == "macos" => true,
        Some("developer-id") if os == "macos" => {
            receipt["developer_id_signed"] == true && receipt["notarized"] == true
        }
        Some("authenticode-self-signed") if os == "windows" => {
            receipt["authenticode"]["cryptographically_verified"] == true
        }
        Some("authenticode-trusted") if os == "windows" => {
            receipt["authenticode"]["cryptographically_verified"] == true
                && receipt["authenticode"]["timestamped"] == true
        }
        _ => false,
    };
    if !signed {
        return Err("Update requires a signed installer".into());
    }
    if os == "windows" {
        let files = receipt["payload"]["files"]
            .as_array()
            .ok_or("Windows installer file inventory missing")?;
        if ["guest.qcow2", "release-init.cpio.gz", "release.ext4"]
            .into_iter()
            .any(|name| {
                let path = format!("host/qemu/guest/{name}");
                !files.iter().any(|file| file["path"] == path)
            })
        {
            return Err("Windows installer requires its offline guest and release layer".into());
        }
    }
    let bytes: Box<dyn Read> = if from_stdin {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(file)?)
    };
    let mut hash = Sha512::new();
    let size = io::copy(&mut bytes.take(1 << 31), &mut hash)?;
    if !(1..1 << 31).contains(&size) {
        return Err("Installer exceeds the release asset limit".into());
    }
    let sha512 = STANDARD.encode(hash.finalize());
    if receipt["sha512"] != sha512 {
        return Err("Installer differs from its build receipt".into());
    }
    Ok(Artifact { size, sha512 })
}

pub(super) fn sign(arguments: &[OsString]) -> Result<()> {
    let (stdin_platform, arguments) = match arguments {
        [flag, platform, rest @ ..] if flag == "--stdin" => match platform.to_str() {
            Some(os @ ("macos" | "windows")) => (Some(os), rest),
            _ => return Err("--stdin expects macos or windows".into()),
        },
        rest => (None, rest),
    };
    let [key, mac, windows, output] = arguments else {
        return Err(
            "sign-update requires the private key, macOS DMG, Windows EXE and feed output".into(),
        );
    };
    let policy: Policy = serde_json::from_reader(File::open("config/client-update.json")?)?;
    let Release {
        version,
        release_name,
        release_notes,
    } = serde_json::from_reader(File::open("config/release-policy.json")?)?;
    let output = Path::new(output);
    if output.file_name().and_then(|name| name.to_str()) != Some(policy.feed_file.as_str()) {
        return Err(format!("Write the native update feed to {}", policy.feed_file).into());
    }
    if !regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$")?.is_match(&version)
        || release_name.trim().is_empty()
        || release_notes.trim().is_empty()
    {
        return Err("The native update feed requires a release version, name and notes".into());
    }
    let pem = std::fs::read(key)?;
    let (label, der) = pem_rfc7468::decode_vec(&pem)?;
    if label != "PRIVATE KEY" {
        return Err("Expected an Ed25519 PKCS#8 private key".into());
    }
    let key = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der)
        .map_err(|_| "Invalid Ed25519 private key")?;
    if key.public_key().as_ref() != STANDARD.decode(policy.public_key)? {
        return Err("Signing key does not match the client update public key".into());
    }
    let macos_arm64 = artifact(
        Path::new(mac),
        &version,
        "macos",
        "arm64",
        "macos-arm64.dmg",
        stdin_platform == Some("macos"),
    )?;
    let windows_x64 = artifact(
        Path::new(windows),
        &version,
        "windows",
        "x64",
        "windows-x64-setup.exe",
        stdin_platform == Some("windows"),
    )?;
    let manifest = serde_json::to_string(&json!({
        "format": 2, "version": version, "name": release_name.trim(), "notes": release_notes.trim(),
        "macos_arm64": macos_arm64, "windows_x64": windows_x64,
    }))?;
    let signature = STANDARD.encode(key.sign(manifest.as_bytes()).as_ref());
    let mut envelope =
        serde_json::to_vec(&json!({ "manifest": manifest, "signature": signature }))?;
    envelope.push(b'\n');
    if envelope.len() > 16 * 1024 {
        return Err("Update manifest exceeds the client limit".into());
    }
    std::io::Write::write_all(&mut File::create_new(output)?, &envelope)?;
    Ok(())
}
