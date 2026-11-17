//! Android helper APK assembly and offline SDK file selection.
use crate::Result;
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn select(files: Vec<String>) -> Vec<String> {
    files
        .into_iter()
        .filter(|name| {
            !name.starts_with("sdk/emulator/lib64/qt/")
                && (!name.starts_with("sdk/emulator/qemu/windows-x86_64/qemu-system-")
                    || name == "sdk/emulator/qemu/windows-x86_64/qemu-system-x86_64-headless.exe")
        })
        .collect()
}

pub(super) fn sdk() -> Result<PathBuf> {
    Ok(env::var_os("ANDROID_HOME")
        .map(PathBuf::from)
        .ok_or("ANDROID_HOME is required")?)
}

fn jdk() -> Result<PathBuf> {
    Ok(PathBuf::from(env::var_os("JAVA_HOME").ok_or("JAVA_HOME is required")?).join("bin"))
}

fn program(directory: &Path, name: &str, extension: &str) -> Command {
    let file = directory.join(name);
    Command::new(if cfg!(windows) {
        file.with_extension(extension)
    } else {
        file
    })
}

fn run(command: &mut Command) -> Result<()> {
    if !command.stdout(Stdio::null()).status()?.success() {
        return Err(format!(
            "Android tool failed: {}",
            command.get_program().to_string_lossy()
        )
        .into());
    }
    Ok(())
}

fn matching_files(folder: &Path, extension: &str, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            matching_files(&path, extension, output)?;
        } else if kind.is_file() && path.extension() == Some(OsStr::new(extension)) {
            output.push(path);
        }
    }
    Ok(())
}

pub(super) fn inspect(
    tools: &Path,
    apk: &Path,
    package: &str,
    pin: Option<&OsStr>,
) -> Result<Value> {
    let certificates = program(tools, "apksigner", "bat")
        .args(["verify", "--print-certs"])
        .arg(apk)
        .output()?;
    if !certificates.status.success() {
        return Err(String::from_utf8_lossy(&certificates.stderr)
            .trim()
            .to_owned()
            .into());
    }
    let output = std::str::from_utf8(&certificates.stdout)?;
    let certificate =
        regex::Regex::new(r"(?m)^Signer #\d+ certificate SHA-256 digest: ([0-9a-f]{64})\r?$")?;
    let mut signatures = certificate.captures_iter(output);
    let (_, [signer]) = signatures.next().ok_or("APK signer missing")?.extract();
    if signatures.next().is_some() || pin.is_some_and(|pin| pin != signer) {
        return Err("APK signer mismatch".into());
    }
    let badging = program(tools, "aapt2", "exe")
        .args(["dump", "badging"])
        .arg(apk)
        .output()?;
    if !badging.status.success()
        || !badging
            .stdout
            .starts_with(format!("package: name='{package}'").as_bytes())
    {
        return Err("APK package mismatch".into());
    }
    Ok(json!({"package": package, "signer_sha256": signer}))
}

fn compile(sdk: &Path, sources: Vec<PathBuf>, classes: &Path, dex: &Path) -> Result<()> {
    let platform = sdk.join("platforms/android-35/android.jar");
    fs::create_dir(classes)?;
    run(program(&jdk()?, "javac", "exe")
        .args(["--release", "8", "-classpath"])
        .arg(&platform)
        .arg("-d")
        .arg(classes)
        .args(sources))?;
    let mut compiled = Vec::new();
    matching_files(classes, "class", &mut compiled)?;
    compiled.sort_unstable();
    run(program(&sdk.join("build-tools/35.0.0"), "d8", "bat")
        .arg("--lib")
        .arg(&platform)
        .args(["--min-api", "26", "--output"])
        .arg(dex)
        .args(compiled))
}

pub(super) fn notification_probe(source: PathBuf, destination: &Path) -> Result<()> {
    let build = tempfile::tempdir()?;
    compile(
        &sdk()?,
        vec![source],
        &build.path().join("classes"),
        destination,
    )
}

pub fn build(arguments: &[OsString]) -> Result<Value> {
    let [root, role] = arguments else {
        return Err("build-android-apk requires the project root and app role".into());
    };
    let role = match role.to_str() {
        Some(name @ ("home" | "input")) => name,
        _ => return Err("Android app role must be home or input".into()),
    };
    let (apk, mut receipt) = build_app(&std::path::absolute(root)?, role)?;
    receipt["file"] = serde_json::to_value(apk)?;
    Ok(receipt)
}

pub(super) fn build_app(root: &Path, role: &str) -> Result<(PathBuf, Value)> {
    let source = root.join("apps").join(format!("android-{role}"));
    let out = root.join("data/android").join(role);
    fs::create_dir_all(&out)?;
    let build = tempfile::tempdir_in(&out)?;
    let sdk = sdk()?;
    let tools = sdk.join("build-tools/35.0.0");
    let platform = sdk.join("platforms/android-35/android.jar");
    let tool = |name, extension| {
        let mut command = program(&tools, name, extension);
        command.current_dir(build.path());
        command
    };
    run(tool("aapt2", "exe")
        .args(["compile", "--dir"])
        .arg(source.join("res"))
        .args(["-o", "resources.zip"]))?;
    run(tool("aapt2", "exe")
        .args(["link", "-o", "base.apk", "-I"])
        .arg(&platform)
        .arg("--manifest")
        .arg(source.join("AndroidManifest.xml"))
        .args([
            "--min-sdk-version",
            "26",
            "--target-sdk-version",
            "35",
            "resources.zip",
        ]))?;
    let mut sources = Vec::new();
    matching_files(&source.join("src"), "java", &mut sources)?;
    sources.sort_unstable();
    compile(&sdk, sources, &build.path().join("classes"), build.path())?;
    run(tool("aapt", "exe").args(["add", "base.apk", "classes.dex"]))?;
    run(tool("zipalign", "exe").args(["-f", "4", "base.apk", "aligned.apk"]))?;
    let key = out.join("debug.jks");
    if !key.try_exists()? {
        run(program(&jdk()?, "keytool", "exe")
            .args(["-genkeypair", "-keystore"])
            .arg(&key)
            .args([
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "androiddebugkey",
            ])
            .args([
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "3650",
                "-dname",
            ])
            .arg(format!("CN=AI Phone {role},O=Local Development,C=CN")))?;
    }
    let apk = out.join(format!("ai-phone-{role}.apk"));
    run(tool("apksigner", "bat")
        .args(["sign", "--ks"])
        .arg(key)
        .args(["--ks-pass", "pass:android", "--out"])
        .arg(&apk)
        .arg("aligned.apk"))?;
    let receipt = inspect(&tools, &apk, &format!("dev.aiphone.{role}"), None)?;
    Ok((apk, receipt))
}
