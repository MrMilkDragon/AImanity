use super::{Result, file_digest, json_file, output};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    process::Command,
};

fn metadata(manifest: &str, option: &str, target: Option<&str>) -> Result<Value> {
    let mut cargo = Command::new("cargo");
    cargo.args([
        "metadata",
        "--manifest-path",
        manifest,
        "--locked",
        "--format-version",
        "1",
        option,
    ]);
    cargo.args(target);
    Ok(serde_json::from_str(&output(&mut cargo)?)?)
}

fn field<'a>(row: &'a Value, name: &str) -> Result<&'a str> {
    row[name]
        .as_str()
        .ok_or_else(|| format!("Missing notice field: {name}").into())
}

fn checked_notice(folder: &Path, row: &Value) -> Result<String> {
    let relative = field(row, "file")?;
    let path = folder.join(relative);
    if file_digest(&path)? != field(row, "sha256")? {
        return Err(format!("Licence material changed: {relative}").into());
    }
    Ok(fs::read_to_string(path)?)
}

fn emit(text: &mut impl Write, name: &str, url: Option<&str>, value: &str) -> Result<()> {
    writeln!(text, "\n--- {name} ---")?;
    if let Some(url) = url {
        writeln!(text, "{url}")?;
    }
    for line in value.lines() {
        writeln!(text, "{}", line.trim_end_matches([' ', '\t']))?;
    }
    Ok(())
}

pub(super) fn write(root: &Path, target: &str, destination: &Path) -> Result<()> {
    let manifest = "apps/native/Cargo.toml";
    let graph = metadata(manifest, "--filter-platform", Some(target))?;
    let server = metadata("Cargo.toml", "--no-deps", None)?;
    let owners: BTreeSet<_> = [&server, &graph]
        .into_iter()
        .flat_map(|graph| graph["workspace_members"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let upstream = json_file(root.join("config/native-notice-supplements.json"))?;
    let runtime = json_file(root.join("config/rust-runtime-notices.json"))?;
    let lock = fs::read_to_string(root.join("apps/native/Cargo.lock"))?;
    let mut packages: Vec<_> = graph["packages"]
        .as_array()
        .ok_or("Missing Cargo packages")?
        .iter()
        .collect();
    packages.sort_unstable_by(|a, b| {
        (a["name"].as_str(), a["version"].as_str())
            .cmp(&(b["name"].as_str(), b["version"].as_str()))
    });
    fs::create_dir_all(root.join(".tools"))?;
    let mut text = BufWriter::new(File::create(destination)?);
    writeln!(
        text,
        "AImanity Iced workbench dependency notices\n\nLocked build graph for {target}, including build dependencies. Component licences remain independent."
    )?;
    let separator = "=".repeat(72);
    for package in packages {
        if owners.contains(field(package, "id")?) {
            continue;
        }
        let name = field(package, "name")?;
        let version = field(package, "version")?;
        let folder = Path::new(field(package, "manifest_path")?)
            .parent()
            .ok_or("Package parent missing")?;
        let mut paths = Vec::new();
        for file in fs::read_dir(folder)? {
            let file = file?;
            let name = file.file_name().to_string_lossy().to_ascii_uppercase();
            if file.file_type()?.is_file()
                && ["LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT"]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
            {
                paths.push(file.path());
            }
        }
        if let Some(file) = package["license_file"]
            .as_str()
            .map(|file| folder.join(file))
            && file.is_file()
            && !paths.contains(&file)
        {
            paths.push(file);
        }
        paths.sort_unstable();
        let key = format!("{name}@{version}");
        let extra = upstream["packages"]
            .as_array()
            .ok_or("Notice supplements missing")?
            .iter()
            .find(|row| row["package"] == key);
        if paths.is_empty() && extra.is_none() {
            return Err(format!("Missing workbench notices: {key}").into());
        }
        writeln!(
            text,
            "\n{separator}\n{name} {version}\nSPDX: {}\nSource: {}\nAttribution: {}",
            package["license"].as_str().unwrap_or("see licence file"),
            package["repository"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("https://crates.io/crates/{name}")),
            package["authors"]
                .as_array()
                .filter(|authors| !authors.is_empty())
                .map(|authors| authors
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_else(|| "See retained upstream grant/source".into())
        )?;
        for file in paths {
            emit(
                &mut text,
                &file
                    .file_name()
                    .ok_or("Notice filename missing")?
                    .to_string_lossy(),
                None,
                &fs::read_to_string(&file)?,
            )?;
        }
        if let Some(extra) = extra {
            if package["repository"] != extra["repository"] {
                return Err(format!("Notice repository differs: {key}").into());
            }
            if let Some(commit) = extra["commit"].as_str() {
                if json_file(folder.join(".cargo_vcs_info.json"))?["git"]["sha1"] != commit {
                    return Err(format!("Notice source differs: {key}").into());
                }
            } else {
                let name_field = format!("name = \"{name}\"");
                let version_field = format!("version = \"{version}\"");
                let checksum = format!("checksum = \"{}\"", field(extra, "archive_sha256")?);
                if !lock.split("[[package]]").any(|entry| {
                    [&name_field, &version_field, &checksum]
                        .into_iter()
                        .all(|field| entry.lines().any(|line| line == field))
                }) {
                    return Err(format!("Notice archive differs: {key}").into());
                }
                for row in extra["grant_files"]
                    .as_array()
                    .ok_or("Missing archive grants")?
                {
                    checked_notice(folder, row)?;
                }
            }
            for row in extra["files"]
                .as_array()
                .ok_or("Missing supplemental files")?
            {
                let file = field(row, "file")?;
                if !Path::new(file).starts_with("docs/public/licenses/native") {
                    return Err("Supplemental notice path outside its source tree".into());
                }
                let name = format!(
                    "{} (pinned grant or linked full licence text)",
                    file.rsplit('/').next().unwrap_or(file)
                );
                emit(
                    &mut text,
                    &name,
                    Some(field(row, "url")?),
                    &checked_notice(root, row)?,
                )?;
            }
        }
    }
    for package in runtime["packages"]
        .as_array()
        .ok_or("Rust runtime notices missing")?
    {
        writeln!(
            text,
            "\n{separator}\n{}\nStandard-library copyright and accompanying terms; compiler tools are not bundled.",
            field(package, "package")?
        )?;
        for row in package["files"]
            .as_array()
            .ok_or("Rust runtime notice files missing")?
        {
            let path = field(row, "file")?;
            emit(
                &mut text,
                path.rsplit('/').next().unwrap_or(path),
                Some(field(row, "url")?),
                &checked_notice(root, row)?,
            )?;
        }
    }
    text.flush()?;
    Ok(())
}
