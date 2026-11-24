use ai_phone_api::wire;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, io::Write, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

pub const ADDRESS: &str = "http://127.0.0.1:17906";
const CONTROL: &str = "http://127.0.0.1:17907";
const MAX_PROFILE: usize = 2 * 1024 * 1024;

#[derive(Default, Deserialize, Serialize)]
struct Profile {
    url: String,
    enabled: bool,
}
pub struct HarnessProxy {
    directory: PathBuf,
    executable: PathBuf,
    local: reqwest::Client,
    runtime: Mutex<Option<Child>>,
    enabled: std::sync::atomic::AtomicBool,
    pub downloads: reqwest::Client,
}

impl HarnessProxy {
    pub fn new(directory: PathBuf, executable: PathBuf) -> anyhow::Result<Self> {
        let proxy = Self {
            directory,
            executable,
            local: download_client(None)?,
            runtime: Mutex::new(None),
            enabled: false.into(),
            downloads: download_client(Some(ADDRESS))?,
        };
        proxy.set_enabled(proxy.profile()?.enabled);
        Ok(proxy)
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    fn profile(&self) -> anyhow::Result<Profile> {
        match fs::read(self.directory.join("profile.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Profile::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary.write_all(bytes)?;
        temporary.persist(self.directory.join(name))?;
        Ok(())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    fn create_directory(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

pub fn download_client(proxy_url: Option<&str>) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().no_proxy();
    if let Some(url) = proxy_url.filter(|url| !url.is_empty()) {
        builder = builder.proxy(reqwest::Proxy::all(url)?);
    }
    builder
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()
}

async fn controller(
    proxy: &HarnessProxy,
    secret: &str,
    method: reqwest::Method,
    parts: &[&str],
    body: Option<Value>,
) -> anyhow::Result<reqwest::Response> {
    let mut url = reqwest::Url::parse(CONTROL)?;
    url.path_segments_mut()
        .expect("loopback HTTP controller")
        .extend(parts);
    let mut request = proxy.local.request(method, url).bearer_auth(secret);
    if let Some(body) = body {
        request = request.json(&body);
    }
    Ok(request
        .timeout(Duration::from_secs(3))
        .send()
        .await?
        .error_for_status()?)
}

fn subscription_config(bytes: &[u8], secret: &str) -> anyhow::Result<Vec<u8>> {
    use serde_yaml_ng::Value as Yaml;
    let input: Yaml = serde_yaml_ng::from_slice(bytes)?;
    let input = input
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("proxy_profile_format"))?;
    anyhow::ensure!(
        ["proxies", "proxy-providers"]
            .into_iter()
            .any(|name| input.contains_key(Yaml::from(name))),
        "proxy_profile_nodes"
    );
    let mut config = serde_yaml_ng::Mapping::new();
    for name in [
        "proxies",
        "proxy-providers",
        "proxy-groups",
        "rules",
        "rule-providers",
        "sub-rules",
    ] {
        if let Some(value) = input.get(Yaml::from(name)) {
            config.insert(Yaml::from(name), value.clone());
        }
    }
    for key in ["proxy-providers", "rule-providers"] {
        if let Some(providers) = config
            .get_mut(Yaml::from(key))
            .and_then(Yaml::as_mapping_mut)
        {
            for (index, value) in providers.values_mut().enumerate() {
                let provider = value
                    .as_mapping_mut()
                    .ok_or_else(|| anyhow::anyhow!("proxy_profile_format"))?;
                anyhow::ensure!(
                    provider.get(Yaml::from("type")).and_then(Yaml::as_str) != Some("file"),
                    "proxy_profile_format"
                );
                provider.insert(
                    Yaml::from("path"),
                    Yaml::from(format!("./providers/{key}-{index}.yaml")),
                );
            }
        }
    }
    config.insert(Yaml::from("mode"), Yaml::from("rule"));
    config.insert(Yaml::from("mixed-port"), Yaml::from(17906));
    config.insert(Yaml::from("allow-lan"), Yaml::from(false));
    if std::env::var("AI_PHONE_ANDROID_QEMU").as_deref() == Ok("1") {
        config.insert(Yaml::from("listeners"), serde_yaml_ng::to_value(serde_json::json!([{
            "name": "managed-guests", "type": "mixed", "listen": "10.0.2.15", "port": 17906, "udp": false,
        }]))?);
    }
    config.insert(
        Yaml::from("external-controller"),
        Yaml::from("127.0.0.1:17907"),
    );
    config.insert(Yaml::from("secret"), Yaml::from(secret));
    config.insert(
        Yaml::from("profile"),
        serde_yaml_ng::to_value(serde_json::json!({"store-selected":true}))?,
    );
    Ok(serde_yaml_ng::to_string(&config)?.into_bytes())
}

async fn fetch_subscription(url: &str) -> anyhow::Result<Vec<u8>> {
    let url = reqwest::Url::parse(url)?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none(),
        "proxy_subscription_url"
    );
    let mut response = reqwest::Client::builder()
        .no_proxy()
        .user_agent("mihomo/1.19.33")
        .timeout(Duration::from_secs(20))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= MAX_PROFILE,
            "proxy_profile_size"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn start(
    proxy: &HarnessProxy,
    runtime: &mut Option<Child>,
    secret: &str,
) -> anyhow::Result<()> {
    if let Some(child) = runtime
        && child.try_wait()?.is_none()
    {
        return Ok(());
    }
    let mut child = proxy
        .command()
        .args(["-d"])
        .arg(&proxy.directory)
        .args(["-f"])
        .arg(proxy.directory.join("config.yaml"))
        .spawn()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        if controller(proxy, secret, reqwest::Method::GET, &["version"], None)
            .await
            .is_ok()
        {
            *runtime = Some(child);
            return Ok(());
        }
        anyhow::ensure!(
            child.try_wait()?.is_none() && tokio::time::Instant::now() < deadline,
            "proxy_start_failed"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn configure(
    proxy: &HarnessProxy,
    runtime: &mut Option<Child>,
    bytes: &[u8],
    secret: &str,
) -> anyhow::Result<()> {
    let mut candidate = tempfile::NamedTempFile::new_in(&proxy.directory)?;
    candidate.write_all(bytes)?;
    let status = proxy
        .command()
        .args(["-t", "-d"])
        .arg(&proxy.directory)
        .arg("-f")
        .arg(candidate.path())
        .status()
        .await?;
    anyhow::ensure!(status.success(), "proxy_profile_invalid");
    if let Some(child) = runtime
        && child.try_wait()?.is_none()
    {
        controller(
            proxy,
            secret,
            reqwest::Method::PUT,
            &["configs"],
            Some(serde_json::json!({
                "path": candidate.path(),
            })),
        )
        .await?;
    }
    candidate.persist(proxy.directory.join("config.yaml"))?;
    Ok(())
}

async fn snapshot(
    proxy: &HarnessProxy,
    profile: &Profile,
    runtime: &mut Option<Child>,
    secret: &str,
) -> anyhow::Result<wire::HarnessProxyState> {
    let mut groups = Vec::new();
    if !profile.url.is_empty() {
        start(proxy, runtime, secret).await?;
        let value: Value = controller(proxy, secret, reqwest::Method::GET, &["proxies"], None)
            .await?
            .json()
            .await?;
        if let Some(proxies) = value["proxies"].as_object() {
            for (name, group) in proxies {
                if group["type"] != "Selector" || name == "GLOBAL" {
                    continue;
                }
                if let (Some(now), Some(all)) = (group["now"].as_str(), group["all"].as_array()) {
                    groups.push(wire::HarnessProxyGroup {
                        name: name.clone(),
                        selected: now.into(),
                        nodes: all
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                    });
                }
            }
        }
    }
    Ok(wire::HarnessProxyState {
        groups,
        enabled: profile.enabled,
        has_subscription: !profile.url.is_empty(),
    })
}

fn controller_secret(proxy: &HarnessProxy) -> anyhow::Result<String> {
    match fs::read_to_string(proxy.directory.join("controller.key")) {
        Ok(secret) => Ok(secret),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("proxy_controller_key"))?;
            let secret = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            proxy.save("controller.key", secret.as_bytes())?;
            Ok(secret)
        }
        Err(error) => Err(error.into()),
    }
}
impl HarnessProxy {
    pub async fn restore(&self) -> anyhow::Result<bool> {
        let mut runtime = self.runtime.lock().await;
        if !self.enabled() {
            return Ok(false);
        }
        start(self, &mut runtime, &controller_secret(self)?).await?;
        Ok(true)
    }
    pub async fn apply_enabled<F>(&self, apply: F) -> anyhow::Result<()>
    where
        F: std::future::Future<Output = anyhow::Result<()>>,
    {
        let _runtime = self.runtime.lock().await;
        if self.enabled() {
            apply.await?;
        }
        Ok(())
    }
    pub async fn stop(&self) {
        if let Some(mut core) = self.runtime.lock().await.take() {
            let _ = core.kill().await;
        }
    }
    pub async fn request<F, Fut>(
        &self,
        operation: &str,
        input: wire::HarnessProxyRequest,
        apply: F,
    ) -> anyhow::Result<wire::HarnessProxyState>
    where
        F: Fn(bool) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        let mut runtime = self.runtime.lock().await;
        let mut profile = self.profile()?;
        if operation == "state" && profile.url.is_empty() {
            return Ok(wire::HarnessProxyState::default());
        }
        self.create_directory()?;
        let secret = controller_secret(self)?;
        match operation {
            "import" | "refresh" => {
                let url = if operation == "import" {
                    &input.subscription_url
                } else {
                    &profile.url
                };
                let bytes = fetch_subscription(url.trim()).await?;
                configure(
                    self,
                    &mut runtime,
                    &subscription_config(&bytes, &secret)?,
                    &secret,
                )
                .await?;
                start(self, &mut runtime, &secret).await?;
                profile.url = url.trim().into();
            }
            "toggle" => {
                anyhow::ensure!(!profile.url.is_empty(), "proxy_subscription_missing");
                if input.enabled {
                    start(self, &mut runtime, &secret).await?;
                }
                if input.enabled != profile.enabled
                    && let Err(error) = apply(input.enabled).await
                {
                    if let Err(rollback) = apply(profile.enabled).await {
                        eprintln!(
                            "Workbench proxy device settings could not be restored: {rollback}"
                        );
                    }
                    return Err(error);
                }
                profile.enabled = input.enabled;
            }
            "select" => {
                anyhow::ensure!(!profile.url.is_empty(), "proxy_subscription_missing");
                start(self, &mut runtime, &secret).await?;
                controller(
                    self,
                    &secret,
                    reqwest::Method::PUT,
                    &["proxies", &input.group],
                    Some(serde_json::json!({"name":input.node})),
                )
                .await?;
            }
            "state" => {}
            _ => anyhow::bail!("Unknown proxy operation"),
        }
        if matches!(operation, "import" | "refresh" | "toggle") {
            self.save("profile.json", &serde_json::to_vec(&profile)?)?;
        }
        let state = snapshot(self, &profile, &mut runtime, &secret).await?;
        self.set_enabled(state.enabled);
        if !state.enabled
            && let Some(mut core) = runtime.take()
        {
            core.kill().await?;
        }
        Ok(state)
    }
}
