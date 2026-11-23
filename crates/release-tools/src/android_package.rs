//! Android helper APK assembly and offline SDK file selection.
mod home_dex;
pub(super) mod network_dex;
mod receiver_dex;

use crate::Result;
use apk::sign::{Keystore, Signer, verify};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn mobile_dex(args: &[OsString]) -> Result<()> {
    use receiver_dex::{Class, emit_for};
    let [output] = args else { return Err("mobile-activity-dex OUTPUT".into()); };
    let classes = [
        Class {
            name: "Ldev/aimanity/mobile/KeyboardView;", parent: "Landroid/view/View;",
            interfaces: &[], constructor_args: &["Landroid/content/Context;"], fields: &[],
            natives: &[
                ("onCheckIsTextEditor", ("Z", &[]), 1),
                ("onCreateInputConnection", ("Landroid/view/inputmethod/InputConnection;", &["Landroid/view/inputmethod/EditorInfo;"]), 1),
                ("onKeyPreIme", ("Z", &["I", "Landroid/view/KeyEvent;"]), 1),
            ],
        },
        Class {
            name: "Ldev/aimanity/mobile/KeyboardConnection;", parent: "Landroid/view/inputmethod/BaseInputConnection;",
            interfaces: &[], constructor_args: &["Landroid/view/View;", "Z"],
            fields: &[("generation", "I", 1), ("preedit", "Ljava/lang/String;", 1)],
            natives: &[
                ("setComposingText", ("Z", &["Ljava/lang/CharSequence;", "I"]), 1),
                ("commitText", ("Z", &["Ljava/lang/CharSequence;", "I"]), 1),
                ("finishComposingText", ("Z", &[]), 1),
                ("deleteSurroundingText", ("Z", &["I","I"]), 1),
                ("deleteSurroundingTextInCodePoints", ("Z", &["I","I"]), 1),
                ("performEditorAction", ("Z", &["I"]), 1),
                ("sendKeyEvent", ("Z", &["Landroid/view/KeyEvent;"]), 1),
            ],
        },
        Class {
            name: "Ldev/aimanity/mobile/WorkbenchActivity;",
            parent: "Landroid/app/NativeActivity;", interfaces: &[], constructor_args: &[], fields: &[],
            natives: &[
                ("onDestroy", ("V", &[]), 4),
                ("onActivityResult", ("V", &["I", "I", "Landroid/content/Intent;"]), 4),
                ("onRequestPermissionsResult", ("V", &["I", "[Ljava/lang/String;", "[I"]), 1),
            ],
        },
        Class {
            name: "Ldev/aimanity/mobile/SpeechInit;", parent: "Ljava/lang/Object;",
            interfaces: &["Landroid/speech/tts/TextToSpeech$OnInitListener;"],
            constructor_args: &[], fields: &[], natives: &[("onInit", ("V", &["I"]), 1)],
        },
        Class {
            name: "Ldev/aimanity/mobile/SpeechProgress;", parent: "Landroid/speech/tts/UtteranceProgressListener;",
            interfaces: &[], constructor_args: &[], fields: &[],
            natives: &[("onStart", ("V", &["Ljava/lang/String;"]), 1), ("onDone", ("V", &["Ljava/lang/String;"]), 1), ("onError", ("V", &["Ljava/lang/String;"]), 1)],
        },
    ];
    Ok(fs::write(output, emit_for(&classes, "aimanity_native"))?)
}

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

pub(super) fn verify_package(path: &Path, package: &str) -> Result<()> {
    if apk::Apk::open(path)?.manifest()?.package() != Some(package) {
        return Err("APK package mismatch".into());
    }
    Ok(())
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
    let mut link = tool("aapt2", "exe");
    link.args(["link", "-o", "base.apk", "-I"])
        .arg(&platform)
        .arg("--manifest")
        .arg(source.join("AndroidManifest.xml"))
        .args(["--min-sdk-version", "26", "--target-sdk-version", "35"]);
    if role == "input" {
        let version = fs::read_to_string(source.join("version-code.txt"))?;
        link.args(["--version-code", version.trim()]);
    }
    run(link.arg("resources.zip"))?;
    let (dex, module, entry) = if role == "home" {
        (home_dex::emit(), "aimanity_home", "rust/main.rs")
    } else {
        (receiver_dex::emit(), "aimanity_input", "rust/receiver.rs")
    };
    fs::write(build.path().join("classes.dex"), dex)?;
    for (abi, target) in [
        ("arm64-v8a", "aarch64-linux-android"),
        ("x86_64", "x86_64-linux-android"),
    ] {
        let library = format!("lib/{abi}/lib{module}.so");
        fs::create_dir_all(build.path().join(format!("lib/{abi}")))?;
        crate::android_native::link(target, &source.join(entry), &build.path().join(&library))?;
        run(tool("aapt", "exe").args(["add", "base.apk", &library]))?;
    }
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
