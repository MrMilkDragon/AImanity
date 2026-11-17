use super::{Result, read_json};
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::Md5;
use ring::{hmac, pbkdf2};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    num::NonZeroU32,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const ROLES: [&str; 2] = ["ai_phone_runtime", "ai_phone_migrator"];
const MARKER: &str = "AI Phone managed database role v1";
pub(super) fn environment(root: &Path) -> Result<BTreeMap<String, String>> {
    let values: BTreeMap<_, _> = fs::read_to_string(root.join(".env"))?
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().into(), value.trim().into()))
        .collect();
    if ["POSTGRES_DB", "POSTGRES_USER", "POSTGRES_PASSWORD"]
        .iter()
        .any(|key| values.get(*key).is_none_or(String::is_empty))
    {
        return Err("Private PostgreSQL configuration incomplete".into());
    }
    Ok(values)
}
fn identifier(value: &str) -> Result<String> {
    if value.is_empty() || value.contains('\0') {
        return Err("Invalid PostgreSQL identifier".into());
    }
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}
fn literal(value: &str) -> Result<String> {
    if value.contains('\0') {
        return Err("Invalid PostgreSQL value".into());
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}
pub(super) struct Connection {
    host: OsString,
    port: u16,
    admin: String,
    database: String,
    password: String,
    #[cfg(target_os = "macos")]
    guest: Option<Guest>,
}

#[cfg(target_os = "macos")]
struct Guest {
    lima: String,
    home: String,
    passfile: tempfile::NamedTempFile,
}

fn local_tool(tool: &str) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let bundled = Path::new("/opt/aiphone/postgres/lib/postgresql/17/bin").join(tool);
        if bundled.is_file() {
            return bundled;
        }
    }
    PathBuf::from(tool)
}

impl Connection {
    fn new(
        root: &Path,
        host: OsString,
        port: u16,
        admin: Option<&str>,
        database: Option<&str>,
    ) -> Result<Self> {
        let config = environment(root)?;
        let database = database.unwrap_or(&config["POSTGRES_DB"]);
        // libpq treats a database argument containing a URI or key=value as a connection string.
        if database.is_empty()
            || database.contains(['=', '\0'])
            || database.starts_with("postgresql://")
            || database.starts_with("postgres://")
        {
            return Err("Expected a PostgreSQL database name".into());
        }
        Ok(Self {
            host,
            port,
            admin: admin.unwrap_or(&config["POSTGRES_USER"]).to_owned(),
            database: database.to_owned(),
            password: config["POSTGRES_PASSWORD"].clone(),
            #[cfg(target_os = "macos")]
            guest: None,
        })
    }

    pub(super) fn source(root: &Path) -> Result<Self> {
        let port = match std::env::var("AI_PHONE_PG_PORT") {
            Ok(port) => port.parse()?,
            Err(std::env::VarError::NotPresent) => 55482,
            Err(error) => return Err(error.into()),
        };
        let connection = Self::new(root, "127.0.0.1".into(), port, None, None)?;
        #[cfg(target_os = "macos")]
        let connection = {
            let mut connection = connection;
            let runtime = super::environment(root)?;
            if let Some(lima) = runtime.get("AI_PHONE_LIMA_COMMAND") {
                let mut passfile = tempfile::NamedTempFile::new_in(root)?;
                let escape = |field: &str| field.replace('\\', "\\\\").replace(':', "\\:");
                writeln!(
                    passfile,
                    "127.0.0.1:{port}:{}:{}:{}",
                    escape(&connection.database),
                    escape(&connection.admin),
                    escape(&connection.password)
                )?;
                connection.guest = Some(Guest {
                    lima: lima.clone(),
                    home: runtime["LIMA_HOME"].clone(),
                    passfile,
                });
            }
            connection
        };
        Ok(connection)
    }

    pub(super) fn restore_target(
        root: &Path,
        socket: &Path,
        admin: Option<&str>,
        database: &str,
    ) -> Result<Self> {
        let root = root.canonicalize()?;
        let socket = socket.canonicalize()?;
        if !socket.starts_with(&root)
            || socket.as_os_str().as_bytes().contains(&b',')
            || !fs::symlink_metadata(socket.join(".s.PGSQL.5432"))?
                .file_type()
                .is_socket()
        {
            return Err("Expected a dedicated PostgreSQL Unix socket under the target root".into());
        }
        Self::new(&root, socket.into_os_string(), 5432, admin, Some(database))
    }

