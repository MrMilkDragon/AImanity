use crate::{
    Result, android_component, assets,
    client_installer::{self, execute, file_digest, json_file, output},
    client_payload, native_services, phone_apps, source,
};
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

fn field<'a>(row: &'a Value, key: &str) -> Result<&'a str> {
    row[key]
        .as_str()
        .ok_or_else(|| format!("Missing Mac factory field: {key}").into())
}

fn pin(folder: &Path, name: &str, row: &Value) -> Result<PathBuf> {
    let path = folder.join(name);
    assets::stage_pin::<Sha256>(
        field(row, "url")?,
        &path,
        row["size_bytes"].as_u64(),
        field(row, "sha256")?,
    )?;
    Ok(path)
}

fn min_version(program: &Path, maximum: &str) -> Result<String> {
    let build = output(
        Command::new("xcrun")
            .args(["vtool", "-show-build"])
            .arg(program),
    )?;
    let versions: Vec<_> = build
        .lines()
        .filter_map(|line| line.trim().strip_prefix("minos "))
        .collect();
    let [version] = versions.as_slice() else {
        return Err("Native minimum macOS missing".into());
    };
    let parts = |release: &str| -> Result<_> {
        let mut parts = [0u32; 3];
        for (part, value) in parts.iter_mut().zip(release.split('.')) {
            *part = value.parse()?;
        }
        Ok(parts)
    };
    if parts(version)? > parts(maximum)? {
        return Err("Native helper requires newer macOS".into());
    }
    Ok((*version).to_owned())
}

fn build_lima(policy: &Value, work: &Path, output_dir: &Path) -> Result<PathBuf> {
    let version = field(&policy["runtime"], "lima")?;
    let minimum = field(policy, "minimum_macos")?;
    let build = &policy["runtime"]["lima_build"];
    let commit = field(build, "source_commit")?;
    let go_version = field(build, "go_version")?;
    let binary = output_dir.join("limactl");
    let receipt = output_dir.join("build.json");
    let identity = format!("{:x}", Sha256::digest(serde_json::to_vec(policy)?));
    if receipt.try_exists()? {
        let cached = json_file(&receipt)?;
        if cached["identity"] != identity || cached["sha256"] != file_digest(&binary)? {
            return Err("Native Lima build differs from policy".into());
        }
        return Ok(binary);
    }
    let go_archive = pin(work, "go.tar.gz", &build["go_archive"])?;
    let go_root = work.join("go-compiler");
    tar::Archive::new(GzDecoder::new(File::open(go_archive)?)).unpack(&go_root)?;
    let go = go_root.join("go/bin/go");
    if output(Command::new(&go).arg("version"))? != format!("go version {go_version} darwin/arm64")
    {
        return Err("Pinned Go compiler required".into());
    }
    let checkout = work.join("lima-source");
    execute(Command::new("git").arg("init").arg(&checkout))?;
    execute(Command::new("git").current_dir(&checkout).args([
        "fetch",
        "--no-tags",
        "--depth=1",
        field(build, "source_url")?,
        commit,
    ]))?;
    execute(Command::new("git").current_dir(&checkout).args([
        "checkout",
        "--detach",
        "FETCH_HEAD",
    ]))?;
    if output(
        Command::new("git")
            .current_dir(&checkout)
            .args(["rev-parse", "HEAD"]),
    )? != commit
    {
        return Err("Native Lima checkout does not match its pin".into());
    }
    let temporary = tempfile::tempdir_in(output_dir.parent().ok_or("Missing Lima cache parent")?)?;
    let program = temporary.path().join("limactl");
    let mut command = Command::new(&go);
    command.current_dir(&checkout).args(["build","-mod=readonly","-trimpath","-p","2","-ldflags"])
        .arg(format!("-s -w -X github.com/lima-vm/lima/v2/pkg/version.Version={version} -linkmode=external -extldflags=-mmacosx-version-min={minimum}"))
        .arg("-o").arg(&program).arg("./cmd/limactl")
        .envs([("GOOS","darwin"),("GOARCH","arm64"),("CGO_ENABLED","1"),("GOMAXPROCS","2"),
            ("GOTOOLCHAIN","local"),("GOWORK","off"),("GOENV","off"),("GOSUMDB","sum.golang.org"),
            ("GOPROXY","https://proxy.golang.org"),("GOPRIVATE",""),("GONOSUMDB",""),("GONOPROXY",""),
            ("GOINSECURE",""),("GOFLAGS",""),("MACOSX_DEPLOYMENT_TARGET",minimum)])
        .env("CGO_CFLAGS",format!("-O2 -g0 -mmacosx-version-min={minimum}"))
        .env("CGO_LDFLAGS",format!("-mmacosx-version-min={minimum}"));
    for name in ["GOCACHE", "GOMODCACHE", "GOTMPDIR"] {
        let cache = work.join(name);
        fs::create_dir(&cache)?;
        command.env(name, cache);
    }
    execute(&mut command)?;
    execute(
        Command::new("codesign")
            .args(["-f", "--entitlements"])
            .arg(checkout.join("vz.entitlements"))
            .args(["-s", "-"])
            .arg(&program),
    )?;
    execute(
        Command::new("codesign")
            .args(["--verify", "--strict"])
            .arg(&program),
    )?;
    if output(Command::new(&program).arg("--version"))? != format!("limactl version {version}") {
        return Err("Native Lima version mismatch".into());
    }
    let minimum = min_version(&program, minimum)?;
    serde_json::to_writer(
        File::create(temporary.path().join("build.json"))?,
        &json!({"identity":identity,"source_commit":commit,"minimum_macos":minimum,"sha256":file_digest(&program)?}),
    )?;
    fs::rename(temporary.path(), output_dir)?;
    Ok(binary)
}

