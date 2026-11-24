use super::{Result, execute, json_file, linux_payload::copy_tree, notices, output};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};

const TITLE: &str = "AImanity native Android and Linux workbench";
const DESKTOP: &str = "[Desktop Entry]\nType=Application\nName=AImanity\nExec=/usr/bin/aimanity-workspace\nIcon=dev.aimanity.workspace\nTerminal=false\nCategories=Utility;\n";
const RPM_SPEC: &str = include_str!("aimanity.spec");
const KVM_RULE: &str = "SUBSYSTEM==\"misc\", KERNEL==\"kvm\", TAG+=\"uaccess\"\n";
const ACTIVATE_RULE: &str = "if [ -S /run/udev/control ]; then\n  udevadm control --reload\n  if [ -e /sys/class/misc/kvm ]; then udevadm trigger --action=change --settle /sys/class/misc/kvm; fi\nfi\n";

pub(crate) fn build(args: &[OsString]) -> Result<Value> {
    let [host, client, dist] = args else {
        return Err("linux-packages HOST_RESOURCES CLIENT_BINARY OUTPUT_DIRECTORY".into());
    };
    let host = fs::canonicalize(host)?;
    let binary = fs::canonicalize(client)?;
    let root = std::env::current_dir()?;
    let version = json_file(root.join("config/release-policy.json"))?["version"]
        .as_str()
        .ok_or("Missing installer version")?
        .to_owned();
    fs::create_dir_all(dist)?;
    let dist = fs::canonicalize(dist)?;
    let scratch = tempfile::tempdir_in(&dist)?;
    let image = scratch.path().join("package");
    let app = image.join("usr/lib/aimanity-workspace");
    let legal = image.join("usr/share/doc/aimanity-workspace");
    fs::create_dir_all(&legal)?;
    execute(
        Command::new(&binary)
            .arg("--verify-bundled-host")
            .arg(&host),
    )?;
    copy_tree(&host, &app.join("host"))?;
    fs::copy(&binary, app.join("aimanity-native"))?;
    fs::create_dir_all(image.join("usr/bin"))?;
    symlink(
        "../lib/aimanity-workspace/aimanity-native",
        image.join("usr/bin/aimanity-workspace"),
    )?;
    let applications = image.join("usr/share/applications");
    fs::create_dir_all(&applications)?;
    fs::write(applications.join("dev.aimanity.workspace.desktop"), DESKTOP)?;
    let rules = image.join("usr/lib/udev/rules.d");
    fs::create_dir_all(&rules)?;
    fs::write(rules.join("70-aimanity-workspace-kvm.rules"), KVM_RULE)?;
    let icons = image.join("usr/share/icons/hicolor/scalable/apps");
    fs::create_dir_all(&icons)?;
    fs::copy(
        root.join("apps/native/icons/source.svg"),
        icons.join("dev.aimanity.workspace.svg"),
    )?;
    notices::write(
        &root,
        "x86_64-unknown-linux-gnu",
        &legal.join("native-client-licenses.txt"),
    )?;
    for (input, name) in notices::LEGAL {
        if input != notices::NOTICES {
            fs::copy(root.join(input), legal.join(name))?;
        }
    }
    execute(
        Command::new("chmod")
            .args(["-R", "a+rX,go-w"])
            .arg(image.join("usr")),
    )?;
    let deb = dist.join(format!("AImanity-{version}-linux-amd64.deb"));
    fs::create_dir(image.join("DEBIAN"))?;
    let postinst = image.join("DEBIAN/postinst");
    fs::write(&postinst, format!("#!/bin/sh\nset -e\n{ACTIVATE_RULE}"))?;
    fs::set_permissions(postinst, fs::Permissions::from_mode(0o755))?;
    fs::create_dir(scratch.path().join("debian"))?;
    fs::write(
        scratch.path().join("debian/control"),
        "Source: aimanity-workspace\nSection: utils\nPriority: optional\nMaintainer: AImanity maintainers <maintainers@users.noreply.github.com>\n\nPackage: aimanity-workspace\nArchitecture: amd64\nDescription: AImanity desktop workbench\n",
    )?;
    let shlibs = output(
        Command::new("dpkg-shlibdeps")
            .current_dir(scratch.path())
            .arg("-O")
            .arg(&binary),
    )?;
    let linked = shlibs
        .strip_prefix("shlibs:Depends=")
        .ok_or("Missing native Debian dependencies")?;
    fs::write(
        image.join("DEBIAN/control"),
        format!(
            "Package: aimanity-workspace\nVersion: {version}\nArchitecture: amd64\nMaintainer: AImanity maintainers <maintainers@users.noreply.github.com>\nSection: utils\nPriority: optional\nDepends: {linked}, libvulkan1, libwayland-client0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0, pkexec, espeak-ng, fonts-noto-cjk, udev\nDescription: {TITLE}\n Offline workspace with optional Android and Linux devices and a shared agent.\n"
        ),
    )?;
    execute(
        Command::new("dpkg-deb")
            .args(["--build", "--root-owner-group", "-Zzstd", "-z6"])
            .arg(&image)
            .arg(&deb),
    )?;
    let spec = scratch.path().join("aimanity.spec");
    let elf = output(Command::new("objdump").arg("-p").arg(&binary))?;
    let linked = elf
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("NEEDED "))
        .map(|soname| format!("{}()(64bit)", soname.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        &spec,
        RPM_SPEC
            .replace("@VERSION@", &version)
            .replace("@ACTIVATE_KVM@", ACTIVATE_RULE)
            .replace("@LINKED@", &linked),
    )?;
    let rpmdir = scratch.path().join("RPMS");
    execute(
        Command::new("rpmbuild")
            .args(["-bb", "--target", "x86_64", "--define"])
            .arg(format!("_topdir {}", scratch.path().display()))
            .args([
                "--define",
                "_binary_payload w6.zstdio",
                "--define",
                "_build_id_links none",
            ])
            .env("AIMANITY_STAGE", &image)
            .arg(spec),
    )?;
    let rpm = dist.join(format!("AImanity-{version}-linux-x86_64.rpm"));
    fs::rename(
        rpmdir
            .join("x86_64")
            .join(format!("aimanity-workspace-{version}-1.x86_64.rpm")),
        &rpm,
    )?;
    for package in [&deb, &rpm] {
        if package.metadata()?.len() >= 2 * 1024u64.pow(3) {
            return Err(
                format!("{} exceeds the release asset size limit", package.display()).into(),
            );
        }
    }
    Ok(json!({"version":version,"deb":deb,"rpm":rpm,"architecture":"x86_64"}))
}
