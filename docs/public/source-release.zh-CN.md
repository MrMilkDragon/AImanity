# 源码构建与贡献

[English](source-release.en.md) · [源码仓库](https://github.com/MrMilkDragon/AImanity)

Rust 客户端、模型适配器和嵌入式工作区位于 `apps/native` 和 `crates/model`。模型配置、会话、任务和知识保存在工作区数据库。

原创代码采用 [Apache-2.0](../../LICENSE)，第三方依赖、素材与客体应用保留各自许可，见[第三方声明](../../THIRD_PARTY_NOTICES.md)。[下载 macOS 或 Windows 客户端](release-status.md)。

## 环境与启动

在仓库中启动 Rust 客户端。开发对话和存储功能时可使用零设备工作区。

```sh
cargo run --locked --manifest-path apps/native/Cargo.toml --bin aimanity-native
```

在 Agent 设置中保存模型服务地址，选择当前 Agent。

详见[安装](getting-started.zh-CN.md)、[运行组件](runtime-bundles.md)、[架构](architecture.zh-CN.md)、[备份恢复](backup-restore.zh-CN.md)和[发行状态](release-status.md)。

## 源码与用户数据

导出工具按白名单选取源码，执行隐私和许可检查。开发流程见[贡献指南](../../CONTRIBUTING.md)。

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- source check
cargo run --quiet --locked -p ai-phone-release-tools -- source export /path/to/new-review-directory --git
```

源码导出到新目录，并生成可审阅的本地仓库。

## 自动检查

Source CI 编译 protobuf 和 Rust 目标。原生工作区测试核对重开后的消息和附件，在模型请求进行中退出，再次打开时核对队列里的任务。

Native CI 在匹配环境构建 macOS ARM64 和 Windows x64 桌面客户端，检查包体并验证虚拟手机控制应用。详见[组件目录](engineering/index.md)、[发行状态](release-status.md)与[兼容矩阵](compatibility.md)。
