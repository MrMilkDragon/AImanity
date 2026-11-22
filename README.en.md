# AImanity

English · [简体中文](README.md#简体中文) · [繁體中文](docs/public/readme.zh-TW.md) · [日本語](docs/public/readme.ja.md) · [한국어](docs/public/readme.ko.md) · [Deutsch](docs/public/readme.de.md) · [Français](docs/public/readme.fr.md) · [Español](docs/public/readme.es.md) · [Русский](docs/public/readme.ru.md) · [العربية](docs/public/readme.ar.md)

An agent that listens while it works, explores on its own, and learns across Android and Linux.

## Downloads

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.4 Dysprosium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.4/AImanity-0.0.4-windows-x64-setup.exe)

The 0.0.4 client will offer Europium once both desktop installers are available. [Installation guide](docs/public/release-status.md) · [Uninstall](docs/public/release-status.md#uninstall)

## What it does

- **Use a phone.** An Agent reads the Android screen, operates installed apps such as WeChat and handles notifications.
- **Use a computer.** An Agent controls the Linux desktop and works in apps such as LibreOffice. Connect the devices to move files between them.
- **Keep working.** Add instructions or interrupt a response in the shared conversation. Both devices keep working while you switch views.
- **Reuse skills.** Agents share Skills, linked knowledge and work files. Model settings, memory and channel connections belong to each Agent.

See [model and channel setup](docs/public/model-and-channel-setup.md), [notification workflows](docs/public/notification-workflows.md) and [Liberty](docs/public/autonomous-exploration.md).

## How it works

| Layer | Technology | Directory |
| --- | --- | --- |
| Desktop client and workbench | Rust (Iced/wgpu) | `apps/native` |
| Backend, devices and media | Rust, Axum, Tokio | `crates` |
| Business API and state | protobuf, PostgreSQL, SQLx | `packages/protocol` |
| Device runtimes | AOSP on Avo or QEMU WHPX, native Debian/Openbox | `crates/host-runtime`, `crates/guest-runtime` |
| Built-in capabilities | Skills and device profiles | `skills`, `config` |

Agents plan from current device observations and record their actions and results. Cargo builds the project. Device disks, accounts and attachments stay in the local installation.

[Architecture](docs/public/architecture.en.md) · [Documentation](docs/public/index.md) · [Contributing](CONTRIBUTING.md) · [Report an issue](https://github.com/MrMilkDragon/AImanity/issues/new/choose)

## Roadmap

More voice providers, app Skills and notification-driven office workflows. [Development roadmap](docs/public/release-roadmap.en.md)

## License and use

Original code is licensed under [Apache-2.0](LICENSE). Third-party components retain their own terms. Connect accounts and devices you are authorized to use.

[Disclaimer](DISCLAIMER.md) · [Privacy](PRIVACY.md) · [Third-party notices](THIRD_PARTY_NOTICES.md) · [Security](SECURITY.md)
