# 备份 Fluorine 工作区

[English](backup-restore.en.md)

退出客户端，让它关闭数据库。把[应用数据目录](distribution.zh-CN.md)中的整个 `host` 文件夹复制到私人存储，同时保留 `workspace.redb` 和 `files/`。数据库包含对话、模型凭据、知识和原始附件，文件夹还包含可编辑的工作文件。

恢复到全新的 Fluorine 配置时，先退出客户端，把备份的 `host` 文件夹放回该配置的应用数据目录。重开后核对对话、模型和工作文件。存放在其他路径的设备磁盘需要单独复制。
