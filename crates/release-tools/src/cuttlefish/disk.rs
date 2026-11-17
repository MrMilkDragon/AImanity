use crate::Result;
use md5::{Digest, Md5};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
};

use super::MIB;

const SECTOR: u64 = 512;
const PARTITION_TYPE: [u8; 16] = [
    0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7,
];

pub(super) struct Partition {
    pub label: String,
    pub size: u64,
    pub image: Option<PathBuf>,
    pub sparse: bool,
}

impl Partition {
    pub(super) fn empty(label: impl Into<String>, size: u64) -> Self {
        Self {
            label: label.into(),
            size,
            image: None,
            sparse: false,
        }
    }
}

fn put<const N: usize>(target: &mut [u8], at: usize, data: [u8; N]) {
    target[at..at + N].copy_from_slice(&data);
}

struct Sparse {
    header: u16,
    chunk: u16,
    block: u64,
    blocks: u64,
    chunks: u32,
}

impl Sparse {
    fn read(input: &mut File) -> Result<Self> {
        let mut h = [0; 28];
        input.read_exact(&mut h)?;
        let two = |at| u16::from_le_bytes([h[at], h[at + 1]]);
        let four = |at| u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]]);
        if four(0) != 0xed26ff3a || two(8) < 28 || two(10) < 12 || four(12) == 0 {
            return Err("Invalid Android sparse partition header".into());
        }
        Ok(Self {
            header: two(8),
            chunk: two(10),
            block: four(12).into(),
            blocks: four(16).into(),
            chunks: four(20),
        })
    }
}

pub(super) fn sparse_size(path: &Path) -> Result<u64> {
    let sparse = Sparse::read(&mut File::open(path)?)?;
    Ok(sparse.block * sparse.blocks)
}

fn copy(
    source: &mut File,
    target: &mut File,
    size: u64,
    buffer: &mut [u8],
    pattern: Option<[u8; 4]>,
) -> Result<()> {
    if let Some(pattern) = pattern {
        for chunk in buffer.chunks_exact_mut(4) {
            chunk.copy_from_slice(&pattern);
        }
    }
    let mut remaining = size;
    while remaining > 0 {
        let amount = remaining.min(buffer.len() as u64) as usize;
        let block = &mut buffer[..amount];
        if pattern.is_none() {
            source.read_exact(block)?;
        }
        if block.iter().any(|byte| *byte != 0) {
            target.write_all(block)?;
        } else {
            target.seek(SeekFrom::Current(i64::try_from(amount)?))?;
        }
        remaining -= amount as u64;
    }
    Ok(())
}

fn copy_partition(
    source: &Path,
    target: &mut File,
    partition: &Partition,
    buffer: &mut [u8],
) -> Result<()> {
    let mut input = File::open(source)?;
    if !partition.sparse {
        return copy(&mut input, target, partition.size, buffer, None);
    }
    let sparse = Sparse::read(&mut input)?;
    let mut offset = u64::from(sparse.header);
    let mut written = 0;
    for _ in 0..sparse.chunks {
        input.seek(SeekFrom::Start(offset))?;
        let mut chunk = [0; 12];
        input.read_exact(&mut chunk)?;
        let kind = u16::from_le_bytes(chunk[..2].try_into()?);
        let blocks = u32::from_le_bytes(chunk[4..8].try_into()?);
        let encoded = u32::from_le_bytes(chunk[8..].try_into()?);
        let size = u64::from(blocks) * sparse.block;
        let data = match kind {
            0xcac1 => size,
            0xcac2 | 0xcac4 => 4,
            0xcac3 => 0,
            _ => return Err("Unknown Android sparse chunk".into()),
        };
        if u64::from(encoded) != u64::from(sparse.chunk) + data || written + size > partition.size {
            return Err("Invalid Android sparse chunk extent".into());
        }
        input.seek(SeekFrom::Start(offset + u64::from(sparse.chunk)))?;
        match kind {
            0xcac1 => copy(&mut input, target, size, buffer, None)?,
            0xcac2 => {
                let mut pattern = [0; 4];
                input.read_exact(&mut pattern)?;
                if pattern == [0; 4] {
                    target.seek(SeekFrom::Current(i64::try_from(size)?))?;
                } else {
                    copy(&mut input, target, size, buffer, Some(pattern))?;
                }
            }
            0xcac3 => {
                target.seek(SeekFrom::Current(i64::try_from(size)?))?;
            }
            _ => {}
        }
        offset += u64::from(encoded);
        written += size;
    }
    if written != partition.size {
        return Err("Android sparse partition size differs".into());
    }
    Ok(())
}

