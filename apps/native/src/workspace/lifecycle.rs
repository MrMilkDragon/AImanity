use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Change {
    Disconnect,
    Delete,
}
impl Change {
    pub(super) fn route(route: &str) -> Option<Self> {
        match route {
            "registry/device-disconnect" => Some(Self::Disconnect),
            "registry/device-delete" => Some(Self::Delete),
            _ => None,
        }
    }
    fn pending(self) -> &'static str {
        match self {
            Self::Disconnect => "stopping",
            Self::Delete => "deleting",
        }
    }
}
impl Workspace {
    pub(crate) fn save_volume(&self, id: &str, volume: u8) -> Result<()> {
        self.write_registry(|s, _| {
            if let Some(d) = s.devices.iter_mut().find(|d| d.id == id) {
                d.playback_volume = Some(volume.into());
            }
            Ok(())
        })?;
        Ok(())
    }
    pub(super) fn device_home(&self, id: &str) -> std::path::PathBuf {
        self.files
            .with_file_name("devices")
            .join(hex::encode(id.as_bytes()))
    }
    pub(super) async fn end_device(&self, id: &str, change: Change) -> Result<wire::RegistryState> {
        let (state, closed) = self.peers.remove(id, || {
            self.write_registry(|s, _| {
                let d = s
                    .devices
                    .iter_mut()
                    .find(|d| d.id == id)
                    .ok_or("设备不存在")?;
                if d.runtime_state == "deleting" && change == Change::Disconnect {
                    return Err("设备正在删除".into());
                }
                d.runtime_state = change.pending().into();
                d.start_error.clear();
                d.connected = false;
                d.guest_audio = false;
                Ok(())
            })
        })?;
        #[cfg(any(
            target_os = "macos",
            windows,
            all(target_os = "android", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64")
        ))]
        self.stop_power(Some(id)).await;
        if let Some(closed) = closed {
            closed.cancelled().await;
        }
        self.peers.release_idle_listener().await;
        let device = state
            .devices
            .iter()
            .find(|d| d.id == id)
            .ok_or("设备不存在")?;
        let home = self.device_home(id);
        if home.parent().is_some_and(Path::is_symlink) || home.is_symlink() {
            return Err("设备数据目录不是独立目录".into());
        }
        if !device.paired && home.try_exists()? {
            #[cfg(not(all(target_os = "android", target_arch = "aarch64")))]
            let home = home.canonicalize()?;
            #[cfg(target_os = "macos")]
            {
                if device.kind == "phone" {
                    ai_phone_host_runtime::stop_mac_phone(&home).await?;
                } else {
                    let user = std::env::var_os("HOME")
                        .map(std::path::PathBuf::from)
                        .ok_or("用户目录不可用")?;
                    ai_phone_host_runtime::stop_mac_guest(&home, &user).await?;
                    if change == Change::Delete {
                        ai_phone_host_runtime::remove_mac_guest(&home, &user)?;
                    }
                }
            }
            #[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
            if device.kind == "phone" {
                ai_phone_host_runtime::android::session::stop_owned(&home).await?;
            } else {
                ai_phone_host_runtime::qemu_host::stop_desktop(&home).await?;
            }
            #[cfg(not(any(
                target_os = "macos",
                windows,
                all(target_os = "android", target_arch = "aarch64"),
                all(target_os = "linux", target_arch = "x86_64")
            )))]
            if home.join("user").exists() {
                return Err("该平台尚未接入虚拟机关机".into());
            }
        }
        if change == Change::Delete {
            match tokio::fs::remove_dir_all(&home).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        let result = self.write_registry(|s, tx| {
            let Some(i) = s.devices.iter().position(|d| d.id == id) else {
                return Ok(());
            };
            if s.devices[i].runtime_state == change.pending() {
                match change {
                    Change::Disconnect => s.devices[i].runtime_state = "stopped".into(),
                    Change::Delete => {
                        let prefix = format!("notice/{}/{}/", id.len(), id);
                        tx.open_table(INBOX)?
                            .retain(|key, _| !key.starts_with(&prefix))?;
                        s.devices.remove(i);
                    }
                }
                if s.active_device_id == id {
                    s.active_device_id.clear();
                }
            }
            Ok(())
        })?;
        self.peers.signal();
        Ok(self.peers.available(result))
    }
    pub(super) async fn finish_device_changes(&self) {
        let result: Result<()> = async {
            for d in self.get::<wire::RegistryState>("registry")?.devices {
                let change = match d.runtime_state.as_str() {
                    "deleting" => Change::Delete,
                    "stopping" => Change::Disconnect,
                    _ => continue,
                };
                if let Err(e) = self.end_device(&d.id, change).await {
                    eprintln!("Device {}: {e}", d.id);
                }
            }
            Ok(())
        }
        .await;
        if let Err(e) = result {
            eprintln!("Device registry: {e}");
        }
    }
}
