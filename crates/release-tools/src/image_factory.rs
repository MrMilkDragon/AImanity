use crate::Result;
use serde_json::Value;
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

pub(super) fn execute(command: &mut Command) -> Result<()> {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(format!("{}: {status}", command.get_program().to_string_lossy()).into());
    }
    Ok(())
}

pub(super) fn run(args: &[OsString]) -> Result<Value> {
    let [flag, arch, file] = args else {
        return Err("factory-images requires rootfs ARCH OUTPUT".into());
    };
    if flag != "rootfs" {
        return Err("factory-images requires rootfs ARCH OUTPUT".into());
    }
    crate::linux_rootfs::build(
        &std::env::current_dir()?,
        arch.to_str().ok_or("Invalid Linux architecture")?,
        Path::new(file),
    )
}