pub(super) fn create(parts: &[Partition], raw: &Path, output: &Path, qemu: &Path) -> Result<()> {
    let mut layout = [0; 128 * 128];
    let mut next = MIB / SECTOR;
    let name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Missing Cuttlefish disk name")?;
    let mut starts = Vec::with_capacity(parts.len());
    for (part, entry) in parts.iter().zip(layout.chunks_exact_mut(128)) {
        let size = part.size.div_ceil(SECTOR);
        put(entry, 0, PARTITION_TYPE);
        entry[16..32].copy_from_slice(&Md5::digest(format!("{}:{name}", part.label)));
        put(entry, 32, next.to_le_bytes());
        put(entry, 40, (next + size - 1).to_le_bytes());
        for (at, value) in part.label.encode_utf16().enumerate() {
            put(entry, 56 + 2 * at, value.to_le_bytes());
        }
        starts.push(next * SECTOR);
        next = (next + size).next_multiple_of(MIB / SECTOR);
    }
    let end = next + MIB / SECTOR - 1;
    let mut mbr = [0; 512];
    mbr[450] = 0xee;
    put(&mut mbr, 454, 1_u32.to_le_bytes());
    put(&mut mbr, 458, u32::try_from(end)?.to_le_bytes());
    put(&mut mbr, 510, 0xaa55_u16.to_le_bytes());
    let labels = parts
        .iter()
        .map(|part| part.label.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let identity = Md5::digest(format!("{labels}{end}"));
    let checksum = crc32fast::hash(&layout);
    let header = |mine: u64, other: u64, entries: u64| {
        let mut h = [0; 512];
        put(&mut h, 0, *b"EFI PART");
        put(&mut h, 8, 0x00010000_u32.to_le_bytes());
        put(&mut h, 12, 92_u32.to_le_bytes());
        put(&mut h, 24, mine.to_le_bytes());
        put(&mut h, 32, other.to_le_bytes());
        put(&mut h, 40, 34_u64.to_le_bytes());
        put(&mut h, 48, (end - 33).to_le_bytes());
        h[56..72].copy_from_slice(&identity);
        put(&mut h, 72, entries.to_le_bytes());
        put(&mut h, 80, 128_u32.to_le_bytes());
        put(&mut h, 84, 128_u32.to_le_bytes());
        put(&mut h, 88, checksum.to_le_bytes());
        let checksum = crc32fast::hash(&h[..92]);
        put(&mut h, 16, checksum.to_le_bytes());
        h
    };
    let mut disk = File::create_new(raw)?;
    disk.write_all(&mbr)?;
    disk.write_all(&header(1, end, 2))?;
    disk.write_all(&layout)?;
    let mut buffer = vec![0; 4 * MIB as usize];
    for (part, start) in parts.iter().zip(starts) {
        if let Some(source) = &part.image {
            disk.seek(SeekFrom::Start(start))?;
            copy_partition(source, &mut disk, part, &mut buffer)?;
        }
    }
    disk.seek(SeekFrom::Start((end - 32) * SECTOR))?;
    disk.write_all(&layout)?;
    disk.write_all(&header(end, 1, end - 32))?;
    drop(disk);
    if !Command::new(qemu)
        .args(["convert", "-f", "raw", "-O", "qcow2", "-S", "4k"])
        .arg(raw)
        .arg(output)
        .status()?
        .success()
    {
        return Err("QEMU failed to convert the Cuttlefish disk".into());
    }
    fs::remove_file(raw)?;
    Ok(())
}
