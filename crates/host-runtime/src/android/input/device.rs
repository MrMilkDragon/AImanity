use crate::{Error, Result};
use std::time::Duration;
use tokio::{io::AsyncReadExt, sync::watch};

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
                    let size = apk.metadata().await?.len();
                    let mut stream = self
                        .client
                        .open_stream(
                            &format!("exec:cmd package install {flags} -S {size} -"),
                            Duration::from_secs(seconds),
                        )
                        .await
                        .map_err(|_| Error::Invalid("phone_input_install_unknown"))?;
                    tokio::io::copy(&mut apk, &mut stream).await?;
                    let mut receipt = Vec::new();
                    stream.take(65537).read_to_end(&mut receipt).await?;
                    if !receipt.trim_ascii().ends_with(b"Success") {
                        return Err(Error::Invalid("phone_input_install_unknown"));
                    }
                    Ok(receipt)
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