fn stage_host(root: &Path, work: &Path) -> Result<Value> {
    let destination = root.join(".tools/container-host");
    let folder = destination.parent().ok_or("Missing host component cache")?;
    let policy = client_payload::container_policy(root)?;
    let identity = format!("{:x}", Sha256::digest(serde_json::to_vec(&policy)?));
    if destination.try_exists()? {
        let record = json_file(destination.join("manifest.json"))?;
        if record["policy_sha256"] != identity {
            return Err("Staged Mac host differs from its policy".into());
        }
        return Ok(record);
    }
    let lima_tar = pin(work, "lima.tar.gz", &policy["lima"])?;
    let iso = pin(work, "image.iso", &policy["runtime"]["image"])?;
    let stage = tempfile::tempdir_in(folder)?;
    let lima = stage.path().join("lima");
    tar::Archive::new(GzDecoder::new(File::open(lima_tar)?)).unpack(&lima)?;
    let program = lima.join("bin/limactl");
    let limactl = build_lima(&policy, work, &folder.join("lima-cargo-build"))?;
    fs::copy(limactl, &program)?;
    let minimum = min_version(&program, field(&policy, "minimum_macos")?)?;
    fs::hard_link(iso, stage.path().join("image.iso"))?;
    let packages = stage.path().join("packages");
    fs::create_dir(&packages)?;
    for package in policy["runtime"]["packages"]
        .as_array()
        .ok_or("Mac package pins missing")?
    {
        let name = field(package, "file")?;
        let downloaded = pin(work, name, package)?;
        fs::hard_link(downloaded, packages.join(name))?;
    }
    let mut paths = vec![stage.path().to_owned()];
    let mut files = Vec::new();
    while let Some(folder) = paths.pop() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let file = entry.path();
            let meta = entry.file_type()?;
            if meta.is_dir() {
                paths.push(file);
            } else if meta.is_file() {
                let mode = if entry.metadata()?.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                };
                files.push(json!({"path":file.strip_prefix(stage.path())?,"mode":mode,"size":entry.metadata()?.len(),"sha256":file_digest(&file)?}));
            } else if !meta.is_symlink()
                || file.strip_prefix(stage.path())? != Path::new("lima/share/doc/lima/templates")
            {
                return Err(format!("Unsupported Lima archive member: {}", file.display()).into());
            }
        }
    }
    files.sort_unstable_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let manifest = json!({"format":1,"host":"macos-arm64","policy_sha256":identity,"native_targets":{"lima":minimum},"files":files});
    serde_json::to_writer_pretty(File::create(stage.path().join("manifest.json"))?, &manifest)?;
    fs::rename(stage.path(), destination)?;
    Ok(manifest)
}

