use super::{Result, Signing, execute, resources::Plan};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[cfg(target_os = "macos")]
pub(super) fn assemble(
    root: &Path,
    plan: &Plan,
    binary: &Path,
    installer: &Path,
    version: &str,
    signing: Signing,
) -> Result<(Value, Value)> {
    use super::{json_file, output, resources::link};
    use std::os::unix::fs::symlink;
    let config = json_file(root.join("config/native-client.json"))?;
    let field = |name: &str| {
        config[name]
            .as_str()
            .ok_or_else(|| format!("Missing application metadata: {name}"))
    };
    let executable = field("executable")?;
    let bundle_id = field("bundle_id")?;
    let minimum = field("minimum_macos")?;
    let parent = installer.parent().ok_or("Installer directory missing")?;
    let workspace = tempfile::tempdir_in(parent)?;
    let contents = workspace.path().join("contents");
    let application = contents.join("AImanity.app");
    let app = application.join("Contents");
    let program = app.join("MacOS").join(executable);
    let resources = app.join("Resources");
    fs::create_dir_all(
        program
            .parent()
            .ok_or("Application binary directory missing")?,
    )?;
    fs::copy(binary, &program)?;
    link(
        &root.join("apps/native/icons/icon.icns"),
        &resources.join("icon.icns"),
    )?;
    for (input, target) in &plan.resources {
        link(&root.join(input), &resources.join(target))?;
    }
    let version_valid = version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !version_valid {
        return Err("Invalid application version".into());
    }
    fs::write(
        app.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>AImanity</string>
<key>CFBundleDisplayName</key><string>AImanity</string>
<key>CFBundleExecutable</key><string>{executable}</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>LSMinimumSystemVersion</key><string>{minimum}</string>
<key>NSMicrophoneUsageDescription</key><string>Copilot transcribes your speech locally.</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>"#
        ),
    )?;
    let identity = if signing == Signing::Release {
        let identity = std::env::var("AIMANITY_MAC_SIGNING_IDENTITY")?;
        if !identity.starts_with("Developer ID Application: ") {
            return Err("Developer ID Application identity required".into());
        }
        identity
    } else {
        "-".into()
    };
    let sign = |path: &Path| -> Result<()> {
        let mut command = Command::new("codesign");
        command.args(["--force", "--sign", &identity]);
        if signing == Signing::Release {
            command.arg("--timestamp");
            if path.extension().is_some_and(|ext| ext == "app") {
                command.args(["--options", "runtime"]);
            }
        } else {
            command.arg("--timestamp=none");
        }
        execute(command.arg(path))
    };
    let verify = |path: &Path| {
        execute(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(path),
        )
    };
    if signing != Signing::Unsigned {
        sign(&application)?;
    }
    let files = plan.inspect(root, &application, &program, &resources)?;
    symlink("/Applications", contents.join("Applications"))?;
    let image = workspace.path().join("client.dmg");
    execute(
        Command::new("hdiutil")
            .args(["create", "-srcfolder"])
            .arg(contents)
            .args([
                "-volname",
                "AImanity",
                "-format",
                "UDZO",
                "-imagekey",
                "zlib-level=1",
            ])
            .arg(&image),
    )?;
    if signing != Signing::Unsigned {
        sign(&image)?;
    }
    if signing == Signing::Release {
        let profile = std::env::var("AIMANITY_MAC_NOTARY_PROFILE")?;
        let receipt: Value = serde_json::from_str(&output(
            Command::new("xcrun")
                .args(["notarytool", "submit"])
                .arg(&image)
                .args([
                    "--keychain-profile",
                    &profile,
                    "--wait",
                    "--output-format",
                    "json",
                ]),
        )?)?;
        if receipt["status"] != "Accepted" {
            return Err(format!("Apple notarization returned {}", receipt["status"]).into());
        }
        for step in ["staple", "validate"] {
            execute(Command::new("xcrun").args(["stapler", step]).arg(&image))?;
        }
        execute(
            Command::new("spctl")
                .args([
                    "--assess",
                    "--type",
                    "open",
                    "--context",
                    "context:primary-signature",
                ])
                .arg(&image),
        )?;
    }
    execute(Command::new("hdiutil").arg("verify").arg(&image))?;
    if signing != Signing::Unsigned {
        verify(&image)?;
        let mount = workspace.path().join("readback");
        fs::create_dir(&mount)?;
        execute(
            Command::new("hdiutil")
                .args([
                    "attach",
                    "-readonly",
                    "-nobrowse",
                    "-noverify",
                    "-owners",
                    "off",
                    "-mountpoint",
                ])
                .arg(&mount)
                .arg(&image),
        )?;
        let verified = verify(&mount.join("AImanity.app"));
        let detached = execute(Command::new("hdiutil").arg("detach").arg(mount));
        verified?;
        detached?;
    }
    fs::rename(image, installer)?;
    let signature = match signing {
        Signing::Unsigned => {
            json!({"signing":"unsigned","developer_id_signed":false,"notarized":false})
        }
        Signing::Temporary => {
            json!({"signing":"ad-hoc","developer_id_signed":false,"notarized":false})
        }
        Signing::Release => {
            json!({"signing":"developer-id","developer_id_signed":true,"notarized":true})
        }
    };
    Ok((
        signature,
        json!({"bundle_id":bundle_id,"version":version,"minimum_macos":minimum,"files":files}),
    ))
}

