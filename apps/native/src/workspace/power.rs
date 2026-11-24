use super::*;
#[cfg(not(target_os = "android"))]
use ai_phone_host_runtime::virtual_device::Instance;
use tokio::sync::oneshot;

pub(super) struct Owner {
    stop: watch::Sender<bool>,
    done: CancellationToken,
}
impl Workspace {
    pub(super) async fn stop_power(&self, id: Option<&str>) {
        let pending: Vec<_> = self
            .power
            .lock()
            .unwrap()
            .iter()
            .filter(|(key, _)| id.is_none_or(|s| s == key.as_str()))
            .map(|(_, owner)| {
                owner.stop.send_replace(true);
                owner.done.clone()
            })
            .collect();
        for done in pending {
            done.cancelled().await;
        }
    }
    pub(super) fn resume_devices(self: &Arc<Self>) {
        let Ok(state) = self.get::<wire::RegistryState>("registry") else {
            return;
        };
        for d in state.devices {
            if !d.paired
                && matches!(d.runtime_state.as_str(), "managed" | "starting" | "booted")
                && let Some(profile) = d.resources
                && let Err(error) = self.launch_device(&d.id, profile)
            {
                eprintln!("Device {}: {error}", d.id);
            }
        }
    }
    pub(super) async fn start_device(
        self: &Arc<Self>,
        q: wire::DeviceStartRequest,
    ) -> Result<wire::RegistryState> {
        let profile = q.resources.ok_or("缺少设备配置")?;
        let before = self.registry()?;
        let d = before
            .devices
            .iter()
            .find(|d| d.id == q.device_id)
            .ok_or("设备不存在")?;
        if d.paired || !matches!(d.kind.as_str(), "phone" | "desktop") {
            return Err("仅自建设备可配置硬件".into());
        }
        let (mem, storage): (&[u32], &[u32]) = if d.kind == "phone" {
            (&[1536, 2048, 3072, 4096], &[16, 24, 32])
        } else {
            (&[2048, 2560, 3072, 4096], &[16, 32, 64])
        };
        if ![1, 2, 4].contains(&profile.cpus)
            || !mem.contains(&profile.memory_mb)
            || !storage.contains(&profile.storage_gb)
        {
            return Err("设备配置无效".into());
        }
        if d.resources.as_ref().is_some_and(|r| {
            if d.kind == "phone" {
                r.storage_gb != profile.storage_gb
            } else {
                r.storage_gb > profile.storage_gb
            }
        }) {
            return Err("已安装的设备磁盘不支持此容量调整".into());
        }
        if d.connected || matches!(d.runtime_state.as_str(), "starting" | "booted") {
            if d.resources.as_ref() == Some(&profile) {
                return Ok(before);
            }
            self.end_device(&d.id, lifecycle::Change::Disconnect)
                .await?;
        }
        self.launch_device(&q.device_id, profile)?;
        self.registry()
    }
    fn launch_device(
        self: &Arc<Self>,
        id: &str,
        profile: wire::VirtualDeviceProfile,
    ) -> Result<()> {
        let mut owners = self.power.lock().unwrap();
        if self.shutdown.is_cancelled() {
            return Err("工作区已关闭".into());
        }
        if owners.contains_key(id) {
            return Ok(());
        }
        let state = self.write_registry(|s, _| {
            let i = s
                .devices
                .iter()
                .position(|d| d.id == id && !d.paired)
                .ok_or("设备不存在")?;
            if matches!(s.devices[i].runtime_state.as_str(), "deleting" | "stopping") {
                return Err("设备正在关闭".into());
            }
            #[cfg(not(target_os = "android"))]
            {
                let kind = &s.devices[i].kind;
                let slot = match s.devices[i].instance_slot {
                    Some(slot) => slot,
                    None => (0..if kind == "phone" { 65 } else { 46000 })
                        .find(|slot| {
                            !s.devices
                                .iter()
                                .any(|d| d.kind == *kind && d.instance_slot == Some(*slot))
                        })
                        .ok_or("设备端口已用尽")?,
                };
                s.devices[i].instance_slot = Some(slot);
            }
            let d = &mut s.devices[i];
            d.resources = Some(profile);
            d.runtime_state = "starting".into();
            d.start_error.clear();
            Ok(())
        })?;
        let d = state
            .devices
            .into_iter()
            .find(|d| d.id == id)
            .ok_or("设备不存在")?;
        #[cfg(not(target_os = "android"))]
        let slot: u16 = d.instance_slot.ok_or("设备端口未分配")?.try_into()?;
        let (end, stop) = watch::channel(false);
        let done = CancellationToken::new();
        owners.insert(
            id.into(),
            Owner {
                stop: end,
                done: done.clone(),
            },
        );
        drop(owners);
        self.peers.signal();
        let w = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let _finished = done.drop_guard();
            let (ready, up) = oneshot::channel();
            let home = w.device_home(&id);
            let mut job = tokio::task::spawn_blocking(move || -> Result<()> {
                let setup = crate::deployment::Setup::load()?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                #[cfg(target_os = "android")]
                runtime.block_on(crate::android_host::run(
                    home,
                    setup.resource,
                    &d.kind,
                    &d.id,
                    profile,
                    setup.language.code(),
                    stop,
                    ready,
                ))?;
                #[cfg(not(target_os = "android"))]
                {
                    let spec = Instance {
                        home,
                        bundle: setup.resource,
                        inventory: crate::deployment::component_versions()?,
                        profile,
                        slot,
                        locale: setup.language.code().into(),
                    };
                    runtime.block_on(spec.run(&d.kind, stop, ready))?;
                }
                Ok(())
            });
            let outcome = tokio::select! {biased;
                ready=up=>{
                    if ready.is_ok(){w.record_power(&id,"booted",String::new());}
                    job.await
                }
                r=&mut job=>r
            };
            let why = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e.to_string()),
                Err(e) => Some(e.to_string()),
            };
            let mut owners = w.power.lock().unwrap();
            if let Some(owner) = owners.remove(&id)
                && !*owner.stop.borrow()
                && !w.shutdown.is_cancelled()
            {
                match why {
                    Some(reason) => w.record_power(&id, "failed", reason),
                    None => w.record_power(&id, "stopped", String::new()),
                }
            }
        });
        Ok(())
    }
    fn record_power(&self, id: &str, state: &str, reason: String) {
        let result = self.write_registry(|s, _| {
            if let Some(d) = s.devices.iter_mut().find(|d| {
                d.id == id && matches!(d.runtime_state.as_str(), "starting" | "booted" | "managed")
            }) {
                d.runtime_state = state.into();
                d.start_error = reason;
            }
            Ok(())
        });
        if let Err(e) = result {
            eprintln!("Device {id}: {e}");
        }
        self.peers.signal();
    }
}