fn admit_images(root: &Path, folder: &Path, commit: &str, expected: &str) -> Result<PathBuf> {
    let metadata = folder.join("factory-images-arm64.json");
    let info = metadata.symlink_metadata()?;
    if !info.is_file() || info.len() > 1024 * 1024 || file_digest(&metadata)? != expected {
        return Err("Mac image metadata differs".into());
    }
    let row = json_file(metadata)?;
    if row["format"] != 1
        || row["kind"] != "factory-container-images"
        || row["source_commit"] != commit
        || row["source_manifest_sha256"] != file_digest(&root.join("SOURCE-MANIFEST.json"))?
        || row["architecture"] != "arm64"
        || row["redistribution"] != "review-pending"
        || row["binary_publication"] != false
        || row["user_state_bundled"] != false
    {
        return Err("Mac image handoff source differs".into());
    }
    let name = "factory-images-arm64.payload";
    let file = folder.join(name);
    let info = file.symlink_metadata()?;
    let artifact = &row["artifact"];
    if !info.is_file()
        || info.len() == 0
        || info.len() >= 2 * 1024 * 1024 * 1024
        || artifact["file"] != name
        || artifact["size"] != info.len()
        || artifact["sha256"] != file_digest(&file)?
    {
        return Err("Mac image handoff bytes differ".into());
    }
    let mut archive = tar::Archive::new(File::open(&file)?);
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.path_bytes().as_ref() == b"IMAGES-MANIFEST.json" {
            let manifest: Value = serde_json::from_reader(entry)?;
            let images = manifest["images"].as_object().ok_or("Mac images missing")?;
            if manifest["architecture"] != "arm64"
                || images.len() != 2
                || !["workspace", "desktop"]
                    .iter()
                    .all(|name| images.contains_key(*name))
            {
                return Err("Mac desktop and workspace images must be ARM64".into());
            }
            return Ok(file);
        }
    }
    Err("Mac image handoff manifest missing".into())
}

fn staged_android(assistant: &Path, stage: &Path) -> Result<(PathBuf, Value)> {
    let (path, digest) = match (
        std::env::var_os("AIMANITY_ANDROID_SDK_INPUT"),
        std::env::var_os("AIMANITY_ANDROID_PAYLOAD"),
        std::env::var("AIMANITY_ANDROID_PAYLOAD_SHA256").ok(),
    ) {
        (Some(sdk), None, None) => {
            let path = stage.join("android.payload");
            let receipt = android_component::seal(&[sdk, path.as_os_str().into(), "3".into()])?;
            (path, field(&receipt, "archive_sha256")?.to_owned())
        }
        (None, Some(path), Some(digest)) => (std::path::absolute(path)?, digest),
        _ => return Err("Provide one sealed Android component or one staged SDK".into()),
    };
    let scratch = tempfile::tempdir_in(stage)?;
    let receipt: Value = serde_json::from_str(&output(
        Command::new(assistant)
            .arg("--prepare-android")
            .arg(scratch.path())
            .arg(&path)
            .arg(&digest),
    )?)?;
    let sdk = Path::new(field(&receipt, "sdkRoot")?)
        .parent()
        .ok_or("Android SDK parent missing")?;
    let manifest = json_file(sdk.join("ANDROID-MANIFEST.json"))?;
    if manifest["engine"] != "avo" {
        return Err("Mac Android component requires Avo".into());
    }
    let files = manifest["files"]
        .as_array()
        .ok_or("Android file inventory missing")?;
    Ok((
        path,
        json!({"host":manifest["host"],"abi":manifest["abi"],"engine":"avo","files":files.len(),
        "bytes":files.iter().filter_map(|row|row["size"].as_u64()).sum::<u64>(),"archive_sha256":digest,
        "redistribution":"review-pending","programs_prepared":true,"guest_boot":"not-executed"}),
    ))
}