#[cfg(windows)]
pub(super) fn assemble(
    root: &Path,
    plan: &Plan,
    binary: &Path,
    installer: &Path,
    _version: &str,
    signing: Signing,
) -> Result<(Value, Value)> {
    use super::{file_digest, resources::windows_program};
    let trusted = |file: &Path| -> Result<()> {
        let thumbprint = std::env::var("AIMANITY_WINDOWS_CERT_THUMBPRINT")?;
        let timestamp = std::env::var("AIMANITY_WINDOWS_TIMESTAMP_URL")?;
        if thumbprint.len() != 40
            || !thumbprint.bytes().all(|value| value.is_ascii_hexdigit())
            || timestamp.is_empty()
        {
            return Err("Windows certificate thumbprint and timestamp URL required".into());
        }
        execute(
            Command::new("signtool.exe")
                .args([
                    "sign",
                    "/sha1",
                    &thumbprint,
                    "/fd",
                    "SHA256",
                    "/tr",
                    &timestamp,
                    "/td",
                    "SHA256",
                ])
                .arg(file),
        )?;
        execute(
            Command::new("signtool.exe")
                .args(["verify", "/pa", "/tw"])
                .arg(file),
        )
    };
    if signing == Signing::Release {
        trusted(binary)?;
    }
    crate::windows_factory::assemble(root, binary, installer, &plan.resources)?;
    let signature = match signing {
        Signing::Unsigned => json!({"signing":"unsigned"}),
        Signing::Release => {
            trusted(installer)?;
            json!({"signing":"authenticode-trusted","authenticode":{"file_digest":"sha256","cryptographically_verified":true,"timestamped":true}})
        }
        Signing::Temporary => {
            let work =
                tempfile::tempdir_in(installer.parent().ok_or("Installer directory missing")?)?;
            let key = work.path().join("key.pem");
            let cert = work.path().join("certificate.pem");
            let der = work.path().join("certificate.der");
            let signed = work.path().join("setup.exe");
            execute(
                Command::new("openssl")
                    .args([
                        "req",
                        "-x509",
                        "-newkey",
                        "rsa:3072",
                        "-sha256",
                        "-noenc",
                        "-days",
                        "30",
                        "-subj",
                        "/CN=AImanity Temporary Signing",
                        "-addext",
                        "basicConstraints=critical,CA:FALSE",
                        "-addext",
                        "keyUsage=critical,digitalSignature",
                        "-addext",
                        "extendedKeyUsage=codeSigning",
                        "-keyout",
                    ])
                    .arg(&key)
                    .arg("-out")
                    .arg(&cert),
            )?;
            execute(
                Command::new("osslsigncode")
                    .args(["sign", "-certs"])
                    .arg(&cert)
                    .arg("-key")
                    .arg(&key)
                    .args(["-h", "sha256", "-n", "AImanity", "-in"])
                    .arg(installer)
                    .arg("-out")
                    .arg(&signed),
            )?;
            fs::remove_file(key)?;
            execute(
                Command::new("openssl")
                    .args(["x509", "-in"])
                    .arg(&cert)
                    .args(["-outform", "DER", "-out"])
                    .arg(&der),
            )?;
            execute(
                Command::new("osslsigncode")
                    .args(["verify", "-CAfile"])
                    .arg(&cert)
                    .arg("-in")
                    .arg(&signed),
            )?;
            fs::rename(signed, installer)?;
            json!({"signing":"authenticode-self-signed","authenticode":{"certificate_sha256":file_digest(&der)?,
                "file_digest":"sha256","cryptographically_verified":true,"timestamped":false}})
        }
    };
    let unpack = tempfile::tempdir()?;
    crate::windows_factory::unpack(root, installer, unpack.path(), &[r"{app}\*".into()])?;
    let program = windows_program(unpack.path())?;
    let header = fs::read(&program)?;
    let pe = u32::from_le_bytes(
        header
            .get(60..64)
            .ok_or("Missing executable header")?
            .try_into()?,
    ) as usize;
    if !header.starts_with(b"MZ") || header.get(pe..pe + 6) != Some(b"PE\0\0\x64\x86") {
        return Err("Unexpected Windows client architecture".into());
    }
    let folder = program.parent().ok_or("Windows bundle directory missing")?;
    let files = plan.inspect(root, unpack.path(), &program, folder)?;
    Ok((signature, json!({"files":files})))
}
