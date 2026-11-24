use std::{
    fs::File,
    io,
    mem::size_of,
    os::fd::{AsRawFd, FromRawFd},
    thread,
};
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

pub(super) fn connect(cid: u32, port: u32) -> io::Result<File> {
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

pub(super) fn forward(from: File, to: File) {
    thread::scope(|group| {
        group.spawn(|| copy(&from, &to));
        copy(&to, &from);
    });
}
#[cfg(target_os = "linux")]
pub(super) fn guest() -> io::Result<()> {
    let listener = listen(17861)?;
    loop {
        match accept(&listener) {
            Ok((stream, peer)) if peer == libc::VMADDR_CID_HOST => {
                thread::spawn(move || {
                    if let Ok(local) =
                        std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, 17861))
                    {
                        forward(stream, std::os::fd::OwnedFd::from(local).into());
                    }
                });
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}
