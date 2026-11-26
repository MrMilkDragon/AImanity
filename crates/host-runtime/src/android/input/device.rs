use crate::{Error, Result};
use std::time::Duration;
use tokio::sync::watch;

pub(super) trait Device {
    async fn execute(&mut self, args: &[&str], seconds: u64) -> Result<String>;
    fn cancelled(&self) -> bool;
}
pub(super) struct Adb {
    pub client: ai_phone_adb::Client,
    pub stop: watch::Receiver<bool>,
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
impl Device for Adb {
    fn cancelled(&self) -> bool {
        *self.stop.borrow()
    }
    async fn execute(&mut self, args: &[&str], seconds: u64) -> Result<String> {
        if self.cancelled() {
            return Err(Error::Cancelled);
        }
        let work = async {
            match args {
                ["shell", words @ ..] => {
                    let text = words.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
                    let result = self
                        .client
                        .shell(
                            &text,
                            ai_phone_adb::Limits {
                                stdout: 65536,
                                stderr: 4096,
                            },
                            Duration::from_secs(seconds),
                        )
                        .await
                        .map_err(|_| Error::Invalid("phone_input_command_unknown"))?;
                    if result.code != 0 {
                        return Err(Error::Invalid("phone_input_command_unknown"));
                    }
                    Ok(result.stdout)
                }
                ["install", flags @ .., path] => {
                    let flags = flags
                        .iter()
                        .filter(|v| **v != "--no-incremental")
                        .map(|a| quote(a))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let mut apk = tokio::fs::File::open(path).await?;
                    let deadline = Duration::from_secs(seconds);
                    let remote = "/data/local/tmp/aimanity-offline-install.apk";
                    self.client
                        .push(remote, &mut apk, 0, deadline)
                        .await
                        .map_err(|_| Error::Invalid("phone_input_transfer_failed"))?;
                    let command = format!(
                        "pm install {flags} {remote}; result=$?; rm -f {remote}; exit $result"
                    );
                    let result = self
                        .client
                        .shell(
                            &command,
                            ai_phone_adb::Limits {
                                stdout: 65_536,
                                stderr: 4_096,
                            },
                            deadline,
                        )
                        .await
                        .map_err(|_| Error::Invalid("phone_input_install_unknown"))?;
                    if result.code != 0 {
                        eprintln!(
                            "Android package install: {}",
                            String::from_utf8_lossy(&result.stdout).trim()
                        );
                        return Err(Error::Invalid("phone_input_install_failed"));
                    }
                    Ok(result.stdout)
                }
                _ => Err(Error::Invalid("phone_input_command_unknown")),
            }
        };
        let result = tokio::select! {
            biased;
            _ = self.stop.changed() => return Err(Error::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(seconds),work) => result.map_err(|_|Error::Invalid("phone_input_command_unknown"))??,
        };
        String::from_utf8(result)
            .map(|s| s.trim().to_owned())
            .map_err(|_| Error::Invalid("phone_input_observation"))
    }
}
