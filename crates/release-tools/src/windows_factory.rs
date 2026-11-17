use crate::{Result, assets};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    path::{self, Component, Path},
    process::{Command, Stdio},
};

const BINARIES: [&str; 4] = [
    "runtime/qemu-system-x86_64.exe",
    "runtime/qemu-img.exe",
    "runtime/share/bios-256k.bin",
    "runtime/share/linuxboot_dma.bin",
];
const GUEST: [&str; 4] = [
    "guest.qcow2",
    "bzImage",
    "kernel.config",
    "KERNEL-LICENSE.txt",
];

#[derive(Deserialize, Serialize)]
struct Record {
    path: String,
    size: u64,
    sha256: String,
}

fn checked(command: &mut Command) -> Result<()> {
    let status = command.stdin(Stdio::null()).status()?;
    if !status.success() {
        return Err(format!(
            "{} exited with {status}",
            command.get_program().to_string_lossy()
        )
        .into());
    }
    Ok(())
}

fn argument(prefix: &str, value: impl AsRef<OsStr>) -> OsString {
    let mut argument = OsString::from(prefix);
    argument.push(value);
    argument
}

fn unpack(root: &Path, installer: &Path, output: &Path, files: &[OsString]) -> Result<()> {
    checked(
        Command::new(assets::windows_unpacker(root)?)
            .args(["-x", "-b", "-q", "-a", "-c{app}"])
            .arg(argument("-d", output))
            .arg(installer)
            .args(files),
    )
}

fn read_json(file: &Path) -> Result<Value> {
    Ok(serde_json::from_reader(File::open(file)?)?)
}

fn stage_runtime(source: &Path, manifest: &Value, target: &Path) -> Result<Vec<Record>> {
    if manifest["format"] != 1 || manifest["component"] != "windows-qemu-whpx" {
        return Err("Invalid Windows QEMU factory manifest".into());
    }
    let source = source.canonicalize()?;
    let rows: Vec<Record> = serde_json::from_value(manifest["files"].clone())?;
    let allowed = regex::Regex::new(
        r"^(?:runtime/[A-Za-z0-9_.-]+(?i:\.dll)|licenses/(?:[A-Za-z0-9_-][A-Za-z0-9_.-]*/)*[A-Za-z0-9_-][A-Za-z0-9_.-]*)$",
    )?;
    let mut paths = BTreeSet::new();
    let mut files = Vec::new();
    for row in rows
        .into_iter()
        .filter(|row| !row.path.starts_with("source/"))
    {
        if !(BINARIES.contains(&row.path.as_str()) || allowed.is_match(&row.path))
            || !paths.insert(row.path.to_ascii_lowercase())
        {
            return Err(format!("Unexpected Windows QEMU file: {}", row.path).into());
        }
        let input = source.join(&row.path);
        let output = target.join(&row.path);
        let info = input.symlink_metadata()?;
        if !info.is_file() || !input.canonicalize()?.starts_with(&source) || info.len() != row.size
        {
            return Err(format!("Windows QEMU file changed: {}", row.path).into());
        }
        fs::create_dir_all(output.parent().expect("QEMU file has a directory"))?;
        if !assets::verify_stream::<Sha256>(
            File::open(input)?,
            File::create_new(output)?,
            row.size,
            &row.sha256,
        )? {
            return Err(format!("Windows QEMU file changed: {}", row.path).into());
        }
        files.push(row);
    }
    if BINARIES.iter().any(|file| !paths.contains(*file))
        || !paths.contains("licenses/qemu/copying")
    {
        return Err("Windows QEMU runtime or license missing".into());
    }
    Ok(files)
}

fn qemu(root: &Path, target: &Path, installer: Option<&OsString>) -> Result<Value> {
    if !cfg!(target_arch = "x86_64") || target.try_exists()? {
        return Err("Prepare the QEMU component in a new directory on Windows x64".into());
    }
    let parent = target.parent().ok_or("Missing QEMU output directory")?;
    fs::create_dir_all(parent)?;
    let work = tempfile::tempdir_in(parent)?;
    let policy = read_json(&root.join("config/windows-qemu.json"))?;
    let pin: assets::Pin = serde_json::from_value(policy["calcium_installer"].clone())?;
    let installer = match installer {
        Some(file) => {
            let file = path::absolute(file)?;
            pin.verify(&file)?;
            file
        }
        None => pin.stage(work.path())?,
    };
    let extraction = work.path().join("extracted");
    let mut members: Vec<OsString> = GUEST
        .iter()
        .map(|name| format!(r"{{app}}\host\qemu\guest\{name}").into())
        .collect();
    members.extend(
        [
            r"{app}\host\qemu\manifest.json",
            r"{app}\host\qemu\runtime\*",
            r"{app}\host\qemu\licenses\*",
        ]
        .map(OsString::from),
    );
    unpack(root, &installer, &extraction, &members)?;
    let bundled = extraction.join("host/qemu");
    let installed = fs::read(bundled.join("manifest.json"))?;
    let digest = format!("{:x}", Sha256::digest(&installed));
    if policy["bundled_manifest_sha256"] != digest {
        return Err("QEMU manifest differs from the pinned Calcium runtime".into());
    }
    let installed_manifest: Value = serde_json::from_slice(&installed)?;
    let mut component = installed_manifest["qemu"].clone();
    if component["version"] != "11.1.3" {
        return Err("Expected the qualified QEMU 11.1.3 component".into());
    }
    let rows: Vec<Record> = serde_json::from_value(installed_manifest["files"].clone())?;
    component["files"] = json!(
        rows.iter()
            .filter(|row| row.path.starts_with("runtime/") || row.path.starts_with("licenses/"))
            .collect::<Vec<_>>()
    );
    let stage = work.path().join("output");
    stage_runtime(&bundled, &component, &stage)?;
    let baseline = stage.join("baseline/host/qemu");
    fs::create_dir_all(baseline.join("guest"))?;
    fs::write(baseline.join("manifest.json"), installed)?;
    for name in GUEST {
        let path = format!("guest/{name}");
        let source = bundled.join(&path);
        let row = rows
            .iter()
            .find(|row| row.path == path)
            .ok_or("Missing pinned Calcium guest file")?;
        if source.symlink_metadata()?.len() != row.size {
            return Err(format!("Missing pinned Calcium guest file: {name}").into());
        }
        fs::rename(source, baseline.join(path))?;
    }
    let runtime = stage.join("runtime");
    let accelerators = Command::new(runtime.join("qemu-system-x86_64.exe"))
        .args(["-accel", "help"])
        .output()?;
    let version = Command::new(runtime.join("qemu-img.exe"))
        .arg("--version")
        .output()?;
    if !accelerators.status.success()
        || !version.status.success()
        || !String::from_utf8_lossy(&accelerators.stdout)
            .lines()
            .any(|name| name.eq_ignore_ascii_case("whpx"))
        || !String::from_utf8_lossy(&version.stdout).contains("11.1.3")
    {
        return Err("QEMU runtime failed the Windows x64 component check".into());
    }
    let mut origin = policy["calcium_installer"].clone();
    origin["manifest_sha256"] = json!(digest);
    component["binary_origin"] = origin;
    serde_json::to_writer_pretty(File::create(stage.join("windows-qemu.json"))?, &component)?;
    fs::rename(stage, target)?;
    Ok(component)
}

