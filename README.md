# AImanity

English · [简体中文](#简体中文) · [繁體中文](docs/public/readme.zh-TW.md) · [日本語](docs/public/readme.ja.md) · [한국어](docs/public/readme.ko.md) · [Deutsch](docs/public/readme.de.md) · [Français](docs/public/readme.fr.md) · [Español](docs/public/readme.es.md) · [Русский](docs/public/readme.ru.md) · [العربية](docs/public/readme.ar.md)

An agent that listens while it works, explores on its own, and learns from using your phone and computer.

## Downloads

[Download the Apple Silicon macOS installer](https://github.com/MrMilkDragon/AImanity/releases/latest) · [Fluorine 0.0.6 archive](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6)

[Gallium guide](docs/public/gallium.md)

## Gallium features

- **Use a phone.** An Agent reads the Android screen, operates installed apps such as WeChat and handles notifications.
- **Use a computer.** An Agent controls the Linux desktop and works in apps such as LibreOffice. Choose two devices in Files to copy work between them.
- **Keep working.** Add instructions or interrupt a response in the same conversation. One conversation follows the task across devices.
- **Choose a mode.** Copilot takes live voice instructions locally. Liberty explores freely. Task & Goal works toward a goal you can revise as it runs. Plan starts a task on a schedule or when a device notification matches.
- **Reuse skills.** Skills, linked knowledge and work files are shared. Save model profiles and switch the active Agent in Configuration.

See [Gallium setup](docs/public/gallium.md) and [Liberty](docs/public/autonomous-exploration.md).

## Architecture

| Layer | Technology | Directory |
| --- | --- | --- |
| Desktop client and workbench | Rust (Iced/wgpu) | `apps/native` |
| Workspace, devices and media | Rust and Tokio | `crates` |
| Business API and state | protobuf, embedded redb | `packages/protocol`, `apps/native/src/workspace` |
| Device runtimes | AOSP on Avo and Debian/Openbox on Lima | `crates/host-runtime`, `crates/guest-runtime` |
| Built-in capabilities | Skills and device profiles | `skills`, `config` |

Gallium starts with zero devices. Add Android or Linux guests as needed and grant host desktop access for an individual task. The Agent can inspect and repair local guests from the shared conversation.

[Use this computer for one task](docs/public/native-client.en.md#use-this-computer-for-one-task)

Agents plan from current device observations and record their actions and results. Cargo builds the project. Device disks, accounts and attachments stay in the local installation.

[Architecture](docs/public/architecture.en.md) · [Documentation](docs/public/index.md) · [Contributing](CONTRIBUTING.md) · [Report an issue](https://github.com/MrMilkDragon/AImanity/issues/new/choose)

## Roadmap

More voice providers, app Skills and notification-driven office workflows. [Development roadmap](docs/public/release-roadmap.en.md)

## License and use

Original code is licensed under [Apache-2.0](LICENSE). Third-party components retain their own terms. Connect accounts and devices you are authorized to use.

[Disclaimer](DISCLAIMER.md) · [Privacy](PRIVACY.md) · [Third-party notices](THIRD_PARTY_NOTICES.md) · [Security](SECURITY.md)

---

# 简体中文

[Back to English](#aimanity)

## 下载

[下载 Apple Silicon macOS 安装包](https://github.com/MrMilkDragon/AImanity/releases/latest) · [Fluorine 0.0.6 存档](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6)

[Gallium 安装与使用](docs/public/gallium.md#简体中文)

## Gallium 功能

- **操作手机。** Agent 查看真实 Android 画面，操作微信等已安装应用并处理通知。
- **操作电脑。** Agent 控制 Linux 桌面，可使用 LibreOffice 等应用。在文件管理器选择设备即可互传文件。
- **持续工作。** 在同一对话中补充指令或打断回复。切换设备时，同一 Agent 和对话继续工作。
- **选择模式。** Copilot 在本机识别实时语音，Liberty 自由探索，任务与目标支持边执行边修订。计划可按时间或设备通知启动任务。
- **复用能力。** Skill、关联知识文档和工作文件共享。在配置中心保存模型配置并切换当前 Agent。

[Gallium 设置](docs/public/gallium.md#简体中文) · [Liberty](docs/public/autonomous-exploration.md)

## 技术架构

客户端与工作台采用 Rust、Iced 和 wgpu。设备控制和媒体使用 Rust 与 Tokio，工作区使用 protobuf 和内嵌 redb。Cargo 负责构建。macOS 通过 Avo 运行 Android，通过 Lima 运行 Debian/Openbox 桌面。

Gallium 从零设备启动。按需创建 Android 或 Linux 虚拟设备，可逐任务授权使用本机。Agent 可以从共享对话检查和修复本地虚拟设备。

[在本机执行一次任务](docs/public/native-client.zh-CN.md#在本机执行一次任务) · [应用工作区设计](docs/public/gallium-runtime.zh-CN.md)

Agent 根据当前设备画面操作，动作账本保留执行过程和结果。设备盘、账号、聊天和附件保存在本地安装目录或数据库中。

[架构与目录](docs/public/architecture.zh-CN.md) · [文档](docs/public/index.md) · [参与贡献](CONTRIBUTING.md) · [提交问题](https://github.com/MrMilkDragon/AImanity/issues/new/choose)

## 后续开发

扩展语音服务商、应用专项 Skill 和通知触发的办公流程。[开发路线](docs/public/release-roadmap.zh-CN.md)

## 许可与使用

原创代码采用 [Apache-2.0](LICENSE)。第三方组件沿用各自许可。请仅连接有权使用的账号和设备。

[免责声明](DISCLAIMER.md) · [隐私](PRIVACY.md) · [第三方声明](THIRD_PARTY_NOTICES.md) · [安全](SECURITY.md)
