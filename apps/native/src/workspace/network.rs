use super::*;
use aimanity_workbench_proxy::HarnessProxy;

impl Workspace {
    pub(super) fn network_proxy(&self) -> Result<&HarnessProxy> {
        if let Some(core) = self.proxy.get() { return Ok(core); }
        #[cfg(target_os="android")]
        let executable = crate::deployment::android_assets::helper()?.with_file_name("libaimanity_proxy.so");
        #[cfg(not(target_os="android"))]
        let executable = crate::deployment::Setup::resource_dir()?.join(if cfg!(windows) { "mihomo.exe" } else { "mihomo" });
        let core = HarnessProxy::new(self.files.with_file_name("proxy"), executable)?;
        Ok(self.proxy.get_or_init(|| core))
    }

    pub(super) async fn proxy_request(&self, operation: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
        let request = wire::HarnessProxyRequest::decode(bytes.as_slice())?;
        Ok(self.network_proxy()?.request(operation, request, |_| async { Ok(()) }).await?.encode_to_vec())
    }
}
