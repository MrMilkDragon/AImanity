# Fluorine 技术架构

[English](architecture.en.md)

选好语言后，Iced 与 wgpu 原生客户端直接打开本地工作区。界面、数据库和模型任务在同一进程内运行。零台设备即可使用共享对话。

| 模块 | 实现 |
| --- | --- |
| 共享对话、设备登记、任务队列 | `apps/native/src/workspace` 用 redb 事务保存 protobuf 记录。 |
| 模型执行 | `crates/model` 连接所选模型服务，支持 Chat Completions、Responses 和 Anthropic Messages 的流式响应。 |
| 知识与文件 | 工作区保存关联文档和聊天附件原件。工作文件目录中的副本可直接编辑。 |
| 设备 | 每台设备有独立 ID。已连接设备的控制与音视频走各自的传输通道。 |
| 宿主机操作 | 用户授权对应一次请求。截屏和输入使用该次授权。 |

发送消息时，用户消息、助手回复位置和任务在一次事务内落盘，随后唤醒执行器。重发相同请求 ID 会找到原消息。退出会取消进行中的模型请求，未完成任务保留在队列。界面通过进程内通知读取变化。

当前开发版已接入共享对话、模型选择、知识和工作文件。虚拟与配对设备的实际操作、主动模式以及并行交接仍在迁移。

[工作区目录与安装](distribution.zh-CN.md) · [工作区备份](backup-restore.zh-CN.md) · [Europium 架构](https://github.com/MrMilkDragon/AImanity/blob/v0.0.5/docs/public/architecture.zh-CN.md)
