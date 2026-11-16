use futures_util::SinkExt;
use serde::Deserialize;
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::oneshot,
};

#[derive(Clone, Deserialize)]
pub struct Host {
    pub home: PathBuf,
    pub resource: PathBuf,
    request: PathBuf,
    source: String,
    android: String,
    images: String,
}

#[derive(Clone, Debug)]
pub enum Event {
    Phase(String),
    Ready(String),
    Failed(String),
    Stopped(Result<(), String>),
}

impl Host {
    pub fn load(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    pub fn events(
        self,
        mut stop: oneshot::Receiver<()>,
    ) -> impl futures_util::Stream<Item = Event> {
        iced::stream::channel(8, async move |mut output| {
            let result: Result<bool, String> = async {
                let mut child = Command::new(std::env::current_exe().map_err(|e|e.to_string())?)
                    .arg("--initialize-unix-host")
                    .arg(&self.resource).arg(&self.home).arg(&self.request)
                    .arg(&self.source).arg(&self.android).arg(&self.images).arg("--resume").arg("--native-ui")
                    .stdout(Stdio::piped()).spawn().map_err(|e|e.to_string())?;
                let mut lines=BufReader::new(child.stdout.take().unwrap()).lines();
                loop {
                    tokio::select! {
                        _ = &mut stop => {
                            let status=Command::new("/bin/kill").arg("-TERM").arg(child.id().unwrap().to_string()).status().await.map_err(|e|e.to_string())?;
                            if !status.success() { return Err("无法停止设备启动进程".into()); }
                            child.wait().await.map_err(|e|e.to_string())?;
                            return Ok(false);
                        }
                        line=lines.next_line() => {
                            let Some(line)=line.map_err(|e|e.to_string())? else {break};
                            if let Some(json)=line.strip_prefix("AI_PHONE_SETUP ") {
                                #[derive(Deserialize)]
                                struct Progress {phase:String, error:String}
                                let progress:Progress=serde_json::from_str(json).map_err(|e|e.to_string())?;
                                let message=if progress.error.is_empty(){Event::Phase(progress.phase)}else{Event::Failed(progress.error)};
                                if output.send(message).await.is_err(){return Ok(false);}
                            }
                        }
                    }
                }
                let status=child.wait().await.map_err(|e|e.to_string())?;
                if !status.success(){return Err(format!("设备启动失败：{status}"));}
                let root=self.home.join("releases").join(&self.source);
                output.send(Event::Ready(root.to_string_lossy().into_owned())).await.map_err(|e|e.to_string())?;
                Ok(true)
            }.await;
            match result {
                Ok(true) => {
                    let _ = stop.await;
                }
                Ok(false) => {}
                Err(error) => {
                    let _ = output.send(Event::Failed(error)).await;
                    let _ = stop.await;
                }
            }
            let result = Self::stop(self.home).await;
            let _ = output.send(Event::Stopped(result)).await;
        })
    }

    pub async fn stop(home: PathBuf) -> Result<(), String> {
        ai_phone_host_runtime::stop_macos_installation(&home)
            .await
            .map_err(|e| e.to_string())
    }
}
