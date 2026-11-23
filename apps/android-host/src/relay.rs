//! The two AVF guests are only reachable through ports chosen by the packaged services.
use std::{
    fs::File,
    io,
    mem::size_of,
    net::{Ipv4Addr, TcpListener},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

const CONTROL: u16 = 17861;
const ADB_HOST: u16 = 5555;
#[cfg(target_os = "android")]
const CUTTLEFISH_ADB: u32 = 6520;

fn checked(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

fn socket() -> io::Result<File> {
    let fd = checked(unsafe {
        libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0)
    })?;
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn address(cid: u32, port: u32) -> libc::sockaddr_vm {
    libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as _,
        svm_reserved1: 0,
        svm_cid: cid,
        svm_port: port,
        svm_zero: [0; 4],
    }
}

fn connect(cid: u32, port: u32) -> io::Result<File> {
    let socket = socket()?;
    let address = address(cid, port);
    checked(unsafe {
        libc::connect(
            socket.as_raw_fd(),
            (&raw const address).cast(),
            size_of::<libc::sockaddr_vm>() as _,
        )
    })?;
    Ok(socket)
}

fn listen(port: u16) -> io::Result<File> {
    let socket = socket()?;
    let address = address(libc::VMADDR_CID_ANY, port.into());
    checked(unsafe {
        libc::bind(
            socket.as_raw_fd(),
            (&raw const address).cast(),
            size_of::<libc::sockaddr_vm>() as _,
        )
    })?;
    checked(unsafe { libc::listen(socket.as_raw_fd(), 32) })?;
    Ok(socket)
}

fn accept(listener: &File) -> io::Result<(File, u32)> {
    let mut peer = address(0, 0);
    let mut length = size_of::<libc::sockaddr_vm>() as libc::socklen_t;
    let fd = checked(unsafe {
        libc::accept4(
            listener.as_raw_fd(),
            (&raw mut peer).cast(),
            &mut length,
            libc::SOCK_CLOEXEC,
        )
    })?;
    Ok((unsafe { File::from_raw_fd(fd) }, peer.svm_cid))
}

fn copy(from: &File, to: &File) {
    let _ = io::copy(&mut &*from, &mut &*to);
    unsafe { libc::shutdown(to.as_raw_fd(), libc::SHUT_WR) };
}

#[derive(Clone, Copy)]
enum Target {
    Vsock(u32, u32),
    #[cfg(target_os = "linux")]
    Loopback(u16),
}

impl Target {
    fn connect(self) -> io::Result<File> {
        match self {
            Self::Vsock(cid, port) => connect(cid, port),
            #[cfg(target_os = "linux")]
            Self::Loopback(port) => std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                .map(|s| File::from(OwnedFd::from(s))),
        }
    }
}

fn forward(from: File, target: Target, clients: &Arc<AtomicUsize>) {
    if clients.fetch_add(1, Ordering::Relaxed) >= 64 {
        clients.fetch_sub(1, Ordering::Relaxed);
        return;
    }
    let clients = Arc::clone(clients);
    thread::spawn(move || {
        if let Ok(to) = target.connect() {
            thread::scope(|group| {
                group.spawn(|| copy(&from, &to));
                copy(&to, &from);
            });
        }
        clients.fetch_sub(1, Ordering::Relaxed);
    });
}

fn serve(
    tcp_port: u16,
    tcp_target: Target,
    vsock_port: u16,
    peer_cid: u32,
    vsock_target: Target,
    ended: impl Fn() + Send + Sync + 'static,
) -> io::Result<()> {
    let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, tcp_port))?;
    let vsock = listen(vsock_port)?;
    let ended = Arc::new(ended);
    let tcp_ended = Arc::clone(&ended);
    thread::spawn(move || {
        let clients = Arc::new(AtomicUsize::new(0));
        loop {
            match tcp.accept() {
                Ok((stream, _)) => forward(File::from(OwnedFd::from(stream)), tcp_target, &clients),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        tcp_ended();
    });
    thread::spawn(move || {
        let clients = Arc::new(AtomicUsize::new(0));
        loop {
            match accept(&vsock) {
                Ok((stream, peer)) if peer == peer_cid => forward(stream, vsock_target, &clients),
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        ended();
    });
    Ok(())
}

#[cfg(target_os = "android")]
pub(super) fn start(
    phone: u32,
    linux: u32,
    ended: impl Fn() + Send + Sync + 'static,
) -> io::Result<()> {
    serve(
        CONTROL,
        Target::Vsock(linux, CONTROL.into()),
        ADB_HOST,
        linux,
        Target::Vsock(phone, CUTTLEFISH_ADB),
        ended,
    )
}

#[cfg(target_os = "linux")]
pub(super) fn guest() -> io::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    serve(
        ADB_HOST,
        Target::Vsock(libc::VMADDR_CID_HOST, ADB_HOST.into()),
        CONTROL,
        libc::VMADDR_CID_HOST,
        Target::Loopback(CONTROL),
        move || {
            let _ = tx.send(());
        },
    )?;
    let _ = rx.recv();
    Err(io::Error::other("AVF guest relay stopped"))
}
