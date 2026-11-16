# Native client development

[简体中文](native-client.zh-CN.md)

## 0.0.4 Dysprosium development

`apps/native` contains the Iced + wgpu workbench. Its Rust host integration starts the phone, desktop and local services without the Next.js page server. Device frames upload directly to GPU textures; input and the shared conversation use the existing protobuf services.

The macOS development build connects real devices and preserves one conversation across device switches. Its anchored Agent editor uses the shared provider catalogue and the existing model verification/save flow. Saved profiles include connection, memory and voice settings.

The conversation supports Markdown, older-message pagination, expandable tool receipts, file selection/drop, image previews, downloads, pause, resume and stop. Pending messages are stored locally as protobuf before submission and retain their request IDs when retried. New drafts remain independent of in-flight messages. Rendering caches parsed Markdown and decoded images; idle conversations do not poll tool history.

The native component system shares typography, radii, 40px control heights and glass parameters. Anchored windows measure their content and animate their size; covered device and chat controls receive no pointer input. Device and anchored-window transitions use window frame events. Tool-history polling runs only for the current running task with its process expanded, with one final fetch when it ends.

Remaining migration work covers the other feature windows, native audio and modes, complete language/accessibility coverage, setup, updates and the Windows host entry. Visual parity is reviewed against the current workbench as each component lands.

```sh
cargo build --release --manifest-path apps/native/Cargo.toml
apps/native/target/release/aimanity-native /absolute/path/native-launch.json
```

The private launch file selects an installation with `home`, `resource`, `request`, `source`, `android` and `images`. It reuses the host runtime's offline component identifiers and device setup request. Credentials and device state stay in that installation. See the [runtime bundle layout](runtime-bundles.md).

## Released client

The Tauri client manages local setup. Both platforms run native Android. macOS uses Lima for the Linux environment. Windows starts the bundled Linux guest with QEMU and Windows Hypervisor Platform. The workbench connects to the local services after initialization.

The setup screen supports ten languages, keyboard-accessible selectors, retry and cancellation. Return to setup through **AImanity → Setup**.

## Build

```sh
./scripts/bootstrap.sh
./scripts/bun scripts/build-client.ts --with-web-runtime
```

This command builds a development client. For complete packages, see the [macOS factory](engineering/macos-full-runtime.md) and [Windows factory](windows-client-factory.md).

macOS requires Apple Silicon and macOS 15.0 or later. The Windows installer targets Windows 11 x64. [Downloads and status](release-status.md)
