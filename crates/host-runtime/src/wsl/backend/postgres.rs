use super::{Backend, Execute, text};
use crate::{Error, Result, filesystem};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

const BIN: &str = "/opt/aiphone/postgres/lib/postgresql/17/bin";
const LIB: &str = "/opt/aiphone/postgres/runtime";
const STATE: &str = "/var/lib/aiphone-postgres";
const DATA: &str = "/var/lib/aiphone-postgres/data";
const OWNER: &str = "aiphone-postgres";
const LEGACY: &str = "/var/lib/aiphone/docker/volumes/ai-phone_postgres_data/_data/PG_VERSION";

pub(super) fn check_guest() -> Result<()> {
    filesystem::regular(&Path::new(BIN).join("postgres"))?;
    if Path::new(LEGACY).try_exists()? {
        return Err(Error::Invalid("windows_guest_existing_container_database"));
    }
    Ok(())
}

fn owner_command(tool: &str, args: &[&str]) -> Vec<String> {
    [
        "/usr/bin/setpriv".to_owned(),
        format!("--reuid={OWNER}"),
        format!("--regid={OWNER}"),
        "--clear-groups".into(),
        "--no-new-privs".into(),
        "/usr/bin/env".into(),
        format!("LD_LIBRARY_PATH={LIB}"),
        format!("{BIN}/{tool}"),
    ]
    .into_iter()
    .chain(args.iter().map(|arg| (*arg).to_owned()))
    .collect()
}

impl<E: Execute> Backend<'_, E> {
    pub(super) async fn start_postgres(&mut self) -> Result<()> {
        let source = fs::read_to_string(self.root.join(".env"))?;
        let values: BTreeMap<_, _> = source
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim(), value.trim()))
            .collect();
        let value = |key| {
            values
                .get(key)
                .copied()
                .filter(|value| !value.is_empty())
                .ok_or(Error::Invalid("postgres_configuration"))
        };
        let (database, username, password) = (
            value("POSTGRES_DB")?,
            value("POSTGRES_USER")?,
            value("POSTGRES_PASSWORD")?,
        );
        let port = self
            .env
            .entry("AI_PHONE_PG_PORT".into())
            .or_insert_with(|| "55482".into())
            .clone();

        if !Path::new(DATA).try_exists()? {
            self.initialize_postgres(username, password).await?;
        }
        self.unit(
            "postgres",
            "start",
            owner_command(
                "postgres",
                &[
                    "-D",
                    DATA,
                    "-h",
                    "127.0.0.1",
                    "-p",
                    &port,
                    "-k",
                    "",
                    "-c",
                    "jit=off",
                ],
            ),
            &[],
        )
        .await?;
        let connection = [
            "-h",
            "127.0.0.1",
            "-p",
            &port,
            "-U",
            username,
            "-d",
            "postgres",
        ];
        let ready = Instant::now() + Duration::from_secs(15);
        loop {
            let mut command = self.command(format!("{BIN}/pg_isready"), ["-q", "-t", "1"]);
            command.args(connection).env("LD_LIBRARY_PATH", LIB);
            if self
                .execute
                .run(command, Vec::new())
                .await?
                .status
                .success()
            {
                break;
            }
            if Instant::now() >= ready {
                return Err(Error::Invalid("windows_guest_postgres_not_ready"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut command = self.command(format!("{BIN}/psql"), connection);
        command
            .args([
                "-qXw",
                "-v",
                "ON_ERROR_STOP=1",
                "-v",
                &format!("database={database}"),
            ])
            .env("LD_LIBRARY_PATH", LIB)
            .env("PGPASSWORD", password);
        self.checked(
            command,
            b"SELECT format('CREATE DATABASE %I', :'database') WHERE NOT EXISTS \
              (SELECT FROM pg_database WHERE datname = :'database')\n\\gexec\n"
                .to_vec(),
        )
        .await?;
        Ok(())
    }

    async fn initialize_postgres(&mut self, username: &str, password: &str) -> Result<()> {
        let state = Path::new(STATE);
        filesystem::directory(state, false)?;
        filesystem::mode(state, 0o711)?;
        let lookup = self.command("/usr/bin/getent", ["passwd", OWNER]);
        if !self.execute.run(lookup, Vec::new()).await?.status.success() {
            let account = self.command(
                "/usr/sbin/useradd",
                [
                    "--system",
                    "--user-group",
                    "--home-dir",
                    STATE,
                    "--shell",
                    "/usr/sbin/nologin",
                    OWNER,
                ],
            );
            self.checked(account, Vec::new()).await?;
        }
        let staging = tempfile::tempdir_in(state)?;
        let directory = text(staging.path())?;
        let mut credentials = tempfile::NamedTempFile::new_in(state)?;
        writeln!(credentials, "{password}")?;
        let password_file = text(credentials.path())?;
        let command = self.command(
            "/usr/bin/chown",
            [
                format!("{OWNER}:{OWNER}"),
                directory.clone(),
                password_file.clone(),
            ],
        );
        self.checked(command, Vec::new()).await?;
        let args = owner_command(
            "initdb",
            &[
                "-D",
                &directory,
                "-U",
                username,
                "--locale-provider=builtin",
                "--locale=C.UTF-8",
                "--auth=scram-sha-256",
                &format!("--pwfile={password_file}"),
            ],
        );
        let mut command = self.command(&args[0], &args[1..]);
        command.current_dir(state);
        self.checked(command, Vec::new()).await?;
        drop(credentials);
        fs::rename(staging.path(), DATA)?;
        fs::File::open(state)?.sync_all()?;
        Ok(())
    }
}
