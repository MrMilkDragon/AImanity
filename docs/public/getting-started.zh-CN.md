# 本地安装与配置

[English](getting-started.en.md) · 开发者源码安装

## 选择安装路径

下载前请先查看[发行状态](release-status.md)中的安装包供应情况和各平台验收范围。产品目标是安装客户端后选择语言、设备资源并配置模型，具体见[一体化宿主约定](integrated-hosts.md)。

下文介绍 macOS Apple Silicon 的源码开发环境。完整安装包见[下载](release-status.md)。Windows 后端开发见[Linux 客体指南](windows-backend-runtime.md)。

## 更新已安装客户端

有新版时，工作台右上角会显示更新图标。展开查看版本号、版本名和更新说明，选择**安装更新**。客户端会核验安装包、完成安装并重启，继续使用原设备、模型配置和聊天。Windows 安装时可能出现系统授权提示。

## 源码安装的支持范围

在 Apple Silicon Mac 上准备 Rust 1.96.0、Xcode 命令行工具、Android 35 平台和原生构建工具。按 [macOS 工厂](engineering/macos-full-runtime.md) 准备离线 Android 与 Debian/Openbox 资源。全新源码构建和双设备运行预留 35 GiB 磁盘空间、16 GiB 内存。

## 首次安装

1. 运行 `./ai-phone doctor`。
2. 运行 `./ai-phone install --resources /path/to/host`，选择语言和设备资源。路径使用工厂产出的 `host` 目录。
3. 打开工作台，在「配置中心 → Agent」保存并选择配置。手机和电脑共享当前所选 Agent。

## 资源与网络

`./ai-phone configure` 将本机预设保存在私有 `data/deployment/profile.json`（0600）。手机支持1/2/4核、1.5–4GB内存、16/24/32GB新盘；电脑创建界面单独指定CPU/内存。修改预设不会重建已有设备；资源迁移/磁盘扩容仍是发行阻塞项。Linux主目录由宿主目录承载，没有独立磁盘配额。

新部署默认直连。可在[工作台代理](network.md)导入并启用订阅。

## 日常操作

`./ai-phone start` 启动源码服务，`open` 打开已安装的原生客户端。`stop` 停止本项目服务，保留设备、数据库和文件。重复start不重启现有任务，只补缺失且属于本目录的服务；已加载但反复崩溃的服务仍需检查日志。登录自启与开机前无人值守服务不是同一种模式。

LAN地址见私有 `data/deployment.json`，只选私有网络接口。后端、ADB、数据库和媒体控制保持loopback/私网。通过受信 TLS 或自有 VPN 接入不可信网络。

## 源码归档

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- source package dist
```

命令将带有内置文件清单的源码归档写入 `dist`。见[发行与更新](distribution.zh-CN.md)。

默认 `local_entry: true` 是同一可信局域网内的个人部署模式：能够访问网页的人可作为本机所有者进入。不要把这个入口作为多用户权限隔离；需要密码认证的既有部署可在私有配置中关闭本地入口，旧认证端点保留。

运行服务与数据库迁移使用独立角色；启动前显式执行迁移，运行服务不使用bootstrap超级用户。见[数据库权限](database-roles.md)。使用 `backup` 备份、`restore` 恢复到空目标，见[私有备份恢复](backup-restore.zh-CN.md)。

## Liberty（开发中运行时）

在共享对话中开启 Liberty，Agent 可跨设备选择活动、形成和调整目标。右上角的「任务与目标」可设置、修订或关闭明确目标，运行时会暂停 Liberty。Agent 可以在核验后保存可复用的方法。见 [Liberty](autonomous-exploration.md) 和[通知工作流](notification-workflows.md)。