pub(super) fn run(args: &[OsString]) -> Result<Value> {
    let mut sdk = None;
    let mut inputs = args.iter();
    let mut image = None;
    let mut identity = None;
    let mut sign = OsStr::new("--sign-temporary");
    while let Some(flag) = inputs.next() {
        match flag.to_str() {
            Some("--compiler-sdk") => sdk = Some(inputs.next().ok_or("Missing compiler SDK")?),
            Some("--image-candidate") => {
                image = Some(inputs.next().ok_or("Missing image candidate")?)
            }
            Some("--image-metadata-sha256") => {
                identity = Some(
                    inputs
                        .next()
                        .and_then(|x| x.to_str())
                        .ok_or("Missing image identity")?,
                )
            }
            Some("--sign-release") => sign = flag,
            _ => return Err("Unknown Mac factory option".into()),
        }
    }
    let root = std::env::current_dir()?;
    let cached = !root.join(".tools/container-host/manifest.json").is_file()
        || !root.join(".tools/phone-apps/manifest.json").is_file();
    let minimum = if cached { 35 } else { 24 };
    if fs2::available_space(&root)? < minimum * 1024 * 1024 * 1024 {
        return Err(format!("Mac factory requires {minimum} GiB free").into());
    }
    source::run(&["check".into(), "--repository".into(), "--manifest".into()])?;
    let commit = output(Command::new("git").args(["rev-parse", "HEAD"]))?;
    let checksum = file_digest(&root.join("SOURCE-MANIFEST.json"))?;
    let images = admit_images(
        &root,
        Path::new(image.ok_or("Missing Mac image handoff")?),
        &commit,
        identity.ok_or("Missing Mac image identity")?,
    )?;
    fs::create_dir_all(root.join(".tools"))?;
    let work = tempfile::tempdir_in(root.join(".tools"))?;
    let engine = stage_host(&root, work.path())?;
    let apps = if let Some(sdk) = sdk {
        let receipt = output(
            Command::new(std::env::current_exe()?)
                .args([
                    OsStr::new("phone-apps"),
                    OsStr::new("build"),
                    root.as_os_str(),
                ])
                .env("ANDROID_HOME", sdk)
                .env("ANDROID_SDK_ROOT", sdk),
        )?;
        serde_json::from_str(&receipt)?
    } else {
        phone_apps::run(&["verify".into(), root.as_os_str().into()])?
    };
    native_services::run(&["build".into()])?;
    let (android_file, android) =
        staged_android(&root.join("target/release/ai-phone-assistant"), work.path())?;
    let bytes = images.metadata()?.len();
    client_payload::run(&[
        "--with-phone-apps".into(),
        "--with-service-runtime".into(),
        "--with-container-host".into(),
        "--android-runtime".into(),
        android_file.into(),
        "--images-runtime".into(),
        images.into(),
    ])?;
    let receipt = client_installer::run(&[sign.into()])?;
    let delivered = &receipt["installer_delivery"];
    if receipt["complete_device_bundle"] != true || delivered["github_asset_eligible"] != true {
        return Err("Mac installer incomplete or over delivery limit".into());
    }
    if file_digest(&root.join("SOURCE-MANIFEST.json"))? != checksum {
        return Err("Mac factory source inventory changed".into());
    }
    let report = json!({
        "format":1,"host":"macos-arm64","source_commit":commit,"source_manifest_sha256":checksum,"container_host":engine,"android":android,
        "artifact":{"file":delivered["file"],"size":delivered["size"],"sha256":receipt["sha256"],
        "delivery":{"prebuilt_components":receipt["prebuilt_components"],"complete_device_bundle":true,"runtime_payload_complete":true,"server_included":true,
        "missing_runtime_components":[],"deployment_mode":"embedded-local","server_requirement":"None; the client initializes its local device runtime."}},
        "package_delivery":{"format":"single","file":delivered["file"],"size":delivered["size"],"sha256":receipt["sha256"]},
        "image_capsule_bytes":bytes,"phone_apps_source_sha256":apps["source_sha256"],"physical_installation":"not-tested","binary_publication":false,
    });
    serde_json::to_writer_pretty(
        File::create(root.join("dist/macos-full-factory-report.json"))?,
        &report,
    )?;
    Ok(report)
}

