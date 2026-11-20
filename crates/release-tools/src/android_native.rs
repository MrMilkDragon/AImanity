use crate::Result;
use std::{env, ffi::OsString, path::Path, process::Command};

pub(super) fn link(target: &str, source: &Path, library: &Path) -> Result<()> {
    let compiler = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let libdir = Command::new(&compiler)
        .args(["--print", "target-libdir"])
        .output()?;
    if !libdir.status.success() {
        return Err("Unable to locate the Rust linker".into());
    }
    let linker = Path::new(std::str::from_utf8(&libdir.stdout)?.trim())
        .join("../bin")
        .join(if cfg!(windows) {
            "rust-lld.exe"
        } else {
            "rust-lld"
        });
    let mut linker_arg = OsString::from("linker=");
    linker_arg.push(linker);
    let status = Command::new(compiler)
        .args([
            "--target",
            target,
            "--crate-type",
            "cdylib",
            "--edition",
            "2024",
            "-C",
            "linker-flavor=ld.lld",
            "-C",
            "panic=abort",
            "-C",
            "opt-level=s",
            "-C",
            "strip=symbols",
            "-C",
            "link-arg=--no-undefined",
            "-C",
            "link-arg=-z",
            "-C",
            "link-arg=max-page-size=16384",
            "-C",
        ])
        .arg(linker_arg)
        .arg(source)
        .arg("-o")
        .arg(library)
        .status()?;
    if !status.success() {
        return Err(format!("Android native build failed for {target}: {status}").into());
    }
    Ok(())
}