fn assemble(root: &Path, binary: &Path, target: &Path) -> Result<Value> {
    let resources: BTreeMap<String, String> = serde_json::from_reader(io::stdin().lock())?;
    let folder = root.join(".tools/windows-installer");
    let compiler_folder = folder.join("compiler");
    let compiler = compiler_folder.join("ISCC.exe");
    if !compiler.try_exists()? {
        let mut policy =
            read_json(&root.join("config/windows-installer.json"))?["compiler"].clone();
        policy["file"] = json!("inno-setup.exe");
        let archive = serde_json::from_value::<assets::Pin>(policy)?.stage(&folder)?;
        unpack(root, &archive, &compiler_folder, &[r"{app}\*".into()])?;
    }
    let stage = tempfile::tempdir_in(&folder)?;
    let stage = stage.path();
    fs::hard_link(binary, stage.join("ai-phone-client.exe"))?;
    for (input, output) in resources {
        let input = Path::new(&input);
        let output = Path::new(&output);
        if [input, output].iter().any(|file| {
            file.components()
                .any(|part| !matches!(part, Component::Normal(_)))
        }) {
            return Err("Invalid installer resource path".into());
        }
        let path = stage.join(output);
        fs::create_dir_all(path.parent().expect("staged file has a directory"))?;
        fs::hard_link(root.join(input), path)?;
    }
    let release = read_json(&root.join("config/release-policy.json"))?;
    let version = release["version"]
        .as_str()
        .ok_or("Missing release version")?;
    let numeric = version.split('-').next().unwrap_or(version);
    let manifest = read_json(&stage.join("host/qemu/manifest.json"))?;
    let rows: Vec<Record> = serde_json::from_value(manifest["files"].clone())?;
    let base = rows
        .iter()
        .find(|row| row.path == "guest/guest.qcow2")
        .ok_or("Missing Windows base disk")?;
    let output = target
        .parent()
        .ok_or("Missing installer output directory")?;
    let filename = target.file_stem().ok_or("Missing installer output name")?;
    checked(
        Command::new(compiler)
            .arg("/Qp")
            .arg(format!("/DAppVersion={version}"))
            .arg(format!("/DNumericVersion={numeric}.0"))
            .arg(argument("/DAppSource=", stage))
            .arg(argument("/DProjectRoot=", root))
            .arg(format!("/DBaseDiskSHA256={}", base.sha256))
            .arg(argument("/O", output))
            .arg(argument("/F", filename))
            .arg(root.join("apps/native/windows/installer.iss")),
    )?;
    Ok(json!({"installer":target.file_name(),"bytes":target.metadata()?.len()}))
}

pub(super) fn run(arguments: &[OsString]) -> Result<Option<Value>> {
    match arguments {
        [action, root, target, installer @ ..] if action == "qemu" && installer.len() <= 1 => qemu(
            &path::absolute(root)?,
            &path::absolute(target)?,
            installer.first(),
        )
        .map(Some),
        [action, source, manifest, target] if action == "stage-qemu" => {
            let manifest = read_json(Path::new(manifest))?;
            let files = stage_runtime(Path::new(source), &manifest, Path::new(target))?;
            Ok(Some(json!({"manifest":manifest,"files":files})))
        }
        [action, root, installer, target, files @ ..] if action == "unpack" => {
            let all = [OsString::from(r"{app}\*")];
            let files = if files.is_empty() { &all } else { files };
            unpack(
                &path::absolute(root)?,
                &path::absolute(installer)?,
                &path::absolute(target)?,
                files,
            )?;
            Ok(None)
        }
        [action, root, binary, output] if action == "assemble" => assemble(
            &path::absolute(root)?,
            &path::absolute(binary)?,
            &path::absolute(output)?,
        )
        .map(Some),
        _ => Err("Expected windows-factory qemu|stage-qemu|unpack|assemble".into()),
    }
}
