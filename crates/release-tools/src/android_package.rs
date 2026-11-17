//! Android helper APK assembly and offline SDK file selection.
use crate::Result;
use apk::sign::{Keystore, Signer, verify};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

fn sdk() -> Result<PathBuf> {
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

pub(super) fn verify_package(path: &Path, package: &str) -> Result<()> {
    if apk::Apk::open(path)?.manifest()?.package() != Some(package) {
        return Err("APK package mismatch".into());
    }
    Ok(())
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
    let key = out.join("debug.jks");
    if !key.try_exists()? {
        run(Command::new("openssl")
            .current_dir(build.path())
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "3650",
                "-keyout", "key.pem", "-out", "cert.pem", "-subj",
            ])
            .arg(format!("/CN=AI Phone {role}/O=Local Development/C=CN")))?;
        let store = tempfile::NamedTempFile::new_in(&out)?;
        run(Command::new("openssl")
            .current_dir(build.path())
            .args([
                "pkcs12",
                "-export",
                "-name",
                "androiddebugkey",
                "-inkey",
                "key.pem",
                "-in",
                "cert.pem",
                "-passout",
                "pass:android",
                "-out",
            ])
            .arg(store.path()))?;
        store.persist_noclobber(&key)?;
    }
    let apk = out.join(format!("ai-phone-{role}.apk"));
    let identity = Keystore::open(key, "android")?.sole_identity(None)?;
    Signer::new(identity).sign(build.path().join("base.apk"), &apk)?;
    let signed = verify(&apk)?;
    let package = format!("dev.aiphone.{role}");
    verify_package(&apk, &package)?;
    let signer = format!("{:x}", Sha256::digest(signed.certificate().der()));
    Ok((apk, json!({"package": package, "signer_sha256": signer})))
}
