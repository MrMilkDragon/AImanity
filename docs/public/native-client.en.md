# Native client development

[简体中文](native-client.zh-CN.md)

## 0.0.4 Dysprosium development

`apps/native` contains the Iced + wgpu workbench. Its Rust host integration starts the phone, desktop and local services without the Next.js page server. Device frames upload directly to GPU textures; input and the shared conversation use the existing protobuf services.

The macOS development build currently connects real devices, handles direct input, switches device layouts and opens an anchored Agent selector. The component migration follows the existing workbench's glass, typography, controls and animations. Full feature windows, native audio, setup, updates and the Windows host entry are the next integration stages.

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