    fn tool(&self, name: &str) -> Command {
        #[cfg(target_os = "macos")]
        if let Some(guest) = &self.guest {
            let mut passfile = OsString::from("PGPASSFILE=");
            passfile.push(guest.passfile.path());
            let mut command = Command::new(&guest.lima);
            command
                .env("LIMA_HOME", &guest.home)
                .args([
                    "shell",
                    "--tty=false",
                    "--workdir",
                    "/",
                    "ai-phone",
                    "env",
                    "-i",
                    "PGCONNECT_TIMEOUT=5",
                ])
                .arg(passfile)
                .arg(Path::new("/usr/libexec/postgresql17").join(name));
            return command;
        }
        let mut command = Command::new(local_tool(name));
        command
            .env_clear()
            .env(
                "PATH",
                std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
            )
            .env("PGCONNECT_TIMEOUT", "5")
            .env("PGPASSWORD", &self.password);
        command
    }

    fn client(&self, name: &str) -> Command {
        let mut command = self.tool(name);
        command.arg("--host").arg(&self.host).args([
            "--port",
            &self.port.to_string(),
            "--username",
            &self.admin,
            "--dbname",
            &self.database,
            "--no-password",
        ]);
        command
    }

    fn psql(&self) -> Command {
        let mut command = self.client("psql");
        command.args(["-X", "-v", "ON_ERROR_STOP=1"]);
        command
    }

    pub(super) fn dump(&self, output: File) -> Result<()> {
        let status = self
            .client("pg_dump")
            .args(["-Fc", "--no-owner", "--no-acl"])
            .stdout(Stdio::from(output))
            .status()?;
        if !status.success() {
            return Err("PostgreSQL backup failed".into());
        }
        Ok(())
    }

    pub(super) fn sql(&self, sql: &str) -> Result<String> {
        let mut child = self
            .psql()
            .arg("-At")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let written = child
            .stdin
            .take()
            .ok_or("PostgreSQL stdin missing")?
            .write_all(sql.as_bytes());
        let output = child.wait_with_output()?;
        written?;
        if !output.status.success() {
            return Err("PostgreSQL query failed".into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().into())
    }

    pub(super) fn fingerprints(&self) -> Result<Value> {
        let rows = self.sql(
            r#"SELECT format($fingerprint$
    SELECT json_build_array(%L, json_build_object('rows',count(*),'digest',
        md5(COALESCE(string_agg(digest,'' ORDER BY digest COLLATE "C"),''))))
    FROM (SELECT md5(row_to_json(t)::text) AS digest FROM public.%I t) rows;
    $fingerprint$, tablename, tablename)
    FROM pg_tables WHERE schemaname='public' ORDER BY tablename
    \gexec
    "#,
        )?;
        let fingerprints = rows
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<Map<String, Value>, _>>()?;
        Ok(Value::Object(fingerprints))
    }
}

