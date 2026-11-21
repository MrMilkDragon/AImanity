# 客户端、运行环境、用户数据与升级

[English](distribution.en.md)

安装包内置设备与服务。用户在客户端配置设备资源和模型。详见[下载与宿主要求](release-status.md)。

## 已实现的目录边界

原生首配使用应用数据目录下的独立 `host`：

```text
host/
  deployment.json             首配检查点
  tools/                      当前开发安装链的受管工具
  releases/<payload-sha256>/  校验后的版本代码和运行组件
    data -> host/user/data
    .env -> host/user/.env
  user/
    .env                      私有数据库环境
    data/                     客体、账号、模型、渠道与工作文件
```

`crates/host-runtime/src/source_cli.rs` 按资源摘要暂存[运行组件](runtime-bundles.md)，并核对指向持久用户数据的链接。

源码暂存命令复用原生的内容寻址暂存器。

PostgreSQL 使用 Linux 客体内的私有存储。备份包含数据库、Android 状态、Linux 的可写变更与主目录，以及私有文件。恢复到空目标并保留隔离，供验证后启用。用户 Skill、使用经验、记忆、知识、附件与停用偏好跨升级保留。

## 客户端更新

客户端选择当前平台的正式发行，核验 Ed25519 签名和安装包摘要后安装。macOS 替换应用包并重新打开。Windows 运行现有 Inno 安装器并重新打开客户端，沿用已发布的 QEMU 底盘和原写入层。设备、模型与聊天使用独立的持久数据目录。

两端安装包验收后，在发行维护者本机签署更新清单。

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- sign-update /path/to/offline-key \
  AImanity-0.0.5-macos-arm64.dmg AImanity-0.0.5-windows-x64-setup.exe native-stable.json
```

顺序打包时，本地保留两份安装包回执和一个安装包。另一端的安装包通过管道传给 `sign-update --stdin macos` 或 `sign-update --stdin windows`，后接原有四个参数。工具按传入的文件名读取同目录回执。两端安装包都可下载后再发布共享更新清单。

在 GitHub 发行页附上 DMG 和 EXE，将签名清单 `native-stable.json` 提交到 `update-feed` 分支。Calcium 用户先卸载 0.0.3，再从 Releases 下载 0.0.4 全新安装。此后的版本通过 Dysprosium 客户端内更新。

完整包必须包含干净Android/Linux基础、模拟器、预编译服务与匹配运行环境，并附来源、声明和适用对应源码。详见[发行要求](distribution-requirements.md)、[当前状态](release-status.md)和[备份恢复](backup-restore.zh-CN.md)。
