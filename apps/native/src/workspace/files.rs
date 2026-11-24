use super::*;
use crate::device_files::{Endpoint, absolute, child};
use ai_phone_api::wire::{DeviceIoOperation as Io, FileLocation as Location, FileOperation as Op};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

impl Workspace {
    pub(crate) fn upload(&self, name: String, bytes: Vec<u8>) -> Result<wire::ChatAttachment> {
        use std::io::Write;
        let file = Path::new(&name);
        if file.file_name().and_then(|name| name.to_str()) != Some(name.as_str()) {
            return Err("文件名无效".into());
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.files)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        let destination = self.files.join(file);
        let destination = match staged.persist_noclobber(&destination) {
            Ok(_) => destination,
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stem = file.file_stem().unwrap_or_default().to_string_lossy();
                let mut unique = self
                    .files
                    .join(format!("{}-{}", stem, uuid::Uuid::now_v7()));
                if let Some(ext) = file.extension() {
                    unique.set_extension(ext);
                }
                e.file.persist_noclobber(&unique)?;
                unique
            }
            Err(e) => return Err(e.into()),
        };
        let result = self.store_attachment(name, &bytes);
        if result.is_err() {
            fs::remove_file(destination)?;
        }
        result
    }

    fn work_files(&self) -> Endpoint {
        Endpoint::Directory(self.files.clone())
    }
    pub(crate) async fn import_file(
        &self,
        src: &Path,
        directory: &str,
    ) -> Result<wire::FileLocation> {
        let file = src
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("文件名无效")?;
        let dest = self.work_files().local_path(directory)?.join(file);
        let mut from = tokio::fs::File::open(src).await?;
        let tmp = tempfile::NamedTempFile::new_in(dest.parent().ok_or("目录不存在")?)?;
        let mut writer = tokio::fs::File::from_std(tmp.reopen()?);
        tokio::io::copy(&mut from, &mut writer).await?;
        writer.sync_all().await?;
        drop(writer);
        tmp.persist_noclobber(dest)?;
        Ok(wire::FileLocation {
            path: format!("{}/{file}", directory.trim_end_matches('/')),
            ..Default::default()
        })
    }
    pub(crate) async fn export_file(&self, source: &str, target: &Path) -> Result<()> {
        let src = self.work_files().local_path(source)?;
        let temp = tempfile::NamedTempFile::new_in(target.parent().ok_or("目录不存在")?)?;
        tokio::fs::copy(src, temp.path()).await?;
        temp.persist(target)?;
        Ok(())
    }
    pub(crate) async fn send_audio(
        &self,
        id: &str,
        path: &str,
        session: Option<&CancellationToken>,
    ) -> Result<()> {
        let mut f = tokio::fs::File::open(self.work_files().local_path(path)?).await?;
        self.peers
            .io(
                id,
                wire::DeviceIoRequest {
                    operation: Io::AudioInput.into(),
                    wav_input: true,
                    ..Default::default()
                },
                session,
            )
            .await?
            .upload(&mut f)
            .await
    }
    pub(crate) async fn file_list(&self, loc: Location) -> Result<wire::FileListing> {
        absolute(&loc.path)?;
        let mut list = if loc.device_id.is_empty() {
            self.work_files().list(&loc.path).await?
        } else {
            let mut data = Vec::new();
            self.device_io(&loc.device_id, Io::List, &loc.path, "")
                .await?
                .receive(&mut data)
                .await?;
            wire::FileListing::decode(data.as_slice())?
        };
        list.location = Some(loc);
        Ok(list)
    }
    async fn change_file(&self, op: Io, loc: &Location, to: &str) -> Result<()> {
        if loc.device_id.is_empty() {
            self.work_files().change(op, &loc.path, to).await
        } else {
            self.device_io(&loc.device_id, op, &loc.path, to)
                .await?
                .ack()
                .await
        }
    }
    async fn read_file(&self, loc: &Location, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        if loc.device_id.is_empty() {
            self.work_files().read(&loc.path, out).await
        } else {
            self.device_io(&loc.device_id, Io::Read, &loc.path, "")
                .await?
                .receive(out)
                .await
        }
    }
    async fn write_file(&self, loc: &Location, input: &mut (impl AsyncRead + Unpin)) -> Result<()> {
        if loc.device_id.is_empty() {
            self.work_files().write(&loc.path, input).await
        } else {
            self.device_io(&loc.device_id, Io::Write, &loc.path, "")
                .await?
                .upload(input)
                .await
        }
    }
    async fn copy_leaf(&self, source: &Location, target: &Location) -> Result<()> {
        let (mut read, mut write) = tokio::io::duplex(128 * 1024);
        let produce = async {
            self.read_file(source, &mut write).await?;
            write.shutdown().await?;
            Result::Ok(())
        };
        tokio::try_join!(produce, self.write_file(target, &mut read))?;
        Ok(())
    }
    async fn copy_tree(&self, source: &Location, target: &Location) -> Result<()> {
        let (parent, name) = source.path.rsplit_once('/').ok_or("文件名无效")?;
        let parent = Location {
            path: if parent.is_empty() { "/" } else { parent }.to_owned(),
            device_id: source.device_id.clone(),
        };
        let stat = self
            .file_list(parent)
            .await?
            .entries
            .into_iter()
            .find(|e| e.name == name)
            .ok_or("源文件不存在")?;
        if stat.symlink {
            return Err("不能复制符号链接".into());
        }
        if !stat.directory {
            return self.copy_leaf(source, target).await;
        }
        let (dir, _) = target.path.rsplit_once('/').ok_or("文件名无效")?;
        let stage = Location {
            path: format!("{dir}/.aimanity-{}", uuid::Uuid::new_v4()),
            device_id: target.device_id.clone(),
        };
        self.change_file(Io::Mkdir, &stage, "").await?;
        let result = async {
            let mut directories = vec![(source.clone(), stage.clone())];
            while let Some((from, to)) = directories.pop() {
                for entry in self.file_list(from.clone()).await?.entries {
                    if entry.symlink {
                        return Err("不能复制符号链接".into());
                    }
                    let src = Location {
                        path: child(&from.path, &entry.name)?,
                        device_id: from.device_id.clone(),
                    };
                    let dst = Location {
                        path: child(&to.path, &entry.name)?,
                        device_id: to.device_id.clone(),
                    };
                    if entry.directory {
                        self.change_file(Io::Mkdir, &dst, "").await?;
                        directories.push((src, dst));
                    } else {
                        self.copy_leaf(&src, &dst).await?;
                    }
                }
            }
            self.change_file(Io::Rename, &stage, &target.path).await
        }
        .await;
        if result.is_err() {
            let _ = self.change_file(Io::Remove, &stage, "").await;
        }
        result
    }
    pub(crate) async fn file_manage(
        &self,
        q: wire::FileManageRequest,
    ) -> Result<wire::FileManageResult> {
        let op = Op::try_from(q.operation)?;
        let target = q.target.ok_or("缺少目标目录")?;
        absolute(&target.path)?;
        let mut results = Vec::new();
        if op == Op::Mkdir {
            let dst = Location {
                path: child(&target.path, &q.name)?,
                device_id: target.device_id,
            };
            self.change_file(Io::Mkdir, &dst, "").await?;
            results.push(wire::FileResult {
                target: Some(dst),
                ..Default::default()
            });
        } else {
            for src in q.sources {
                let apply=async {
                    let session=|device:&str| if device.is_empty(){Ok(CancellationToken::new())}else{self.peers.active_session(device)};
                    let source_session=session(&src.device_id)?;
                    let target_session=if op==Op::Delete{source_session.clone()}else{session(&target.device_id)?};
                    tokio::select!{biased;
                    _=source_session.cancelled()=>Err("源设备已断开".into()),
                    _=target_session.cancelled()=>Err("目标设备已断开".into()),
                    result=async {
                    let path=absolute(&src.path)?;
                    let (_,leaf)=path.rsplit_once('/').filter(|(_,n)|!n.is_empty()).ok_or("不能更改根目录")?;
                    if op==Op::Delete {self.change_file(Io::Remove,&src,"").await?;return Result::Ok(None);}
                    let dst=Location {path:child(&target.path,if q.name.is_empty(){leaf}else{&q.name})?,device_id:target.device_id.clone()};
                    let same=src.device_id==dst.device_id;
                    if same && (src.path==dst.path || dst.path.starts_with(&format!("{}/",src.path))) {return Err("源与目标路径重叠".into());}
                    match op {
                        Op::Move|Op::Rename if same=>self.change_file(Io::Rename,&src,&dst.path).await?,
                        Op::Copy|Op::Move=>{
                            self.copy_tree(&src,&dst).await?;
                            if op==Op::Move {self.change_file(Io::Remove,&src,"").await?;}
                        },
                        _=>return Err("文件操作无效".into())
                    }
                    Ok(Some(dst))
                    }=>result }
                }.await;
                let (target, error) = match apply {
                    Ok(dst) => (dst, String::new()),
                    Err(e) => (None, e.to_string()),
                };
                results.push(wire::FileResult {
                    source: Some(src),
                    target,
                    error,
                });
            }
        }
        Ok(wire::FileManageResult { results })
    }
}
