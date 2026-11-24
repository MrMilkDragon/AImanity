use super::*;
use std::{
    cell::RefCell,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Fixture {
    calls: Vec<(String, Vec<String>, Vec<u8>)>,
    trace: Rc<RefCell<Vec<String>>>,
}
impl Execute for Fixture {
    async fn run(&mut self, command: Command, input: Vec<u8>) -> Result<std::process::Output> {
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .into_owned();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        self.trace.borrow_mut().push(args[0].clone());
        let bytes = match args[0].as_str() {
            "--stage-source" => {
                let root = Path::new(&args[2]).join("releases").join(&args[3]);
                fs::create_dir_all(root.join("data")).unwrap();
                format!("{} {}\n", args[3], root.canonicalize().unwrap().display()).into_bytes()
            }
            "--service-control" => b"true".to_vec(),
            _ => Vec::new(),
        };
        self.calls.push((program, args, input));
        Ok(std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: bytes,
            stderr: Vec::new(),
        })
    }
}

const REQUEST: &[u8] = br#"{"locale":"en","phone":{"cpus":2,"memory_mb":2048,"storage_gb":16},"desktop":{"cpus":2,"memory_mb":2560}}"#;
struct Init {
    _folder: tempfile::TempDir,
    bundle: PathBuf,
    home: PathBuf,
    root: PathBuf,
}
impl Init {
    fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let bundle = folder.path().join("bundle");
        fs::create_dir(&bundle).unwrap();
        fs::write(bundle.join("source.sha256"), "a".repeat(64)).unwrap();
        let home = folder.path().join("user/installation");
        let root = home.join("releases").join("a".repeat(64));
        Self {
            _folder: folder,
            bundle,
            home,
            root,
        }
    }
    async fn initialize(&self, fixture: Fixture, free: u64) -> Result<()> {
        initialize(
            &self.bundle,
            &self.home,
            profile::parse_request(REQUEST).unwrap(),
            fixture,
            &|_| Ok(free),
        )
        .await
    }
}

#[test]
fn linux_guest_ignores_host_lima_environment() {
    let env = environment([
        ("LIMA_HOME".into(), "outside".into()),
        ("AI_PHONE_LIMA_COMMAND".into(), "outside".into()),
    ]);
    assert_eq!(env, BTreeMap::from([("PATH".into(), PATH.into())]));
}

#[tokio::test]
async fn initialization_lock_and_low_space_never_start_source_or_services() {
    let init = Init::new();
    let fixture = Fixture::default();
    let trace = fixture.trace.clone();
    assert_eq!(
        init.initialize(fixture, 4 * 1024 * 1024 * 1024 - 1)
            .await
            .unwrap_err()
            .code(),
        "source_resource_space"
    );
    assert!(trace.borrow().is_empty());
    assert!(!init.root.exists());
    let parent = init.home.parent().unwrap();
    let lock = filesystem::lease(&parent.join("initialize.lock"))
        .unwrap()
        .unwrap();
    let fixture = Fixture::default();
    let trace = fixture.trace.clone();
    assert_eq!(
        init.initialize(fixture, u64::MAX).await.unwrap_err().code(),
        "installation_running"
    );
    assert!(trace.borrow().is_empty());
    drop(lock);
}

#[tokio::test]
async fn quarantined_and_lima_owned_state_stop_before_engine_or_profile_changes() {
    for (name, code) in [
        ("data/restore-quarantine.json", "restored_state_quarantined"),
        (
            "data/deployment/container-host.json",
            "foreign_installation",
        ),
    ] {
        let init = Init::new();
        let marker = init.root.join(name);
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(&marker, b"keep-owned-state").unwrap();
        let fixture = Fixture::default();
        let trace = fixture.trace.clone();
        assert_eq!(
            init.initialize(fixture, u64::MAX).await.unwrap_err().code(),
            code
        );
        assert_eq!(*trace.borrow(), ["--stage-source"]);
        assert_eq!(fs::read(marker).unwrap(), b"keep-owned-state");
        assert!(!init.root.join("data/deployment/profile.json").exists());
    }
}

