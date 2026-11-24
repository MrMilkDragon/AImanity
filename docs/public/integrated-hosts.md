# Fluorine host runtime

Choose a language to open the shared workbench. Chat, model settings, knowledge and files work with zero devices. Use Device Manager to create a guest, authorize the current computer or pair another device.

| Client | Local Android phone | Local Linux desktop |
| --- | --- | --- |
| Apple Silicon macOS | AOSP on Avo | Debian desktop on Lima/VZ |
| Windows 11 x64 | AOSP on QEMU/WHPX | Debian desktop on QEMU/WHPX |
| Linux x86_64 | AOSP on QEMU/KVM | Debian desktop on QEMU/KVM |
| Android phone or tablet | AOSP on AVF | Debian desktop on AVF |

Android local guests require ARM64, root access and the AVF custom virtual machine service. The same landscape APK serves phones and tablets.

Guest templates ship with the installer. Creating a device assigns its own resources, writable disk and control endpoint. Disconnecting stops an owned guest and keeps its disk. Deleting the guest removes its local instance. Pairing a physical device registers the authorized connection.

The Iced/wgpu client opens an embedded Rust database (`workspace.redb`) for the shared workspace. Each device owns its audio routes and control session. Device input, state and action receipts use protobuf. [Installation and platform requirements](distribution-requirements.md)

## 中文

选择语言即可进入空工作台。设备管理支持按需创建 Android 手机或 Linux 电脑，授权本机或配对其他设备。每台虚拟设备拥有独立的可写盘和音频通道。断开时关机并保留设备盘，删除时清理该实例。设备、对话和任务由原生工作台统一管理。[平台与安装包](distribution-requirements.md)
