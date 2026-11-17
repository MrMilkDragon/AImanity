# 源码构建与贡献

[English](source-release.en.md) · [源码仓库](https://github.com/MrMilkDragon/AImanity)

源码包含 Rust 原生工作台、Rust 执行服务、Android 桥接、Linux 桌面配方、protobuf 契约、内置 Skill、安装与备份恢复工具、测试及 CI。手机与电脑共用当前 Agent。PostgreSQL 保存任务检查点，Redis 提供缓存。

原创代码采用 [Apache-2.0](../../LICENSE)，第三方依赖、素材与客体应用保留各自许可，见[第三方声明](../../THIRD_PARTY_NOTICES.md)。[下载 macOS 或 Windows 客户端](release-status.md)。

## 环境与启动

在 Apple Silicon 上使用 Rust 1.96.0、Android 35 原生构建工具和 Xcode 命令行工具。[macOS 工厂](engineering/macos-full-runtime.md)负责准备离线设备资源。

```sh
./ai-phone doctor
./ai-phone install --resources /path/to/host
```

宿主默认直联。需要代理时，在工作台导入订阅。在「配置中心 → Agent」保存模型配置并选择两台设备共用的 Agent。

详见[安装](getting-started.zh-CN.md)、[运行组件](runtime-bundles.md)、[架构](architecture.zh-CN.md)、[备份恢复](backup-restore.zh-CN.md)和[发行状态](release-status.md)。

## 源码与用户数据

导出工具按白名单选取源码，执行隐私和许可检查。开发流程见[贡献指南](../../CONTRIBUTING.md)。

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- source check
cargo run --quiet --locked -p ai-phone-release-tools -- source export /path/to/new-review-directory --git
```

导出目标必须是新目录，产出源码清单和没有远端的本地审阅仓库。与公开 checkout 比较差异后提交，后续贡献保留公开历史。运行用户目录的升级使用版本化制品、备份、迁移和恢复流程，不直接用 Git 覆盖用户状态。

## 自动检查

Source CI 执行 protobuf、Rust、原生工作台、适配器、归档和安装契约。预构建验证展开并执行实际包内文件。临时 PostgreSQL 检查迁移、受限角色、备份恢复与 worker 故障；重启测试按顺序运行，不使用真实设备、账号或付费模型。

Native CI 在匹配环境构建 macOS ARM64 和 Windows x64 桌面客户端，检查包体并验证虚拟手机控制应用。详见[组件目录](engineering/index.md)、[发行状态](release-status.md)与[兼容矩阵](compatibility.md)。
