use std::{
    env, fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
#[cfg(target_os = "android")]
mod adb;
#[cfg(target_os = "android")]
mod relay;
const AVF: &str = "/apex/com.android.virt/bin/vm";
struct Machine(Child);
impl Drop for Machine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn started_cid(receipt: &str) -> Option<u32> {
    let details = receipt
        .strip_prefix("Created VM from ")
        .or_else(|| receipt.strip_prefix("Created debuggable VM from "))?;
    let (_, status) = details.rsplit_once(" with CID ")?;
    let (cid, state) = status.split_once(", state is ")?;
    if !matches!(state.trim_end(), "STARTING." | "STARTED." | "READY.") {
        return None;
    }
    cid.parse().ok().filter(|cid| (3..u32::MAX).contains(cid))
}

#[cfg(target_os = "android")]
fn supervise(kind: &str, config: &Path, name: &str) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let dir = config
        .parent()
        .ok_or_else(|| io::Error::other("Missing AVF state"))?;
    let phone = match kind {
        "phone" => true,
        "desktop" => false,
        _ => return Err(io::Error::other("Unknown AVF system")),
    };
    let token: [u8; 32] = fs::read(dir.join("control.key"))?
        .try_into()
        .map_err(|_| io::Error::other("Invalid instance key"))?;
    let credential: String = token.iter().map(|b| format!("{b:02x}")).collect();
    let bootstrap = if phone {
        Some(adb::Bootstrap::open(dir)?)
    } else {
        None
    };
    let profile: serde_json::Value = serde_json::from_reader(fs::File::open(config)?)?;
    let topology = profile["cpu_topology"]
        .as_str()
        .ok_or_else(|| io::Error::other("Missing CPU topology"))?;
    let mut cmd = Command::new(AVF);
    cmd.args([
        "run",
        "--cpu-topology",
        topology,
        "--name",
        name,
        "--console",
        "/dev/null",
        "--console-in",
    ])
    .arg(
        bootstrap
            .as_ref()
            .map_or(Path::new("/dev/null"), |b| b.console_path.as_path()),
    )
    .args(["--log", "/dev/null"])
    .arg(config)
    .stdin(Stdio::null())
    .stdout(Stdio::piped());
    let parent = unsafe { libc::getpid() };
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::ErrorKind::Interrupted.into());
            }
            Ok(())
        });
    }
    let mut vm = Machine(cmd.spawn()?);
    enum Event {
        Cid(u32),
        Stop,
        Exit,
    }
    let (send, events) = mpsc::channel();
    let terminal = send.clone();
    thread::spawn(move || {
        let _ = io::stdin().read(&mut [0]);
        let _ = terminal.send(Event::Stop);
    });
    let mut output = BufReader::new(vm.0.stdout.take().unwrap());
    thread::spawn(move || {
        let mut line = String::new();
        while output.read_line(&mut line).is_ok_and(|n| n > 0) {
            if let Some(cid) = started_cid(&line) {
                let _ = send.send(Event::Cid(cid));
                let _ = io::copy(&mut output, &mut io::sink());
                break;
            }
            line.clear();
        }
        let _ = send.send(Event::Exit);
    });
    let cid = match events
        .recv_timeout(Duration::from_secs(90))
        .map_err(io::Error::other)?
    {
        Event::Cid(cid) => cid,
        Event::Stop => return Ok(()),
        Event::Exit => return Err(io::Error::other("AVF guest exited at startup")),
    };
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    thread::spawn(move || {
        for socket in listener.incoming().map_while(Result::ok) {
            if let Some(keys) = bootstrap.as_ref() {
                let keys = keys.clone();
                let token = credential.clone();
                thread::spawn(move || {
                    let _ = adb::serve(socket, cid, &keys, &token);
                });
            } else {
                thread::spawn(move || {
                    if let Ok(peer) = relay::connect(cid, 17861) {
                        relay::forward(std::os::fd::OwnedFd::from(socket).into(), peer);
                    }
                });
            }
        }
    });
    writeln!(io::stdout().lock(), "READY {cid} {port}")?;
    io::stdout().flush()?;
    match events.recv() {
        Ok(Event::Stop) => Ok(()),
        _ => Err(io::Error::other("AVF guest exited")),
    }
}

fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [kind, config, name] = args.as_slice() else {
        return Err(io::Error::other(
            "usage: aimanity-android-host KIND CONFIG NAME",
        ));
    };
    #[cfg(target_os = "android")]
    return supervise(
        kind.to_str().ok_or(io::ErrorKind::InvalidInput)?,
        &PathBuf::from(config).canonicalize()?,
        name.to_str().ok_or(io::ErrorKind::InvalidInput)?,
    );
    #[cfg(not(target_os = "android"))]
    {
        let _ = (kind, config, name);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "AVF requires Android",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_cid_receipt_only_identifies_a_running_vm_session() {
        assert_eq!(
            started_cid("Created VM from cfg with CID 19, state is STARTING."),
            Some(19)
        );
        assert!(started_cid("Created VM from cfg with CID 2, state is READY.").is_none());
        assert!(started_cid("Created VM from cfg with CID 9, state is DEAD.").is_none());
        assert!(started_cid("vm is ready").is_none());
    }
}
