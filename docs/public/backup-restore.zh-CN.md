# 私有备份与恢复

[English](backup-restore.en.md)

从安装目录运行以下命令。备份暂停本安装的服务，保存 PostgreSQL、Linux 可写层、设备与工作文件，然后恢复服务。请将完整 `.backup` 目录保存在私有加密存储中。

```sh
./ai-phone backup --output data/backups/my-backup.backup
```

将备份恢复到空目录和专用的 PostgreSQL 17 数据库。为目标数据库准备私有配置目录、管理员 `.env` 与本地套接字，并指定匹配的 Android SDK。

```sh
assistant --state-backup restore --root /private/restore-config \
  --pg-socket /private/restore-config/socket --database ai_phone_recovered \
  --sdk-root /private/android-sdk --archive /private/my-backup.backup \
  --destination /private/AImanity-recovered
```

恢复会核对数据库行和文件、重新定位 Android 磁盘路径，并暂停任务和通知触发。报告和隔离标记写入 `data/`。Linux 个人目录随私有文件恢复。

恢复 Linux 可写层时，准备独立原生客体。使用原版共享文件系统，将恢复后的 `data` 挂载为客体的安装数据，并保持桌面可写层为空、桌面已停止。运行：

```sh
assistant --state-backup recover-desktop --root /private/native-guest-release \
  --destination /private/AImanity-recovered
```

可写层保留属主、权限、删除标记和扩展属性。恢复后仍保持隔离。先停止原实例，再核对并启用恢复环境。旧版 Linux 容器备份使用原版客户端启用。
