# Windows Linux guest

The Windows installer carries the Linux kernel, base disk and release layer. QEMU boots the guest through Windows Hypervisor Platform. The client keeps guest writes in a persistent QCOW2 overlay and connects to the Rust services through an authenticated loopback relay.

| Resource | Contents |
|---|---|
| Guest boot | Linux kernel, immutable Calcium base disk and current read-only release layer |
| Rust services | Guest control, assistant, phone control and media, OCR and desktop bridge |
| Databases | Native PostgreSQL and Redis processes in the guest |
| Linux root filesystem | One amd64 filesystem shared by the native desktop and workspaces |
| Phone bridge | Bundled Android platform tools, input resources and audio server |
| User state | Installation under `/var/lib/aiphone` in the writable guest disk |

BusyBox launches the Rust bootstrap. The host authenticates over the named virtio-serial port `org.aiphone.bootstrap`. The client pairs Android through ADB and waits for Linux desktop readiness before completing setup.

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

Build the asset directory with `release-tools windows-guest-assets build/windows-guest-assets` on the same Linux x64 factory. It collects PostgreSQL 17, Redis 8, Tesseract and sandbox tools from signed Debian packages.

The [Linux backend checks](../../.github/workflows/linux-backend.yml) exercise the QEMU bootstrap command, backend startup and restart, database selection and image ownership. The [Android WHPX workflow](../../.github/workflows/windows-aosp-qualification.yml) boots the bundled AOSP guest. [Windows installer acceptance](../../.github/workflows/windows-installer-acceptance.yml) installs the exact signed EXE in a fresh Windows profile and drives both devices through the native client.

## 中文

Windows 客户端内置 Linux 内核、基础磁盘和只读发行层，通过 QEMU 和 Windows Hypervisor Platform 启动。PostgreSQL 和 Redis 在客体内直接运行，桌面和工作区共用随包的 amd64 根文件系统。设备、配置和聊天写入持久化磁盘。

Linux 后端检查覆盖启动、恢复和镜像归属。Windows 首装验收使用最终签名 EXE，验证原生窗口与两台设备。[发行状态](release-status.md)
