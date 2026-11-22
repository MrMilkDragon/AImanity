use crate::{Result, capsule, source};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Serialize)]
struct Record {
    path: String,
    size: u64,
    mode: u32,
    sha256: String,
}

fn identity(reader: &mut impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    io::copy(reader, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn mach_arm(file: &Path) -> Result<()> {
    let mut file = File::open(file)?;
    let header = |file: &mut File| -> io::Result<[u8; 8]> {
        let mut data = [0; 8];
        file.read_exact(&mut data)?;
        Ok(data)
    };
    const THIN: [u8; 8] = [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 1];
    let bytes = header(&mut file)?;
    if bytes == THIN {
        return Ok(());
    }
    let (little, wide) = match bytes[..4] {
        [0xca, 0xfe, 0xba, 0xbe] => (false, false),
        [0xbe, 0xba, 0xfe, 0xca] => (true, false),
        [0xca, 0xfe, 0xba, 0xbf] => (false, true),
        [0xbf, 0xba, 0xfe, 0xca] => (true, true),
        _ => return Err("ARM64 Mach-O required".into()),
    };
    let int = if little {
        u32::from_le_bytes
    } else {
        u32::from_be_bytes
    };
    let int64 = if little {
        u64::from_le_bytes
    } else {
        u64::from_be_bytes
    };
    let count = int(bytes[4..].try_into()?);
    if !(1..=64).contains(&count) {
        return Err("Invalid Mach-O slices".into());
    }
    for index in 0..count {
        let width = if wide { 32 } else { 20 };
        let mut row = [0; 32];
        file.seek(SeekFrom::Start(8 + u64::from(index) * width as u64))?;
        file.read_exact(&mut row[..width])?;
        if int(row[..4].try_into()?) == 0x0100000c {
            let offset = if wide {
                int64(row[8..16].try_into()?)
            } else {
                int(row[8..12].try_into()?).into()
            };
            file.seek(SeekFrom::Start(offset))?;
            if header(&mut file)? == THIN {
                return Ok(());
            }
        }
    }
    Err("Native ARM64 slice missing".into())
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Record)>) -> Result<()> {
    for entry in fs::read_dir(root.join(directory))? {
        let entry = entry?;
        let relative = directory.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(root, &relative, files)?;
        } else if kind.is_file() {
            let path = relative
                .to_str()
                .ok_or("Invalid Android resource path")?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if path.contains(['\\', ':']) || path.chars().any(|c| c <= '\u{1f}') {
                return Err("Invalid Android resource path".into());
            }
            let executable = path.to_ascii_lowercase().ends_with(".exe")
                || matches!(path.as_str(), "platform-tools/adb" | "avo/avo-vmm");
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                executable || (entry.metadata()?.permissions().mode() & 0o111) != 0
            };
            let file = entry.path();
            files.push((
                file.clone(),
                Record {
                    path: format!("sdk/{path}"),
                    size: entry.metadata()?.len(),
                    mode: if executable { 0o755 } else { 0o644 },
                    sha256: identity(&mut File::open(file)?)?,
                },
            ));
        } else {
            return Err("Android capsule may only contain regular files".into());
        }
    }
    Ok(())
}

fn archive(writer: impl io::Write, manifest: &[u8], files: &[(PathBuf, Record)]) -> Result<()> {
    let mut archive = tar::Builder::new(writer);
    source::append_entry(
        &mut archive,
        "ANDROID-MANIFEST.json",
        manifest.len() as u64,
        0o644,
        manifest,
    )?;
    for (file, record) in files {
        source::append_entry(
            &mut archive,
            &record.path,
            record.size,
            record.mode,
            File::open(file)?,
        )?;
    }
    archive.finish()?;
    Ok(())
}

pub(super) fn seal(arguments: &[OsString]) -> Result<Value> {
    let [sdk, output, preset, host @ ..] = arguments else {
        return Err("android-component requires SDK OUTPUT PRESET [HOST]".into());
    };
    let host = match host {
        [] if cfg!(all(target_os = "macos", target_arch = "aarch64")) => "macos-arm64",
        [] if cfg!(all(target_os = "windows", target_arch = "x86_64")) => "windows-x64",
        [host] => host.to_str().ok_or("Invalid Android capsule host")?,
        _ => return Err("Unsupported Android capsule host".into()),
    };
    let (engine, directory, api, abi, architecture) = match host {
        "macos-arm64" => ("avo", "avo", 37, "arm64-v8a", "arm64"),
        "linux-x64" => ("kvm", "cuttlefish", 35, "x86_64", "amd64"),
        "windows-x64" => ("whpx", "cuttlefish", 35, "x86_64", "amd64"),
        _ => return Err("Unsupported Android capsule host".into()),
    };
    let preset: u32 = preset.to_str().ok_or("Invalid XZ preset")?.parse()?;
    let sdk = Path::new(sdk);
    if engine == "avo" {
        mach_arm(&sdk.join("avo/avo-vmm"))?;
        mach_arm(&sdk.join("platform-tools/adb"))?;
    }
    let mut files = Vec::new();
    collect(sdk, Path::new(directory), &mut files)?;
    if engine != "kvm" {
        collect(sdk, Path::new("platform-tools"), &mut files)?;
    }
    files.sort_unstable_by(|left, right| left.1.path.cmp(&right.1.path));
    let manifest = serde_json::to_vec(
        &json!({"format":1,"engine":engine,"host":host,"api":api,"abi":abi,"files":files.iter().map(|(_, r)| r).collect::<Vec<_>>()}),
    )?;

    let output = std::path::absolute(output)?;
    let folder = output
        .parent()
        .ok_or("Missing Android capsule output directory")?;
    fs::create_dir_all(folder)?;
    if output.try_exists()? {
        return Err("Use a new Android capsule filename".into());
    }
    let mut pending = tempfile::NamedTempFile::new_in(folder)?;
    if let Some(xz) = env::var_os("AIMANITY_FACTORY_XZ") {
        let args = if matches!(engine, "whpx" | "kvm") && preset == 6 {
            vec![
                "-T2".into(),
                "--block-size=128MiB".into(),
                "--lzma2=preset=7,dict=64MiB".into(),
            ]
        } else {
            vec![
                format!("-{preset}"),
                "-T6".into(),
                "--block-size=64MiB".into(),
            ]
        };
        let mut child = Command::new(xz)
            .args(args)
            .arg("--check=crc64")
            .stdin(Stdio::piped())
            .stdout(pending.reopen()?)
            .spawn()?;
        let written = archive(child.stdin.take().expect("XZ stdin"), &manifest, &files);
        if written.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        written?;
        if !status.success() {
            return Err("Android capsule compression failed".into());
        }
    } else {
        capsule::write_encoded(
            pending.as_file_mut(),
            architecture,
            preset,
            folder,
            |writer| archive(writer, &manifest, &files),
        )?;
    }
    pending.rewind()?;
    let archive_sha256 = identity(pending.as_file_mut())?;
    pending.persist_noclobber(&output)?;
    Ok(json!({
        "host":host, "abi":abi, "engine":engine, "files":files.len(),
        "bytes":files.iter().map(|(_, record)| record.size).sum::<u64>(),
        "archive_sha256":archive_sha256, "redistribution":"review-pending",
    }))
}
