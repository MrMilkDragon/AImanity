use super::{Backend, Execute, mac_guest, text};
use crate::{Error, Result, filesystem};
use std::{fs, io::Write, path::Path};

const BIN: &str = "/opt/aiphone/postgres/lib/postgresql/17/bin";
const STATE: &str = "/var/lib/aiphone-postgres";
const DATA: &str = "/var/lib/aiphone-postgres/data";
const OWNER: &str = "aiphone-postgres";
const MAC_BIN: &str = "/usr/libexec/postgresql17/postgres";
#[cfg(target_os = "macos")]
pub(crate) const MAC_POSTGRES_SHUTDOWN: &str = r#"data=/var/lib/aiphone-postgres/data
control=/usr/libexec/postgresql17/pg_ctl
if sudo -n -u postgres -- "$control" -D "$data" status >/dev/null 2>&1; then
    exec sudo -n -u postgres -- "$control" -s -D "$data" -m fast -t 5 -w stop
fi
"#;
const MAC_INIT: &str = r#"set -eu
state=/var/lib/aiphone-postgres
test ! -d "$state/data" || exit 0
install -d -m 0711 "$state"
stage=$(mktemp -d "$state/.init-XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
umask 077
cat > "$stage/password"
chown -R postgres:postgres "$stage"
sudo -n -u postgres -- /usr/libexec/postgresql17/initdb -D "$stage/data" -U "$1" --locale-provider=builtin --locale=C.UTF-8 --auth=scram-sha-256 --pwfile="$stage/password"
mv "$stage/data" "$state/data"
sync -f "$state"
"#;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) fn check_guest() -> Result<()> {
    filesystem::regular(&Path::new(BIN).join("postgres")).map(|_| ())
}

fn owner_command(tool: &str, args: &[&str]) -> Vec<String> {
    [
        "/usr/bin/setpriv".to_owned(),
        format!("--reuid={OWNER}"),
        format!("--regid={OWNER}"),
        "--clear-groups".into(),
        "--no-new-privs".into(),
        format!("{BIN}/{tool}"),
    ]
    .into_iter()
    .chain(args.iter().map(|arg| (*arg).to_owned()))
    .collect()
}

impl<E: Execute> Backend<'_, E> {
    pub(super) async fn start_postgres(&mut self, lima: Option<&str>) -> Result<()> {
        let port = self
            .env
            .entry("AI_PHONE_PG_PORT".into())
            .or_insert_with(|| "55482".into())
            .clone();

        if lima.is_some() || !Path::new(DATA).try_exists()? {
            let source = fs::read_to_string(self.root.join(".env"))?;
            let value = |key| {
                source
                    .lines()
                    .rev()
                    .filter_map(|line| line.split_once('='))
                    .find_map(|(name, value)| (name.trim() == key).then(|| value.trim()))
                    .filter(|value| !value.is_empty())
                    .ok_or(Error::Invalid("postgres_configuration"))
            };
            let (username, password) = (value("POSTGRES_USER")?, value("POSTGRES_PASSWORD")?);
            if let Some(lima) = lima {
                let args = mac_guest(
                    lima,
                    "root",
                    &["/bin/sh", "-c", MAC_INIT, "aiphone-initdb", username],
                );
                let command = self.command(&args[0], &args[1..]);
                if !self
                    .execute
                    .run(command, format!("{password}\n").into_bytes())
                    .await?
                    .status
                    .success()
                {
                    return Err(Error::Invalid("macos_guest_postgres_initialization"));
                }
            } else {
                self.initialize_postgres(username, password).await?;
            }
        }
        let args = [
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
        ];
        let command = if let Some(lima) = lima {
            let mut command = mac_guest(lima, "postgres", &[MAC_BIN]);
            command.extend(args.map(str::to_owned));
            command
        } else {
            owner_command("postgres", &args)
        };
        self.unit("postgres", "start", command, &[]).await?;
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
