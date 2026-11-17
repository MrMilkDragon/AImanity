# Native client development

[简体中文](native-client.zh-CN.md)

## 0.0.4 Dysprosium development

`apps/native` contains the Iced + wgpu workbench. Its Rust host integration starts the phone, desktop and local services. Device frames upload directly to GPU textures; input and the shared conversation use the existing protobuf services.

The macOS development build connects real devices and preserves one conversation across device switches. Its anchored Agent editor uses the shared provider catalogue and the existing model verification/save flow. Saved profiles include connection, memory and voice settings.

The conversation supports Markdown, older-message pagination, expandable tool receipts, file selection/drop, image previews, downloads, pause, resume and stop. Pending messages are stored locally as protobuf before submission and retain their request IDs when retried. New drafts remain independent of in-flight messages. Rendering caches parsed Markdown and decoded images; idle conversations do not poll tool history.

The native component system shares typography, radii, 40px control heights and glass parameters. Anchored windows measure their content and animate their size; covered device and chat controls receive no pointer input. Device and anchored-window transitions use window frame events. Tool-history polling runs only for the current running task with its process expanded, with one final fetch when it ends.

The global toolbar opens the language selector, Skill manager, simulated location, workbench proxy, knowledge graph and document editor, three-pane file manager, Task & Goal, and device/Agent settings as anchored panels. The device toolbars control native playback volume, rotation and screenshots. Each device has an independent audio stream that continues across focus changes until muted.

Liberty and Task & Goal use the shared assistant's persisted mode state. Copilot captures the default microphone, uses the bundled offline recognizer, and reads replies with the system voice. Partial recognition appears in the composer; final text enters the shared message outbox. Building the native client requires CMake for the bundled recognizer.

Install Dysprosium from Releases on macOS or Windows. It opens its own data directory and receives later native releases through its updater. The Windows host starts the bundled Android and WHPX desktop and relays local protobuf services over authenticated loopback.

```sh
cargo build --release --manifest-path apps/native/Cargo.toml
apps/native/target/release/aimanity-native /absolute/path/native-launch.json
```

The private launch file selects an installation with `home`, `resource`, `request`, `source`, `android` and `images`. It reuses the host runtime's offline component identifiers and device setup request. Credentials and device state stay in that installation. See the [runtime bundle layout](runtime-bundles.md).

The platform factory packages the Rust workbench with the complete offline payload. The Windows factory runs on Windows 11 x64.

## Released client

The Rust native client manages local setup. Both platforms run native Android. macOS uses Lima for the Linux environment. Windows starts the bundled Linux guest with QEMU and Windows Hypervisor Platform. The workbench connects to the local services after initialization.

The setup screen supports ten languages, keyboard-accessible selectors, retry and cancellation. Return to setup through **AImanity → Setup**.

## Build

```sh
cargo build --release --locked --manifest-path apps/native/Cargo.toml
```

This command builds a development client. For complete packages, see the [macOS factory](engineering/macos-full-runtime.md) and [Windows factory](windows-client-factory.md).

macOS requires Apple Silicon and macOS 15.0 or later. The Windows installer targets Windows 11 x64. [Downloads and status](release-status.md)
