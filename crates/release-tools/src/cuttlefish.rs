use crate::Result;
use std::{ffi::OsString, fs, path::absolute};

mod boot;
mod disk;

const MIB: u64 = 1 << 20;

pub(super) fn android_avf_boot(arguments: &[OsString]) -> Result<()> {
    let [images, output] = arguments else {
        return Err("android-avf-boot requires IMAGES OUTPUT".into());
    };
    boot::stage(
        &absolute(images)?,
        &absolute(output)?,
        boot::Target::Arm64Avf,
    )
}

pub(super) fn run(arguments: &[OsString]) -> Result<()> {
    let [images, output, qemu] = arguments else {
        return Err("windows-cuttlefish requires IMAGES OUTPUT QEMU_IMG".into());
    };
    let images = absolute(images)?;
    let output = absolute(output)?;
    let qemu = absolute(qemu)?;
    let parent = output
        .parent()
        .ok_or("Cuttlefish output needs a parent directory")?;
    fs::create_dir_all(parent)?;
    fs::create_dir(&output)?;
    let work = tempfile::tempdir_in(parent)?;
    boot::stage(&images, &output, boot::Target::X86Qemu)?;
    let mut os = Vec::new();
    for name in [
        "boot",
        "init_boot",
        "vendor_boot",
        "vbmeta",
        "vbmeta_system",
        "vbmeta_system_dlkm",
        "vbmeta_vendor_dlkm",
    ] {
        let file = images.join(format!("{name}.img"));
        let size = file.metadata()?.len();
        os.push(disk::Partition {
            label: format!("{name}_a"),
            size,
            image: Some(file),
            sparse: false,
        });
        os.push(disk::Partition::empty(format!("{name}_b"), size));
    }
    let file = images.join("super.img");
    os.push(disk::Partition {
        label: "super".into(),
        size: disk::sparse_size(&file)?,
        image: Some(file),
        sparse: true,
    });
    let raw = work.path().join("disk.raw");
    disk::create(&os, &raw, &output.join("disk.qcow2"), &qemu)?;
    for gb in [16, 24, 32] {
        let partitions = [
            disk::Partition::empty("misc", MIB),
            disk::Partition::empty("frp", MIB),
            disk::Partition::empty("metadata", 64 * MIB),
            disk::Partition::empty("userdata", gb * 1024 * MIB),
        ];
        disk::create(
            &partitions,
            &raw,
            &output.join(format!("userdata-{gb}.qcow2")),
            &qemu,
        )?;
    }
    Ok(())
}

fn bytes(data: &[u8], start: usize, length: usize) -> Result<&[u8]> {
    data.get(start..)
        .and_then(|tail| tail.get(..length))
        .ok_or_else(|| "Truncated Cuttlefish image".into())
}

fn le32(data: &[u8], offset: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(bytes(data, offset, 4)?.try_into()?) as usize)
}
