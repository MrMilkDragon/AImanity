use crate::relay;
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{
    BigUint, Pkcs1v15Sign, RsaPrivateKey,
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    traits::PublicKeyParts,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::TcpStream,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CNXN: u32 = u32::from_le_bytes(*b"CNXN");
const AUTH: u32 = u32::from_le_bytes(*b"AUTH");
const OPEN: u32 = u32::from_le_bytes(*b"OPEN");
const OKAY: u32 = u32::from_le_bytes(*b"OKAY");
const WRTE: u32 = u32::from_le_bytes(*b"WRTE");
const CLSE: u32 = u32::from_le_bytes(*b"CLSE");
const MAX: usize = 256 * 1024;

#[derive(Clone)]
pub(super) struct Bootstrap {
    pub console_path: PathBuf,
    key: Arc<RsaPrivateKey>,
    public: Arc<str>,
    console: Arc<Mutex<(File, Option<Instant>)>>,
}
impl Bootstrap {
    pub fn open(home: &Path) -> io::Result<Self> {
        let path = home.join("adb.pk8");
        let key = match fs::read(&path) {
            Ok(bytes) => RsaPrivateKey::from_pkcs8_der(&bytes).map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let k =
                    RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).map_err(io::Error::other)?;
                let mut f = tempfile::NamedTempFile::new_in(home)?;
                f.write_all(k.to_pkcs8_der().map_err(io::Error::other)?.as_bytes())?;
                f.as_file().sync_all()?;
                f.persist_noclobber(&path)?;
                k
            }
            Err(e) => return Err(e),
        };
        let n = key.n().to_bytes_le();
        let low = u32::from_le_bytes(n[..4].try_into().unwrap());
        let mut inverse = 1u32;
        for _ in 0..5 {
            inverse = inverse.wrapping_mul(2u32.wrapping_sub(low.wrapping_mul(inverse)));
        }
        let mut public = Vec::with_capacity(524);
        public.extend(64u32.to_le_bytes());
        public.extend(inverse.wrapping_neg().to_le_bytes());
        public.extend(n);
        public.resize(264, 0);
        let rr = (BigUint::from(1u8) << 4096usize) % key.n();
        public.extend(rr.to_bytes_le());
        public.resize(520, 0);
        public.extend(65537u32.to_le_bytes());
        let public: Arc<str> = format!("{} aimanity", STANDARD.encode(public)).into();
        let console_path = home.join("guest-console");
        let c = std::ffi::CString::new(console_path.as_os_str().as_bytes())?;
        if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::AlreadyExists
                || !fs::symlink_metadata(&console_path)?.file_type().is_fifo()
            {
                return Err(e);
            }
        }
        let fifo = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&console_path)?;
        Ok(Self {
            console_path,
            key: Arc::new(key),
            public,
            console: Arc::new(Mutex::new((fifo, None))),
        })
    }

    fn authorize_own_guest(&self) -> io::Result<()> {
        let mut writer = self.console.lock().unwrap();
        if writer
            .1
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
        {
            return Ok(());
        }
        writer.1 = Some(Instant::now());
        // This console belongs to the newly created userdebug guest. Only its own public key is installed.
        let line = format!(
            "su 0 sh -c 'd=/data/misc/adb; test -d /data/system || exit; mkdir -p \"$d\" && chown system:shell \"$d\" && chmod 2750 \"$d\" && restorecon \"$d\"; p=\"$d/adb_keys\"; grep -qxF \"{}\" \"$p\" || {{ echo \"{}\" >> \"$p\" && chown system:shell \"$p\" && chmod 640 \"$p\" && restorecon \"$p\"; }}'\r",
            self.public, self.public
        );
        writer.0.write_all(line.as_bytes())
    }
}

