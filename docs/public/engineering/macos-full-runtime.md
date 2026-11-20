# Apple Silicon runtime distribution

The macOS DMG contains the client, Android and Linux baselines, local services and their runtimes. The client manages device creation.

The host requirement is an Apple Silicon Mac running macOS 15.0 or later, at least 12 GiB RAM and 24 GiB free space at initial deployment. The hardware and space checks run before device creation. Existing user data is kept separately from versioned program files.

## Contents

| Resource | Included implementation |
|---|---|
| Workbench | Iced and wgpu |
| Agent and phone services | Host-native Rust assistant/runner and CPU OCR helper using managed Tesseract |
| Phone integration | Prebuilt input/notification and launcher applications |
| Linux host | App-owned Lima 2.2.1, Alpine boot image and offline native runtime packages |
| Media and connections | Compiled services and their locked production dependencies |
| Device networking | Direct by default, with an optional workbench proxy |
| Android | AOSP Android 17 ARM64 (build 16644040), Avo 1.1.0 and ADB 36.0.1. [Sources and staging](android-runtime.md) |
| Linux desktop and workspace | One shared Debian filesystem |
| Data services | Offline Alpine PostgreSQL 17 and Redis packages, started directly in the Lima guest |

PostgreSQL and Redis listen on loopback ports 55482 and 56382. PostgreSQL stores its cluster on the installation's Linux guest disk.

Resources are sealed by SHA-256 inventories. Runtime selection checks the host, component identity and file permissions. The distribution includes an inspectable source snapshot; chat, accounts, user files and device disks are never package inputs.

## Build

The Mac factory runs on Apple Silicon and uses the shared ARM64 Linux filesystem, its matching Rust desktop bridge, Cargo and Android 35 resource tools. The launcher and input APKs are built in Rust. [Phone APK build](phone-apps.md)

```sh
AIMANITY_ANDROID_SDK_INPUT=/factory/android-sdk \
AI_PHONE_RELEASE_DESKTOP_BRIDGE=/factory/desktop-bridge \
cargo run --locked -p ai-phone-release-tools -- macos-factory --compiler-sdk "$ANDROID_HOME" \
  --linux-rootfs /factory/linux-rootfs.tar.zst
```

The Mac job seals the staged AOSP, Avo and ADB files, builds the Lima guest resources, compiles the phone apps and assembles the signed DMG with Cargo. It writes `dist/macos-full-factory-report.json` after checking the DMG contents.

To reuse a sealed Android capsule, set `AIMANITY_ANDROID_PAYLOAD` and `AIMANITY_ANDROID_PAYLOAD_SHA256` in place of `AIMANITY_ANDROID_SDK_INPUT`. In the private **Desktop installer factory** workflow, select `platform=macos` and enter the Android capsule’s `android_draft_id` and `android_payload_sha256`. The ARM64 Linux job builds the shared filesystem and desktop bridge. The Mac job assembles the DMG.

A source-matched `.tools/phone-apps` lets the factory reuse the phone APKs. With the Linux host and Android capsule already staged, packaging needs 24 GiB free.

For distribution, run the factory with `--sign-release` on a Mac with the Developer ID Application identity and the `notarytool` profile already in Keychain. Set `AIMANITY_MAC_SIGNING_IDENTITY` to the certificate name and `AIMANITY_MAC_NOTARY_PROFILE` to the profile name. The factory signs the app and DMG, submits the DMG to Apple, then staples and validates the ticket.

Private Actions storage retains the DMG, inventory and receipt. The maintainer publishes the reviewed installer to Releases as one asset under 2 GiB.

## Availability

[Release status](../release-status.md)

## 中文

macOS 安装包内置客户端、Android 和 Linux、服务及运行环境。首次部署要求 Apple Silicon、macOS 15.0、12 GiB 内存和 24 GiB 可用空间。用户数据存放在独立目录。

内置 Android 17 ARM64（构建 16644040）、Avo 1.1.0 和 ADB 36.0.1。构建时将 `AIMANITY_ANDROID_SDK_INPUT` 指向暂存目录并运行上方命令。已有离线胶囊时，改用 `AIMANITY_ANDROID_PAYLOAD` 和 `AIMANITY_ANDROID_PAYLOAD_SHA256`。[下载](../release-status.md) · [Android 源码与暂存](android-runtime.md)
