use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
};

const ROOTS: [&str; 5] = ["bin", "lib", "tessdata", "licenses", "source-records.json"];
const REQUIRED: [&str; 8] = [
    "bin/tesseract",
    "lib/ld-linux-x86-64.so.2",
    "lib/libtesseract.so.5",
    "tessdata/eng.traineddata",
    "tessdata/chi_sim.traineddata",
    "tessdata/configs/tsv",
    "source-records.json",
    "licenses/tesseract-ocr.copyright",
];
const INPUTS: [&str; 3] = [
    "crates/release-tools/src/linux_rootfs.rs",
    "crates/release-tools/src/windows_release/assets.rs",
    "crates/release-tools/src/linux_ocr.rs",
];

pub(super) fn output(program: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program).args(arguments).output()?;
    if !output.status.success() {
        return Err(format!("{program}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub(super) fn libraries(program: &str) -> Result<Vec<String>> {
    let libraries = output("ldd", &[program])?;
    if libraries.contains("=> not found") {
        return Err(format!("Missing native library for {program}").into());
    }
    Ok(libraries
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let first = fields.next()?;
            let path = if fields.next() == Some("=>") {
                fields.next()?
            } else {
                first
            };
            path.starts_with('/').then(|| path.to_owned())
        })
        .collect())
}

pub(super) fn collect(root: &Path) -> Result<()> {
    for folder in ["bin", "lib", "tessdata/configs", "licenses"] {
        fs::create_dir_all(root.join(folder))?;
    }
    let program = "/usr/bin/tesseract";
    let mut native_files = BTreeMap::new();
    let libraries = libraries(program)?;
    for file in std::iter::once(program).chain(libraries.iter().map(String::as_str)) {
        let name = Path::new(file)
            .file_name()
            .ok_or("Native file name missing")?;
        let target = Path::new(if file == program { "bin" } else { "lib" }).join(name);
        let destination = root.join(&target);
        let data = fs::read(file)?;
        if destination.exists() && fs::read(&destination)? != data {
            return Err(format!("Conflicting native library: {}", target.display()).into());
        }
        fs::write(&destination, &data)?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
        native_files.insert(
            file,
            json!({"target":target,"sha256":format!("{:x}", Sha256::digest(data))}),
        );
    }
    for model in ["eng.traineddata", "chi_sim.traineddata", "configs/tsv"] {
        fs::copy(
            Path::new("/usr/share/tesseract-ocr/5/tessdata").join(model),
            root.join("tessdata").join(model),
        )?;
    }
    let packages = output("dpkg-query", &["-W", "-f=${Package} ${Version}\n"])?;
    let packages: Vec<_> = packages.lines().collect();
    for row in &packages {
        let package = row
            .split_ascii_whitespace()
            .next()
            .ok_or("Package name missing")?;
        let source = Path::new("/usr/share/doc").join(package).join("copyright");
        if source.is_file() {
            fs::copy(
                source,
                root.join("licenses").join(format!("{package}.copyright")),
            )?;
        }
    }
    serde_json::to_writer(
        File::create(root.join("source-records.json"))?,
        &json!({"native_files":native_files,"packages":packages}),
    )?;
    Ok(())
}

#[derive(Deserialize, Serialize, PartialEq)]
struct Record {
    path: String,
    size: u64,
    sha256: String,
    mode: u32,
}

#[derive(Deserialize, Serialize, PartialEq)]
struct Manifest {
    format: u8,
    host: String,
    source_sha256: String,
    files: Vec<Record>,
}

fn inventory(root: &Path) -> Result<Vec<Record>> {
    let mut folders = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            let member = path.strip_prefix(root)?;
            let relative = member.to_str().ok_or("Invalid Linux OCR path")?;
            if relative == "manifest.json" {
                continue;
            }
            if !ROOTS.iter().any(|group| member.starts_with(group)) {
                return Err(format!("Unexpected Linux OCR file: {relative}").into());
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file() {
                let mut hash = Sha256::new();
                let size = io::copy(&mut File::open(&path)?, &mut hash)?;
                let mode = if entry.metadata()?.permissions().mode() & 0o111 == 0 {
                    0o644
                } else {
                    0o755
                };
                files.push(Record {
                    path: relative.into(),
                    size,
                    sha256: format!("{:x}", hash.finalize()),
                    mode,
                });
            } else {
                return Err(format!("Unsupported Linux OCR file: {relative}").into());
            }
        }
    }
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    if REQUIRED
        .iter()
        .any(|path| !files.iter().any(|record| record.path == *path))
    {
        return Err("Linux OCR closure is incomplete".into());
    }
    Ok(files)
}

fn source() -> Result<String> {
    let records = INPUTS
        .into_iter()
        .map(|file| Ok((file, format!("{:x}", Sha256::digest(fs::read(file)?)))))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&records)?)
    ))
}

fn verify(destination: &Path) -> Result<Value> {
    let recorded: Manifest =
        serde_json::from_reader(File::open(destination.join("manifest.json"))?)?;
    if recorded != manifest(destination)? {
        return Err("Linux OCR component differs from its manifest".into());
    }
    Ok(serde_json::to_value(recorded)?)
}

fn manifest(destination: &Path) -> Result<Manifest> {
    Ok(Manifest {
        format: 1,
        host: "linux-x64".into(),
        source_sha256: source()?,
        files: inventory(destination)?,
    })
}

pub(super) fn seal(destination: &Path) -> Result<()> {
    Ok(serde_json::to_writer(
        File::create(destination.join("manifest.json"))?,
        &manifest(destination)?,
    )?)
}

pub(super) fn run(args: &[OsString]) -> Result<Value> {
    match args {
        [operation, directory] if operation == "verify" => verify(Path::new(directory)),
        _ => Err("linux-ocr requires verify DIRECTORY".into()),
    }
}
