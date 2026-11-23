use super::receiver_dex::{Class, emit_for};
use crate::{Result, android_native};
use std::{ffi::OsString, fs, path::Path};

const CLASS: Class = Class {
    name: "Ldev/aiphone/network/DataUsageWarnings;",
    parent: "Ljava/lang/Object;",
    constructor_args: &[],
    interfaces: &[],
    fields: &[],
    natives: &[("main", ("V", &["[Ljava/lang/String;"]), 9)],
};

const GUEST_LIBRARY: &str = "/data/local/tmp/aimanity-network/libaimanity_network.so";

pub(crate) fn build(args: &[OsString]) -> Result<()> {
    let [root, abi, output] = args else {
        return Err("build-android-network requires ROOT ABI OUTPUT".into());
    };
    let target = match abi.to_str() {
        Some("arm64-v8a") => "aarch64-linux-android",
        Some("x86_64") => "x86_64-linux-android",
        _ => return Err("Unsupported Android network helper ABI".into()),
    };
    let output = Path::new(output);
    fs::create_dir_all(output)?;
    android_native::link(
        target,
        &Path::new(root).join("scripts/android-network/rust/main.rs"),
        &output.join("libaimanity_network.so"),
    )?;
    fs::write(
        output.join("network.dex"),
        emit_for(&[CLASS], GUEST_LIBRARY),
    )?;
    Ok(())
}
