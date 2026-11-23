use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Deserialize)]
struct Guest {
    cpus: u8,
    memory_mb: u16,
    storage_gb: u8,
}

#[derive(Deserialize)]
struct Request {
    phone: Guest,
    desktop: Guest,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn seed(source: &Path, destination: &Path, cancelled: &AtomicBool) -> io::Result<()> {
    if destination.is_file() {
        return Ok(());
    }
    let parent = destination.parent().unwrap();
    fs::create_dir_all(parent)?;
    let mut pending = NamedTempFile::new_in(parent)?;
    let mut input = File::open(source)?;
    let mut buf = vec![0; 256 * 1024];
    let mut size = 0;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Installation cancelled",
            ));
        }
        let read = input.read(&mut buf)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if buf[..read].iter().all(|b| *b == 0) {
            pending.seek(SeekFrom::Current(read as i64))?;
        } else {
            pending.write_all(&buf[..read])?;
        }
    }
    pending.as_file().set_len(size)?;
    pending.as_file().sync_all()?;
    pending.persist_noclobber(destination)?;
    Ok(())
}

fn image(
    value: &mut Value,
    source: &Path,
    state: &Path,
    writable: bool,
    gb: u8,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let path = value.as_str().ok_or_else(|| invalid("AVF image path"))?;
    let name = path.replace("{storage_gb}", &gb.to_string());
    let relative = Path::new(&name);
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(invalid(format!("Invalid bundled path {path}")));
    }
    let original = source.join(relative);
    let metadata = original.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(invalid(format!("Empty guest resource: {name}")));
    }
    let target = if writable {
        let target = state.join(relative);
        seed(&original, &target, cancelled)?;
        let root = relative == Path::new("rootfs.img");
        let data = relative == Path::new("userdata.img");
        if root || data {
            let disk = fs::OpenOptions::new().write(true).open(&target)?;
            let requested = u64::from(gb) << 30;
            let current = disk.metadata()?.len();
            if current > requested {
                return Err(invalid("AVF guest disk shrinking is unsupported"));
            }
            if current < requested {
                disk.set_len(requested)?;
                disk.sync_all()?;
            }
        }
        target
    } else {
        original
    };
    *value = json!(target);
    Ok(())
}

fn configure(
    source: &Path,
    state: &Path,
    guest: &Guest,
    control: Option<&Path>,
    cancelled: &AtomicBool,
) -> io::Result<PathBuf> {
    fs::create_dir_all(state)?;
    let mut vm: Value = serde_json::from_reader(File::open(source.join("avf.json"))?)?;
    if vm.get("kernel").is_some() == vm.get("bootloader").is_some() {
        return Err(invalid("AVF requires a kernel or a bootloader"));
    }
    for field in ["kernel", "initrd", "bootloader"] {
        if let Some(value) = vm.get_mut(field) {
            image(value, source, state, false, guest.storage_gb, cancelled)?;
        }
    }
    let disks = vm["disks"]
        .as_array_mut()
        .ok_or_else(|| invalid("Missing AVF disks"))?;
    for disk in &mut *disks {
        let writable = disk["writable"].as_bool().unwrap_or(false);
        if let Some(value) = disk.get_mut("image") {
            image(value, source, state, writable, guest.storage_gb, cancelled)?;
        } else {
            let partitions = disk["partitions"]
                .as_array_mut()
                .ok_or_else(|| invalid("AVF disk partitions"))?;
            for partition in partitions {
                let writable = partition["writable"].as_bool().unwrap_or(false);
                image(
                    &mut partition["path"],
                    source,
                    state,
                    writable,
                    guest.storage_gb,
                    cancelled,
                )?;
            }
        }
    }
    if let Some(block) = control {
        disks.push(json!({"image":block,"writable":false}));
    }
    if vm.get("kernel").is_some() {
        let params = vm["params"].as_str().unwrap_or_default();
        vm["params"] = json!(format!("{params} nr_cpus={}", guest.cpus).trim());
    } else if guest.cpus != 1 {
        return Err(invalid(
            "Exact AVF vCPU selection requires direct kernel boot",
        ));
    }
    vm["memory_mib"] = json!(guest.memory_mb);
    vm["cpu_topology"] = json!(if guest.cpus == 1 {
        "one_cpu"
    } else {
        "match_host"
    });
    let mut file = NamedTempFile::new_in(state)?;
    serde_json::to_writer(&mut file, &vm)?;
    file.flush()?;
    let destination = state.join("avf.json");
    file.persist(&destination)?;
    Ok(destination)
}

pub fn prepare(
    resources: &Path,
    state: &Path,
    request: &Path,
    cancelled: &AtomicBool,
) -> io::Result<[PathBuf; 2]> {
    let resources = resources.canonicalize()?;
    let state = state.canonicalize()?;
    let request_bytes = fs::read(request)?;
    let request: Request = serde_json::from_slice(&request_bytes)?;
    for (guest, memory, storage) in [
        (
            &request.phone,
            &[1536u16, 2048, 3072, 4096][..],
            &[16u8, 24, 32][..],
        ),
        (
            &request.desktop,
            &[2048u16, 2560, 3072, 4096][..],
            &[16u8, 32, 64][..],
        ),
    ] {
        if ![1, 2, 4].contains(&guest.cpus)
            || !memory.contains(&guest.memory_mb)
            || !storage.contains(&guest.storage_gb)
        {
            return Err(invalid("Invalid AVF device resources"));
        }
    }
    let key: [u8; 32] = fs::read(state.join("control.key"))?
        .try_into()
        .map_err(|_| invalid("Invalid AVF control key"))?;
    let block = state.join("control.raw");
    let mut file = NamedTempFile::new_in(&state)?;
    file.write_all(b"AIMAVF01")?;
    file.write_all(&key)?;
    file.write_all(
        &u32::try_from(request_bytes.len())
            .map_err(invalid_size)?
            .to_le_bytes(),
    )?;
    file.write_all(&request_bytes)?;
    let length = file.stream_position()?.next_multiple_of(4096);
    file.as_file().set_len(length)?;
    file.as_file().sync_all()?;
    file.persist(&block)?;
    Ok([
        configure(
            &resources.join("android"),
            &state.join("android"),
            &request.phone,
            None,
            cancelled,
        )?,
        configure(
            &resources.join("linux"),
            &state.join("linux"),
            &request.desktop,
            Some(&block),
            cancelled,
        )?,
    ])
}

fn invalid_size(_: std::num::TryFromIntError) -> io::Error {
    invalid("Device profile too large")
}