pub(super) fn fetch_images(args: &[OsString]) -> Result<Value> {
    use std::process::Stdio;

    let [tag, id, identity, destination] = args else {
        return Err("fetch-macos-images TAG RELEASE_ID METADATA_DIGEST DESTINATION".into());
    };
    let root = std::env::current_dir()?;
    let commit = output(Command::new("git").args(["rev-parse", "HEAD"]))?;
    let tag = tag.to_str().ok_or("Invalid image tag")?;
    let prefix = commit.get(..12).ok_or("Invalid factory commit")?;
    if !regex::Regex::new(&format!(r"^candidate-{prefix}-[0-9]+-[0-9a-f]{{32}}$"))?.is_match(tag) {
        return Err("Unexpected image handoff tag".into());
    }
    let id: u64 = id.to_str().ok_or("Invalid release identifier")?.parse()?;
    let identity = identity.to_str().ok_or("Invalid metadata digest")?;
    let destination = std::path::absolute(destination)?;
    if id == 0 || destination.try_exists()? {
        return Err("Image handoff needs a draft and a fresh destination".into());
    }
    let repository = std::env::var("GITHUB_REPOSITORY")?;
    if !regex::Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")?.is_match(&repository) {
        return Err("Invalid image factory repository".into());
    }
    let endpoint = format!("repos/{repository}/releases");
    let release: Value = serde_json::from_str(&output(
        Command::new("gh").args(["api", &format!("{endpoint}/{id}")]),
    )?)?;
    if release["id"] != id
        || release["draft"] != true
        || !release["published_at"].is_null()
        || release["target_commitish"] != commit
        || release["tag_name"] != tag
    {
        return Err("Handoff is not its owned unpublished draft".into());
    }
    let assets = release["assets"]
        .as_array()
        .filter(|assets| assets.len() == 2)
        .ok_or("Unexpected image asset inventory")?;
    let parent = destination.parent().ok_or("Missing image handoff parent")?;
    fs::create_dir_all(parent)?;
    let stage = tempfile::tempdir_in(parent)?;
    for name in ["factory-images-arm64.json", "factory-images-arm64.payload"] {
        let descriptor = assets
            .iter()
            .find(|asset| asset["name"] == name && asset["state"] == "uploaded")
            .ok_or("Missing image handoff asset")?;
        let size = descriptor["size"]
            .as_u64()
            .filter(|size| *size > 0 && *size < 2 * 1024 * 1024 * 1024)
            .ok_or("Invalid image asset size")?;
        let asset = descriptor["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or("Invalid image asset ID")?;
        let digest = field(descriptor, "digest")?
            .strip_prefix("sha256:")
            .ok_or("Unexpected image asset digest")?;
        if name.ends_with(".json") && (digest != identity || size > 1024 * 1024) {
            return Err("Image metadata does not match its draft".into());
        }
        let mut process = Command::new("gh")
            .args([
                "api",
                &format!("{endpoint}/assets/{asset}"),
                "-H",
                "Accept: application/octet-stream",
            ])
            .stdout(Stdio::piped())
            .spawn()?;
        let received = assets::verify_stream::<Sha256>(
            process
                .stdout
                .take()
                .ok_or("Missing image download stream")?,
            File::create_new(stage.path().join(name))?,
            size,
            digest,
        );
        if !matches!(received, Ok(true)) {
            let _ = process.kill();
        }
        if !process.wait()?.success() || !received? {
            return Err(format!("Image download changed: {name}").into());
        }
    }
    admit_images(&root, stage.path(), &commit, identity)?;
    fs::rename(stage.path(), &destination)?;
    Ok(json!({"directory":destination}))
}