#[tokio::test]
async fn restarting_device_services_keeps_existing_profile_and_tokens() {
    let init = Init::new();
    fs::create_dir_all(init.root.join("data")).unwrap();
    profile::prepare(
        &init.root,
        Path::new("/existing/sdk"),
        "android35-x86_64",
        REQUEST,
        "/existing/path",
    )
    .unwrap();
    for name in ["data/assistant/token", "data/phone-runner/token"] {
        let path = init.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"keep-existing").unwrap();
    }
    let paths = [
        "data/deployment/profile.json",
        "data/assistant/token",
        "data/phone-runner/token",
    ];
    let before = paths.map(|path| fs::read(init.root.join(path)).unwrap());
    let running = Fixture::default();
    let trace = running.trace.clone();
    let mut backend = fixture(&init.root, running);
    backend.start(Path::new("/root")).await.unwrap();
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(fs::read(init.root.join(path)).unwrap(), bytes);
    }
    assert!(
        !trace
            .borrow()
            .iter()
            .any(|step| step == "--prepare-database" || step == "--migrate")
    );
}

fn fixture(root: &Path, execute: Fixture) -> Backend<'_, Fixture> {
    fs::create_dir_all(root.join("data")).unwrap();
    profile::prepare(
        root,
        Path::new("/opt/aiphone/native-phone-bridge"),
        "android35-x86_64",
        REQUEST,
        PATH,
    )
    .unwrap();
    Backend {
        root,
        program: OsStr::new("native-bootstrap"),
        env: BTreeMap::from([("AI_PHONE_LIMA_COMMAND".into(), "limactl".into())]),
        execute,
    }
}

#[tokio::test]
async fn guest_starts_only_desktop_runner_and_media() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().canonicalize().unwrap();
    let mut backend = fixture(&root, Fixture::default());
    crate::desktop_bridge::configure(&root, 2, 2560).unwrap();
    backend.start(Path::new("/root")).await.unwrap();
    let calls = &backend.execute.calls;
    let units: Vec<_> = calls
        .iter()
        .filter(|(_, args, _)| args[0] == "--service-control" && args[3] == "start")
        .map(|(_, args, _)| args[2].as_str())
        .collect();
    assert_eq!(units, ["desktop", "runner", "media"]);
    let (_, _, config) = calls
        .iter()
        .find(|(_, args, _)| {
            args[0] == "--service-control" && args[2] == "desktop" && args[3] == "start"
        })
        .unwrap();
    let spec: Value = serde_json::from_slice(config).unwrap();
    assert_eq!(spec["command"][10], "native-linux");
    assert_eq!(spec["command"][12], "desktop");
    assert!(
        !calls
            .iter()
            .any(|(program, args, _)| program == "docker" || args[0] == "--activate-images")
    );
}

#[tokio::test]
async fn cancelled_process_does_not_spawn() {
    let (_, stop) = watch::channel(true);
    let mut process = Process(stop);
    assert_eq!(
        process
            .run(Command::new("this-program-must-not-run"), Vec::new())
            .await
            .unwrap_err()
            .code(),
        "installation_cancelled"
    );
}

#[tokio::test]
async fn real_process_transports_unicode_input_and_closes_stdin() {
    let (_send, stop) = watch::channel(false);
    let mut process = Process(stop);
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "/bin/cat"]);
    let output = process
        .run(command, "真实输入\n".as_bytes().to_vec())
        .await
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, "真实输入\n".as_bytes());
}

#[tokio::test]
async fn real_cancel_reaps_the_owned_command() {
    let folder = tempfile::tempdir().unwrap();
    let pid_file = folder.path().join("child.pid");
    let (send, stop) = watch::channel(false);
    let mut process = Process(stop);
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "echo $$ > \"$1.tmp\"; /bin/mv \"$1.tmp\" \"$1\"; exec /bin/sleep 60",
            "fixture",
        ])
        .arg(&pid_file);
    let marker = pid_file.clone();
    let cancellation = tokio::spawn(async move {
        let until = Instant::now() + Duration::from_secs(5);
        while !marker.is_file() {
            assert!(Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let pid = rustix::process::Pid::from_raw(
            fs::read_to_string(&marker).unwrap().trim().parse().unwrap(),
        )
        .unwrap();
        rustix::process::test_kill_process(pid).unwrap();
        send.send_replace(true);
        pid
    });
    assert_eq!(
        process.run(command, Vec::new()).await.unwrap_err().code(),
        "installation_cancelled"
    );
    let pid = cancellation.await.unwrap();
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH),
        "command remained alive after cancellation"
    );
}

#[tokio::test]
async fn output_overflow_closes_and_reaps_the_command_without_waiting_for_cancellation() {
    let (_send, stop) = watch::channel(false);
    let mut process = Process(stop);
    let mut command = Command::new("/bin/sh");
    command.args(["-c","while :; do printf '012345678901234567890123456789012345678901234567890123456789012345'; done"]);
    let started = Instant::now();
    let error = process.run(command, Vec::new()).await.unwrap_err();
    assert!(matches!(error, Error::Io(_)));
    assert!(started.elapsed() < Duration::from_secs(5));
}
