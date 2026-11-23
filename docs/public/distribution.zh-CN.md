# 安装 Fluorine

[English](distribution.en.md)

0.0.6 Fluorine 正在开发，采用全新安装方式，应用标识为 `dev.aimanity.workspace`。正式发布后，安装对应平台的客户端，选好语言即可进入工作台，在设置里配置模型。零台设备也能开始使用。

## 本地工作区

Rust 客户端在自己的应用数据目录打开 `host/workspace.redb`。共享对话、待发送消息、模型配置、设备登记、记忆、知识和聊天附件由嵌入式数据库提交。可编辑的工作文件保存在 `host/files/`。

| 桌面平台 | 应用数据目录 |
| --- | --- |
| macOS | `~/Library/Application Support/dev.aimanity.workspace` |
| Windows | `%APPDATA%\dev.aimanity.workspace` |
| Linux | `$XDG_DATA_HOME/dev.aimanity.workspace` 或 `~/.local/share/dev.aimanity.workspace` |

已安装的 Europium 继续使用自己的目录。Fluorine 使用平台安装包创建独立工作区。
