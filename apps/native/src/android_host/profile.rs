use ai_phone_api::wire::VirtualDeviceProfile as Guest;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};
use tempfile::NamedTempFile;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn seed(source: &Path, destination: &Path, cancelled: &dyn Fn() -> bool) -> io::Result<()> {
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
        if cancelled() {
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
    gb: u32,
    cancelled: &dyn Fn() -> bool,
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
    cancelled: &dyn Fn() -> bool,
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
    kind: &str,
    guest: &Guest,
    locale: &str,
    key: &[u8; 32],
    cancelled: &dyn Fn() -> bool,
) -> io::Result<PathBuf> {
    let seed = match kind {
        "phone" => "android",
        "desktop" => "linux",
        _ => return Err(invalid("Unknown guest system")),
    };
    let source = resources.join(seed).canonicalize()?;
    fs::create_dir_all(state)?;
    let state = state.canonicalize()?;
    let block = if kind == "desktop" {
        let request = serde_json::to_vec(&json!({"locale":locale,"desktop":{
            "cpus":guest.cpus,"memory_mb":guest.memory_mb,"storage_gb":guest.storage_gb
        }}))?;
        let path = state.join("control.raw");
        let mut file = NamedTempFile::new_in(&state)?;
        file.write_all(b"AIMAVF01")?;
        file.write_all(key)?;
        file.write_all(&(request.len() as u32).to_le_bytes())?;
        file.write_all(&request)?;
        file.as_file().set_len(4096)?;
        file.as_file().sync_all()?;
        file.persist(&path)?;
        Some(path)
    } else {
        None
    };
    configure(&source, &state, guest, block.as_deref(), cancelled)
}
