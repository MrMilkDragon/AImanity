# Windows Linux guest

The Windows installer carries the Linux kernel, base disk and release layer. QEMU boots the guest through Windows Hypervisor Platform. Each Linux device has its own writable QCOW2 overlay. The client opens a local workbench at first launch. Device Manager starts guests on demand.

| Resource | Contents |
|---|---|
| Guest boot | Linux kernel, immutable base disk and a release layer built from the candidate commit |
| Guest services | Rust desktop control, files, audio and notifications |
| Workspace | Devices, chats and Agent configuration in the host’s embedded redb database |
| Linux root filesystem | One amd64 filesystem shared by the native desktop and workspaces |
| Android | x86_64 AOSP guest, root ADB, input method, launcher and browser |
| Device data | A writable disk for each created device |

BusyBox launches the Rust bootstrap. The host authenticates over the named virtio-serial port `org.aiphone.bootstrap`. The workbench uses a separate control channel for each guest.

## QEMU build inputs

Prepare the pinned QEMU executables, firmware and libraries on Windows x64 with the [QEMU factory](../../.github/workflows/windows-qemu.yml).

```sh
cargo +1.96.0 run --locked -p ai-phone-release-tools -- windows-factory qemu . build/windows-qemu
```

The [Windows Linux guest workflow](../../.github/workflows/windows-guest.yml) builds the shared amd64 root filesystem and stages current Rust services, database binaries and sandbox tools on an isolated Linux x64 builder. It seals them over the pinned kernel and backing disk.

```sh
cargo +1.96.0 run --locked -p ai-phone-release-tools -- windows-release-context \
  build/windows-release-context --linux-rootfs build/linux-rootfs.tar.zst \
  --guest-assets build/windows-guest-assets
cargo +1.96.0 run --locked -p ai-phone-release-tools -- windows-release \
  build/windows-qemu/baseline build/windows-release-context build/windows-guest.payload
```

Build the asset directory with `release-tools windows-guest-assets build/windows-guest-assets` on the same Linux x64 factory. It collects Tesseract and the guest’s runtime tools from signed Debian packages.

The [Linux backend checks](../../.github/workflows/linux-backend.yml) exercise the QEMU bootstrap command and guest lifecycle. The [Android WHPX workflow](../../.github/workflows/windows-aosp-qualification.yml) boots the bundled AOSP guest. [Windows installer acceptance](../../.github/workflows/windows-installer-acceptance.yml) installs the exact signed EXE in a fresh Windows profile and drives both devices through the native client.

## 中文

Windows 客户端首次打开即可使用空工作台。用户在设备管理中按需创建 Android 或 Linux。每台虚拟设备使用独立的可写磁盘。工作区的设备记录、Agent 配置和聊天保存在宿主机的 redb 数据库中。

Linux 后端检查覆盖启动、恢复和镜像归属。Windows 首装验收使用最终签名 EXE，验证原生窗口与两台设备。[发行状态](release-status.md)