fn password_matches(role: &str, password: &str, verifier: &str) -> Result<bool> {
    if !password.is_ascii() {
        return Ok(false);
    }
    if verifier.starts_with("md5") {
        return Ok(verifier == format!("md5{:x}", Md5::digest(format!("{password}{role}"))));
    }
    let Some(verifier) = verifier.strip_prefix("SCRAM-SHA-256$") else {
        return Ok(false);
    };
    let Some((parameters, keys)) = verifier.split_once('$') else {
        return Ok(false);
    };
    let Some((iterations, salt)) = parameters.split_once(':') else {
        return Ok(false);
    };
    let Some((stored, server)) = keys.split_once(':') else {
        return Ok(false);
    };
    let iterations: u32 = iterations.parse()?;
    if !(1..=1_000_000).contains(&iterations) {
        return Ok(false);
    }
    let salt = STANDARD.decode(salt)?;
    let stored = STANDARD.decode(stored)?;
    let server = STANDARD.decode(server)?;
    if stored.len() != 32 || server.len() != 32 {
        return Ok(false);
    }
    let mut salted = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).ok_or("Invalid SCRAM iteration count")?,
        &salt,
        password.as_bytes(),
        &mut salted,
    );
    let key = hmac::Key::new(hmac::HMAC_SHA256, &salted);
    let client = hmac::sign(&key, b"Client Key");
    let server_key = hmac::sign(&key, b"Server Key");
    Ok(Sha256::digest(client.as_ref())[..] == stored[..]
        && server_key.as_ref() == server.as_slice())
}
fn credentials(root: &Path) -> Result<Value> {
    let value = read_json(&root.join("data/database/credentials.json"))?;
    let env = environment(root)?;
    if value["schema_version"] != 1
        || value["database"] != env["POSTGRES_DB"]
        || value["runtime"]["username"] != ROLES[0]
        || value["migration"]["username"] != ROLES[1]
        || ["runtime", "migration"].iter().any(|role| {
            value[role]["password"]
                .as_str()
                .is_none_or(|password| password.len() < 32)
        })
    {
        return Err("Database credential configuration differs".into());
    }
    Ok(value)
}
fn restore_roles(connection: &Connection, value: &Value) -> Result<BTreeSet<String>> {
    let existing: Map<String, Value> = serde_json::from_str(&connection.sql(
        "SELECT COALESCE(json_object_agg(rolname,rolpassword),'{}'::json) FROM pg_authid WHERE rolname IN ('ai_phone_runtime','ai_phone_migrator');",
    )?)?;
    for kind in ["runtime", "migration"] {
        let login = &value[kind];
        let role = login["username"].as_str().ok_or("Database role missing")?;
        if let Some(verifier) = existing.get(role)
            && !password_matches(
                role,
                login["password"]
                    .as_str()
                    .ok_or("Database password missing")?,
                verifier.as_str().ok_or("Database verifier missing")?,
            )?
        {
            return Err(
                "Existing role credentials differ; use an empty dedicated restore cluster".into(),
            );
        }
    }
    Ok(existing.into_iter().map(|(name, _)| name).collect())
}
fn preparation_sql(value: &Value, existing: &BTreeSet<String>) -> Result<String> {
    let mut output = String::new();
    for kind in ["migration", "runtime"] {
        let login = &value[kind];
        let role = login["username"].as_str().ok_or("Database role missing")?;
        let quoted = identifier(role)?;
        let role_literal = literal(role)?;
        let configure = format!(
            "ALTER ROLE {quoted} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD {}; COMMENT ON ROLE {quoted} IS {};",
            literal(
                login["password"]
                    .as_str()
                    .ok_or("Database password missing")?
            )?,
            literal(MARKER)?
        );
        let (present, missing) = if existing.contains(role) {
            (
                format!(
                    "IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname={role_literal} AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR NOT rolcanlogin)) OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=(SELECT oid FROM pg_roles WHERE rolname={role_literal})) THEN RAISE EXCEPTION 'Unsafe existing database role'; END IF;"
                ),
                "RAISE EXCEPTION 'A database role disappeared after preflight';".into(),
            )
        } else {
            (
                "RAISE EXCEPTION 'A database role appeared after preflight';".into(),
                format!("CREATE ROLE {quoted} LOGIN; {configure}"),
            )
        };
        output.push_str(&format!("DO $role$ BEGIN IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname={role_literal}) THEN IF COALESCE((SELECT shobj_description(oid,'pg_authid') FROM pg_roles WHERE rolname={role_literal}),'') <> {} THEN RAISE EXCEPTION 'Foreign database role'; END IF; {present} ELSE {missing} END IF; END $role$;\n",literal(MARKER)?));
    }
    output.push_str(include_str!("../database/grants.sql"));
    Ok(output)
}
impl Connection {
    pub(super) fn restore(&self, root: &Path, dump: &Path) -> Result<()> {
        if self.sql("SELECT count(*) FROM pg_tables WHERE schemaname='public';")? != "0" {
            return Err("Restore database must be empty".into());
        }
        let value = credentials(root)?;
        let existing = restore_roles(self, &value)?;
        let log = root.join("data/database/restore-error.log");
        let log = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(log)?;
        let mut producer = self
            .tool("pg_restore")
            .args(["--no-owner", "--no-acl", "--file", "-"])
            .stdin(Stdio::from(File::open(dump)?))
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log.try_clone()?))
            .spawn()?;
        let mut consumer = self
            .psql()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()?;
        let result = (|| {
            let mut input = consumer.stdin.take().ok_or("Restore stdin missing")?;
            input.write_all(
                b"BEGIN;\nSET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='60s';\n",
            )?;
            io::copy(
                &mut producer
                    .stdout
                    .take()
                    .ok_or("Database decoder stdout missing")?,
                &mut input,
            )?;
            if !producer.wait()?.success() {
                return Err("Database dump cannot be decoded".into());
            }
            input.write_all(preparation_sql(&value, &existing)?.as_bytes())?;
            input.write_all(b"\nCOMMIT;\n")?;
            drop(input);
            if !consumer.wait()?.success() {
                return Err("Database restore transaction failed".into());
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = producer.kill();
            drop(consumer.stdin.take());
            let _ = consumer.wait();
            let _ = producer.wait();
        }
        result
    }
}
