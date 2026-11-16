# 原生客户端开发

[English](native-client.en.md)

## 0.0.4 Dysprosium 开发

`apps/native` 是 Iced + wgpu 原生工作台。Rust 宿主入口启动手机、电脑和本地服务，原生模式不启动 Next.js 页面服务。设备画面直接上传 GPU 纹理，输入和共享对话复用既有 protobuf 服务。

当前 Mac 开发构建已接通真实双设备、直接输入、设备布局切换和就地 Agent 选择窗。组件迁移沿用现版的玻璃、字体、控件和动效。完整功能窗口、原生音频、首装、更新及 Windows 宿主入口是后续接入板块。

```sh
cargo build --release --manifest-path apps/native/Cargo.toml
apps/native/target/release/aimanity-native /absolute/path/native-launch.json
```

私有启动文件通过 `home`、`resource`、`request`、`source`、`android`、`images` 指定部署位置、离线组件标识和设备配置请求。凭据和设备状态保存在该部署目录。组件结构见[运行包布局](runtime-bundles.md)。

## 已发行客户端

Tauri 客户端负责本机配置。两端都运行原生 Android。macOS 使用 Lima 运行 Linux 环境，Windows 使用 QEMU 和 Windows Hypervisor Platform 启动内置 Linux 客体。初始化完成后，工作台连接本地服务。

配置界面支持十种语言、键盘操作、重试和取消。通过 **AImanity → 设置** 返回配置。

## 构建

```sh
./scripts/bootstrap.sh
./scripts/bun scripts/build-client.ts --with-web-runtime
```

这条命令构建开发客户端。完整安装包见 [macOS 工厂](engineering/macos-full-runtime.md)及 [Windows 工厂](windows-client-factory.md)。

macOS 需要 Apple Silicon 和 macOS 15.0 或更高版本。Windows 安装包适用于 Windows 11 x64。[下载与状态](release-status.md)