struct Packet {
    cmd: u32,
    a: u32,
    b: u32,
    data: Vec<u8>,
}
fn wire(w: &mut File, cmd: u32, a: u32, b: u32, data: &[u8]) -> io::Result<()> {
    let sum = data.iter().map(|b| u32::from(*b)).sum();
    let mut header = [0u8; 24];
    for (dest, value) in header
        .chunks_exact_mut(4)
        .zip([cmd, a, b, data.len() as u32, sum, !cmd])
    {
        dest.copy_from_slice(&value.to_le_bytes());
    }
    w.write_all(&header)?;
    w.write_all(data)
}
fn packet(r: &mut File) -> io::Result<Packet> {
    let mut header = [[0; 4]; 6];
    r.read_exact(header.as_flattened_mut())?;
    let [cmd, a, b, len, _, magic] = header.map(u32::from_le_bytes);
    let len = len as usize;
    if magic != !cmd || len > MAX {
        return Err(io::Error::other("Invalid ADB frame"));
    }
    let mut data = vec![0; len];
    r.read_exact(&mut data)?;
    Ok(Packet { cmd, a, b, data })
}
fn request(local: &mut TcpStream) -> io::Result<String> {
    let mut head = [0; 4];
    local.read_exact(&mut head)?;
    let n = usize::from_str_radix(std::str::from_utf8(&head).map_err(io::Error::other)?, 16)
        .map_err(io::Error::other)?;
    let mut bytes = vec![0; n];
    local.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(io::Error::other)
}
fn fail(local: &mut TcpStream, why: &str) -> io::Result<()> {
    write!(local, "FAIL{:04x}{why}", why.len())
}
pub(super) fn serve(
    mut local: TcpStream,
    cid: u32,
    keys: &Bootstrap,
    token: &str,
) -> io::Result<()> {
    local.set_read_timeout(Some(Duration::from_secs(8)))?;
    if request(&mut local)? != format!("host:transport:{token}") {
        return fail(&mut local, "Unknown device");
    }
    local.write_all(b"OKAY")?;
    let service = request(&mut local)?;
    let timeout = |socket: &File, seconds: libc::time_t| -> io::Result<()> {
        let value = libc::timeval {
            tv_sec: seconds,
            tv_usec: 0,
        };
        let result = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&value as *const libc::timeval).cast(),
                std::mem::size_of_val(&value) as libc::socklen_t,
            )
        };
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    let open = || -> io::Result<(File, u32, usize)> {
        let mut guest = relay::connect(cid, 5555)?;
        timeout(&guest, 8)?;
        wire(
            &mut guest,
            CNXN,
            0x01000000,
            MAX as u32,
            b"host::features=shell_v2\0",
        )?;
        let mut auth = 0;
        let limit = loop {
            let p = packet(&mut guest)?;
            match p.cmd {
                CNXN => break p.b.clamp(1, MAX as u32) as usize,
                AUTH if p.a == 1 && p.data.len() == 20 && auth < 2 => {
                    if auth == 0 {
                        let sig = keys
                            .key
                            .sign(Pkcs1v15Sign::new::<sha1::Sha1>(), &p.data)
                            .map_err(io::Error::other)?;
                        wire(&mut guest, AUTH, 2, 0, &sig)?;
                    } else {
                        keys.authorize_own_guest()?;
                        let mut bytes = keys.public.as_bytes().to_vec();
                        bytes.push(0);
                        wire(&mut guest, AUTH, 3, 0, &bytes)?;
                    }
                    auth += 1;
                }
                _ => return Err(io::Error::other("Guest ADB authorization pending")),
            }
        };
        let mut name = service.into_bytes();
        name.push(0);
        wire(&mut guest, OPEN, 1, 0, &name)?;
        let p = packet(&mut guest)?;
        if p.cmd != OKAY || p.b != 1 || p.a == 0 {
            return Err(io::Error::other("Guest rejected ADB service"));
        }
        timeout(&guest, 0)?;
        Ok((guest, p.a, limit))
    };
    let (mut guest, remote, limit) = match open() {
        Ok(value) => value,
        Err(e) => return fail(&mut local, &e.to_string()),
    };
    local.write_all(b"OKAY")?;
    local.set_read_timeout(None)?;
    let mut pending = false;
    let mut buf = vec![0; limit.min(65536)];
    loop {
        let mut sockets = [
            libc::pollfd {
                fd: guest.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: local.as_raw_fd(),
                events: if pending { 0 } else { libc::POLLIN },
                revents: 0,
            },
        ];
        if unsafe { libc::poll(sockets.as_mut_ptr(), 2, -1) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if sockets[0].revents != 0 {
            let p = packet(&mut guest)?;
            if p.b != 1 || p.a != remote {
                return Err(io::Error::other("Wrong guest stream"));
            }
            match p.cmd {
                WRTE => {
                    local.write_all(&p.data)?;
                    wire(&mut guest, OKAY, 1, remote, &[])?;
                }
                OKAY => pending = false,
                CLSE => {
                    wire(&mut guest, CLSE, 1, remote, &[])?;
                    break;
                }
                _ => return Err(io::Error::other("Unexpected ADB stream packet")),
            }
        }
        if sockets[1].revents != 0 {
            let n = local.read(&mut buf)?;
            if n == 0 {
                wire(&mut guest, CLSE, 1, remote, &[])?;
                break;
            }
            wire(&mut guest, WRTE, 1, remote, &buf[..n])?;
            pending = true;
        }
    }
    Ok(())
}
