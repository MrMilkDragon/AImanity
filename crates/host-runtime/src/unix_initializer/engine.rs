//! The bundled, installation-owned Linux guest.
use super::{Backend, Execute, text};
use crate::{Error, Result, filesystem, managed_engine};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn definition(
    home: &Path,
    folder: &Path,
    policy: &Value,
    memory_mb: u32,
    storage_gb: u32,
    native_asset: &Path,
) -> Result<Value> {
    let arch = policy["arch"]
        .as_str()
        .ok_or(Error::Invalid("container_host_policy"))?;
    let image = policy["image"]["sha256"]
        .as_str()
        .ok_or(Error::Invalid("container_host_policy"))?;
    let packages = quote(&text(&folder.join("packages"))?);
    let mounts = [
        json!({"location":home,"writable":true}),
        json!({"location":native_asset,"mountPoint":"/opt/aiphone/bundle","writable":false}),
    ];
    Ok(json!({
        "minimumLimaVersion":"2.2.1", "vmType":"vz", "arch":arch,
        "cpus":4, "memory":format!("{}GiB", crate::desktop_host_memory_mib(memory_mb) / 1024), "disk":format!("{storage_gb}GiB"),
        "images":[{"location":folder.join("image.iso"),"arch":arch,"digest":format!("sha256:{image}")}],
        "mountType":"virtiofs", "mounts":mounts,
        "containerd":{"system":false,"user":false}, "propagateProxyEnv":false,
        "ssh":{"loadDotSSHPubKeys":false,"forwardAgent":false},
        "env":{"HTTP_PROXY":"","HTTPS_PROXY":"","ALL_PROXY":"","http_proxy":"","https_proxy":"","all_proxy":""},
        "provision":[{"mode":"dependency","skipDefaultDependencyResolution":true,"script":"#!/bin/sh\nexit 0\n"},
            {"mode":"system","script":format!("#!/bin/sh\nset -eu\npackages={packages}\napk --no-network add \"$packages\"/*.apk\n")}],
        "probes":[{"script":"#!/bin/sh\nbwrap --version >/dev/null\n","hint":"The local Linux guest must be ready"}],
        "portForwards":[{"guestPortRange":[17860,17869],"ignore":true},
            {"guestPort":55482,"guestIP":"127.0.0.1","hostIP":"127.0.0.1"},
            {"guestPort":56382,"guestIP":"127.0.0.1","hostIP":"127.0.0.1"}]
    }))
}

pub(super) async fn start<E: Execute>(
    backend: &mut Backend<'_, E>,
    home: &Path,
    user: &Path,
    resource: &Path,
) -> Result<()> {
    let root = backend.root;
    let folder = root.join("runtime/container-host");
    let command = folder.join("lima/bin/limactl");
    let state = managed_engine::state_directory(root, home, user)?;
    managed_engine::claim_state(&root.join("data").canonicalize()?, &state)?;
    backend.env.insert("LIMA_HOME".into(), text(&state)?);
    let body = backend
        .checked(
            backend.command(
                &command,
                [
                    "list",
                    "--format",
                    "{{if eq .Name \"ai-phone\"}}{{.Status}}{{end}}",
                ],
            ),
            vec![],
        )
        .await?;
    let status = std::str::from_utf8(&body)
        .map_err(|_| Error::Invalid("container_host_status"))?
        .trim();
    match status {
        "Running" => {}
        "Stopped" => {
            backend
                .checked(
                    backend.command(&command, ["start", "--tty=false", "ai-phone"]),
                    vec![],
                )
                .await?;
        }
        "" => {
            let policies: Value =
                serde_json::from_slice(&fs::read(root.join("config/container-host-runtime.json"))?)
                    .map_err(|_| Error::Invalid("container_host_policy"))?;
            let policy = &policies["hosts"]["macos-arm64"];
            let profile: Value =
                serde_json::from_slice(&fs::read(root.join("data/deployment/profile.json"))?)
                    .map_err(|_| Error::Invalid("host_profile"))?;
            let memory = serde_json::from_value(profile["desktop"]["memory_mb"].clone())
                .map_err(|_| Error::Invalid("host_profile"))?;
            let storage =
                serde_json::from_value::<Option<u32>>(profile["desktop"]["storage_gb"].clone())
                    .map_err(|_| Error::Invalid("host_profile"))?
                    .unwrap_or(64);
            let file = home.join("container-host.yaml");
            filesystem::publish_private(
                home,
                "container-host.yaml",
                &serde_json::to_vec(&definition(
                    home, &folder, policy, memory, storage, resource,
                )?)
                .map_err(|_| Error::Invalid("container_host_policy"))?,
                false,
            )?;
            backend
                .checked(
                    backend.command(
                        &command,
                        [
                            std::ffi::OsStr::new("start"),
                            std::ffi::OsStr::new("--tty=false"),
                            std::ffi::OsStr::new("--name=ai-phone"),
                            file.as_os_str(),
                        ],
                    ),
                    vec![],
                )
                .await?;
        }
        _ => return Err(Error::Invalid("container_host_status")),
    }
    let managed = managed_engine::save(root, user, home, &state, &command)?;
    backend
        .env
        .extend(crate::runtime_environment::resolve_with_engine(
            root, user, managed,
        )?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_definition_keeps_proxy_and_mount_contract() {
        let value = definition(
            Path::new("/home/App Data"),
            Path::new("/home/App Data/releases/id/runtime/container-host"),
            &json!({"arch":"aarch64","image":{"sha256":"abc"}}),
            2560,
            32,
            Path::new("/resources"),
        )
        .unwrap();
        assert_eq!(value["memory"], "5GiB");
        assert_eq!(value["disk"], "32GiB");
        assert_eq!(value["propagateProxyEnv"], false);
        assert_eq!(value["mounts"][0]["location"], "/home/App Data");
        assert!(
            value["provision"][1]["script"]
                .as_str()
                .unwrap()
                .contains("apk --no-network")
        );
        assert_eq!(value["portForwards"][0]["ignore"], true);
    }
}
