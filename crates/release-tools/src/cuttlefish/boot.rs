use crate::Result;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use super::{bytes, le32};

fn be64(data: &[u8], offset: usize) -> Result<usize> {
    Ok(u64::from_be_bytes(bytes(data, offset, 8)?.try_into()?).try_into()?)
}

fn append_vbmeta(meta: &[u8], hash: &mut Sha256) -> Result<usize> {
    let size = 256 + be64(meta, 12)? + be64(meta, 20)?;
    hash.update(bytes(meta, 0, size)?);
    Ok(size)
}

fn vbmeta(root: &Path) -> Result<(usize, String)> {
    let meta = fs::read(root.join("vbmeta.img"))?;
    let mut hash = Sha256::new();
    let mut size = append_vbmeta(&meta, &mut hash)?;
    let mut offset = 256 + be64(&meta, 12)? + be64(&meta, 96)?;
    let end = offset + be64(&meta, 104)?;
    while offset < end {
        if be64(&meta, offset)? == 4 {
            let length = u32::from_be_bytes(bytes(&meta, offset + 20, 4)?.try_into()?) as usize;
            let name = std::str::from_utf8(bytes(&meta, offset + 92, length)?)?;
            if !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err("Invalid AVB partition name".into());
            }
            let mut image = fs::File::open(root.join(format!("{name}.img")))?;
            let mut head = [0; 28];
            image.read_exact(&mut head)?;
            if head.starts_with(b"AVB0") {
                let length = 256 + be64(&head, 12)? + be64(&head, 20)?;
                image.rewind()?;
                let mut payload = vec![0; length];
                image.read_exact(&mut payload)?;
                hash.update(payload);
                size += length;
            } else {
                image.seek(SeekFrom::End(-64))?;
                let mut footer = [0; 64];
                image.read_exact(&mut footer)?;
                if !footer.starts_with(b"AVBf") {
                    return Err(format!("Missing AVB footer in {name}").into());
                }
                let start = be64(&footer, 20)?;
                let length = be64(&footer, 28)?;
                image.seek(SeekFrom::Start(start as u64))?;
                let mut payload = vec![0; length];
                image.read_exact(&mut payload)?;
                hash.update(payload);
                size += length;
            }
        }
        offset += 16 + be64(&meta, offset + 8)?;
    }
    Ok((size, format!("{:x}", hash.finalize())))
}

fn ramdisk(out: &mut impl Write) -> Result<()> {
    const PROP: &[u8] =
        b"ro.vendor.disable_rename_eth0=1\nservice.adb.tcp.port=5555\nro.adb.secure=1\n";
    for (index, (name, data, mode)) in [
        ("system", &[][..], 0o040755),
        ("system/etc", &[][..], 0o040755),
        ("system/etc/ramdisk", &[][..], 0o040755),
        ("system/etc/ramdisk/build.prop", PROP, 0o100644),
        ("force_debuggable", &[][..], 0o100644),
        ("adb_debug.prop", PROP, 0o100644),
        ("TRAILER!!!", &[][..], 0),
    ]
    .into_iter()
    .enumerate()
    {
        out.write_all(b"070701")?;
        for field in [
            index + 1,
            mode,
            0,
            0,
            1,
            0,
            data.len(),
            0,
            0,
            0,
            0,
            name.len() + 1,
            0,
        ] {
            write!(out, "{field:08x}")?;
        }
        out.write_all(name.as_bytes())?;
        out.write_all(&[0; 4][..1 + ((111 + name.len()).next_multiple_of(4) - 111 - name.len())])?;
        out.write_all(data)?;
        out.write_all(&[0; 3][..data.len().next_multiple_of(4) - data.len()])?;
    }
    Ok(())
}

pub(super) fn stage(root: &Path, out: &Path) -> Result<()> {
    let boot = fs::read(root.join("boot.img"))?;
    let generic = fs::read(root.join("init_boot.img"))?;
    let vendor = fs::read(root.join("vendor_boot.img"))?;
    if !boot.starts_with(b"ANDROID!")
        || le32(&boot, 40)? != 4
        || !generic.starts_with(b"ANDROID!")
        || le32(&generic, 40)? != 4
        || !vendor.starts_with(b"VNDRBOOT")
        || le32(&vendor, 8)? != 4
    {
        return Err("Expected Cuttlefish Android boot image headers v4".into());
    }
    let kernel = bytes(&boot, 4096, le32(&boot, 8)?)?;
    if bytes(kernel, 514, 4)? != b"HdrS" {
        return Err("Expected Linux bzImage".into());
    }
    fs::write(out.join("bzImage"), kernel)?;

    let page = le32(&vendor, 12)?;
    let entry = le32(&vendor, 2120)?;
    let table_size = le32(&vendor, 2112)?;
    if page == 0 || entry < 12 || !table_size.is_multiple_of(entry) {
        return Err("Invalid Cuttlefish vendor ramdisk table".into());
    }
    let base = le32(&vendor, 2096)?.next_multiple_of(page);
    let table = base
        + le32(&vendor, 24)?.next_multiple_of(page)
        + le32(&vendor, 2100)?.next_multiple_of(page);
    let config = table + table_size.next_multiple_of(page);
    let official = std::str::from_utf8(bytes(&vendor, config, le32(&vendor, 2124)?)?)?.trim();
    let (size, digest) = vbmeta(root)?;
    let entries = include_str!("bootconfig.txt")
        .replace("{vbmeta.size}", &size.to_string())
        .replace("{vbmeta.digest}", &digest);
    let mut body = format!("{official}\n{entries}").into_bytes();
    body.resize((body.len() + 1).next_multiple_of(4), 0);
    let checksum = body
        .iter()
        .fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));

    let mut initrd = fs::File::create_new(out.join("initrd.img"))?;
    ramdisk(&mut initrd)?;
    for row in bytes(&vendor, table, table_size)?.chunks_exact(entry) {
        if le32(row, 8)? != 2 {
            initrd.write_all(bytes(&vendor, base + le32(row, 4)?, le32(row, 0)?)?)?;
        }
    }
    initrd.write_all(bytes(&generic, 4096, le32(&generic, 12)?)?)?;
    initrd.write_all(&body)?;
    initrd.write_all(&u32::try_from(body.len())?.to_le_bytes())?;
    initrd.write_all(&checksum.to_le_bytes())?;
    initrd.write_all(b"#BOOTCONFIG\n")?;
    Ok(())
}
