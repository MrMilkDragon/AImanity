# Runtime bundles

AI Phone separates versioned application code, clean device baselines and persistent user data. The [Apple Silicon](engineering/macos-full-runtime.md) and [Windows](windows-client-factory.md) build guides define the seven resource groups required by a complete distribution. Published installers and platform qualification are listed in [release status](release-status.md).

## Package format

`release-tools client-payload` creates a deterministic `source.payload` archive and an integrity record. The client runs its native Rust initializer. Format 1 contains reviewed source. Format 2 declares a nonempty subset in this order: `native-services`, `phone-apps`, `container-host`.

| Component | Contents | Launch requirement |
|---|---|---|
| Native services | Release Rust assistant and phone runner, matched Rust PNG/OCR helper and generated fixture | Tesseract runs directly in the macOS Linux guest or the Windows [Linux offline closure](engineering/linux-offline-ocr.md) |
| Phone applications | Unicode input/notification service and no-drawer launcher APKs | Source fingerprint, complete inventory and APK hashes must match; factory signatures/package IDs are verified |
| macOS Linux host | App-owned Lima, Alpine ISO and offline native runtime packages | [Host component and ownership](container-host-runtime.md) |
| Android runtime | Matched emulator, platform tools and clean Android factory image | Host/guest ABI, complete file inventory and archive integrity must match; redistribution qualification is separate |
| Windows Linux guest | QEMU, WHPX, Linux kernel and QCOW2 base disk | Setup enables Windows Hypervisor Platform and creates a persistent overlay |
| Shared Linux filesystem | Linux desktop, browser, and work environment | The Linux guest stages the matching Zstandard archive |
| Data services | PostgreSQL and Redis run directly in the Linux guests | macOS supplies Alpine packages. Windows supplies dedicated executables and libraries |


Each component manifest records file sizes, SHA-256 hashes and permissions. Native builds also record source fingerprints and compiler provenance. The packager accepts only matching builds. macOS helpers cannot require a newer OS than the client.

## Installation and startup

The native Rust initializer stages bundled source, Android and image resources directly. The Windows Linux guest uses the same native initializer. Guest task tools are independent of first-party runtime code.

The bundled [phone control applications](engineering/phone-apps.md) provide input, notification and launcher integration without first-run APK compilation. Supplying an Android runtime without these applications is rejected before device creation. Source contributor installations retain the compiler-based path.

The staging entry validates bounded archive headers and inventory before writing an immutable release directory. Duplicate entries, malformed paths, private paths, escaping aliases, unlisted files, changed bytes and occupied destinations are rejected. The version's `data` and `.env` link to the persistent `user` directory; staging a new version does not activate it or replace existing devices, credentials or conversations.

Service launchers verify installed native components before choosing their command. A present but invalid component fails explicitly. macOS and Linux use the prebuilt Rust binaries when supplied, and the runner receives the matching OCR path. Source-only installations retain contributor build commands. Messaging channels are included in the Rust assistant binary. Browser and desktop applications are supplied by the included desktop image; Android applications and the emulator/image have their own component inventories.


Each platform ships one complete installer under the GitHub asset limit.

## Component build and verification

These commands are for maintainers building components. Native OCR qualification requires a prepared backend. Set `AI_PHONE_TEST_TESSERACT` to the host Tesseract executable and `AI_PHONE_TEST_TESSDATA` to the directory produced by `cargo run --locked -p ai-phone-release-tools -- prepare-ocr-models --destination <directory>`. The pinned English and Simplified Chinese models work with the Rust helper's TSV output.

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- native-services build
cargo run --locked -p ai-phone-release-tools -- client-payload --with-service-runtime
```

For full distribution commands and factory-resource requirements, use the [Apple Silicon build guide](engineering/macos-full-runtime.md#build) or [Windows factory guide](windows-client-factory.md). The commands above exercise individual components.

Build on the matching host. Windows carries native Android and its matched QEMU Linux guest with Rust services.

Verification stages the archive into an empty temporary installation, executes the native version commands and recognizes a generated OCR image. A disposable PostgreSQL fixture checks embedded migrations and the runtime role.

## Privacy and licensing

The packager checks paths and credentials. Binary packages include upstream notices and applicable corresponding source.

## Complete distribution requirements

| Component | Acceptance required |
|---|---|
| Android and emulator | Clean redistributable baseline, matching host ABI, offline device creation |
| Linux guest | Offline shared filesystem and owned process supervision |
| Media, channels and browser | Matching prebuilt dependencies and real startup checks |
| Windows host | Independent services and both guest devices |
| Updates | Signed artifacts, backup, migrations and rollback activation |
| First run | Client resource/model configuration followed by a usable workbench |

Physical installation on Apple Silicon macOS and Windows x64 remains unverified. See the [engineering records](engineering/index.md) for component checks and [release status](release-status.md) for package and redistribution status.

## 中文

客户端将版本化程序、设备基础和持久用户数据分开。工作台使用 Iced 和 wgpu，原生运行组件包含 Rust 助手、手机执行器与 OCR。

归档在空白目录中验证程序版本和 OCR。数据库验证使用专用的回环临时实例，检查迁移和受限运行角色。手机与电脑共用 Rust PNG/OCR 流程。Tesseract 执行识别，Rust 负责画面摘要、文字边界和置信度解析。

macOS 安装包包含七组资源。Windows 安装包包含原生 Android 和 QEMU Linux 客体。首启、升级恢复和许可的核对结果见[发行状态](release-status.md)。

## Shared Linux filesystem

The native Linux profile uses one Zstandard-compressed desktop filesystem for the desktop and terminal workspace. The desktop has a persistent writable overlay and home. Workspace jobs mount the same system read-only and use a separate job namespace.

Build the filesystem on a matching Linux builder with debootstrap, the Debian Trixie keyring, GNU tar, wget and Zstandard. Set `AI_PHONE_RELEASE_DESKTOP_BRIDGE` to the matching Linux guest ELF before staging the installer.

```sh
cargo build --locked -p ai-phone-release-tools
sudo target/debug/release-tools factory-images rootfs arm64 build/linux-rootfs.tar.zst
AI_PHONE_RELEASE_DESKTOP_BRIDGE="$PWD/build/desktop-bridge" \
  cargo run --locked -p ai-phone-release-tools -- macos-factory --linux-rootfs build/linux-rootfs.tar.zst
```

For Windows, build the `amd64` filesystem and the native guest assets, then run `windows-release-context OUTPUT --linux-rootfs FILE --guest-assets DIR`. The installer stores the filesystem once in the existing platform asset. The two signed installer profiles have separate first-install acceptance.
