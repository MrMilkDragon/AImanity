use std::{
    env,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "android")]
mod relay;

const AVF: &str = "/apex/com.android.virt/bin/vm";
const GUESTS: [&str; 2] = ["android", "linux"];

enum Event {
    Started(usize, u32),
    Ended(usize),
    #[cfg(target_os = "android")]
    RelayEnded,
    Stop,
}

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

fn start(vm: &Path, config: &Path, slot: usize, events: Sender<Event>) -> io::Result<Machine> {
    let profile: serde_json::Value = serde_json::from_reader(std::fs::File::open(config)?)?;
    let topology = match profile["cpu_topology"].as_str() {
        Some("match_host") => "match_host",
        Some("one_cpu") | None => "one_cpu",
        _ => return Err(io::Error::other("Invalid AVF CPU topology")),
    };
    let mut command = Command::new(vm);
    command
        .args([
            "run",
            "--cpu-topology",
            topology,
            "--name",
            GUESTS[slot],
            "--console",
            "/dev/null",
            "--console-in",
            "/dev/null",
            "--log",
            "/dev/null",
        ])
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    #[cfg(target_os = "android")]
    {
        use std::os::unix::process::CommandExt;
        let owner = unsafe { libc::getpid() };
        // Dropping the vm client releases its AVF Binder handle, including on owner death.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != owner {
                    return Err(io::Error::from_raw_os_error(libc::ESRCH));
                }
                Ok(())
            });
        }
    }
    let mut machine = Machine(command.spawn()?);
    let mut output = BufReader::new(machine.0.stdout.take().unwrap());
    thread::spawn(move || {
        let mut line = String::new();
        if output.read_line(&mut line).is_ok()
            && let Some(cid) = started_cid(&line)
            && events.send(Event::Started(slot, cid)).is_ok()
        {
            let _ = io::copy(&mut output, &mut io::sink());
        }
        let _ = events.send(Event::Ended(slot));
    });
    Ok(machine)
}

fn run(
    vm: &Path,
    configs: [PathBuf; 2],
    mut owner: impl Read + Send + 'static,
    report: impl FnOnce([u32; 2]) -> io::Result<()>,
) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let _phone = start(vm, &configs[0], 0, tx.clone())?;
    let _linux = start(vm, &configs[1], 1, tx.clone())?;
    let owner_events = tx.clone();
    thread::spawn(move || {
        let _ = owner.read(&mut [0]);
        let _ = owner_events.send(Event::Stop);
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut cids = [None; 2];
    let mut report = Some(report);
    loop {
        let event = if report.is_some() {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "AVF startup receipt timed out")
                })?
        } else {
            rx.recv().map_err(io::Error::other)?
        };
        match event {
            Event::Stop if report.is_some() => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "owner closed before both guests started",
                ));
            }
            Event::Stop => return Ok(()),
            #[cfg(target_os = "android")]
            Event::RelayEnded => return Err(io::Error::other("AVF relay listener closed")),
            Event::Ended(slot) => {
                return Err(io::Error::other(format!(
                    "{} AVF session ended",
                    GUESTS[slot]
                )));
            }
            Event::Started(slot, cid) => cids[slot] = Some(cid),
        }
        if let [Some(phone), Some(linux)] = cids {
            if phone == linux {
                return Err(io::Error::other(
                    "AVF returned the same CID for both guests",
                ));
            }
            if let Some(started) = report.take() {
                #[cfg(target_os = "android")]
                {
                    let events = tx.clone();
                    relay::start(phone, linux, move || {
                        let _ = events.send(Event::RelayEnded);
                    })?;
                }
                started([phone, linux])?;
            }
        }
    }
}

fn main() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    let (Some(phone), Some(linux), None) = (args.next(), args.next(), args.next()) else {
        return Err(io::Error::other(
            "usage: aimanity-android-host ANDROID_VM.json LINUX_VM.json",
        ));
    };
    let configs = [std::fs::canonicalize(phone)?, std::fs::canonicalize(linux)?];
    if configs[0] == configs[1] {
        return Err(io::Error::other(
            "Android and Linux require separate AVF configurations",
        ));
    }
    run(Path::new(AVF), configs, io::stdin(), |[phone, linux]| {
        writeln!(
            io::stdout().lock(),
            "Android AVF CID={phone} Linux AVF CID={linux}"
        )
    })
}
